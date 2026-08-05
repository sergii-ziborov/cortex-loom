use std::path::Path;

use crate::naive::{matches, scan};
use crate::tasks::tasks;
use crate::{Anchor, ArmKind, measure, token_delta, unavailable};

const ANCHORS: &[Anchor] = &[
    Anchor {
        id: "present",
        any_of: &["needle"],
    },
    Anchor {
        id: "alternative",
        any_of: &["absent", "SECOND-CHANCE"],
    },
    Anchor {
        id: "absent",
        any_of: &["nowhere"],
    },
];

#[test]
fn anchors_match_case_insensitively_on_any_alternative() {
    let arm = measure(ArmKind::Naive, "a NEEDLE and a second-chance", 1, ANCHORS);
    assert_eq!(arm.satisfied_anchors, ["present", "alternative"]);
    assert_eq!(arm.missing_anchors, ["absent"]);
    assert!((arm.recall() - 2.0 / 3.0).abs() < 1e-9);
}

#[test]
fn an_arm_that_delivers_nothing_reports_no_cost_per_fact() {
    // Otherwise an empty context would look like the cheapest strategy.
    let arm = measure(ArmKind::CortexLoom, "unrelated text", 0, ANCHORS);
    assert!(arm.satisfied_anchors.is_empty());
    assert_eq!(arm.tokens_per_fact(), None);
    assert!((arm.recall() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn unavailable_arms_are_reported_not_scored() {
    let arm = unavailable(ArmKind::WeavatrixRaw, "Weavatrix absent");
    assert!(!arm.available);
    assert_eq!(arm.unavailable_reason.as_deref(), Some("Weavatrix absent"));
    assert_eq!(arm.tokens_per_fact(), None);
    let other = measure(ArmKind::Naive, "needle", 1, ANCHORS);
    assert_eq!(token_delta(&arm, &other), None);
    assert_eq!(token_delta(&other, &arm), None);
}

#[test]
fn token_delta_reports_a_regression_as_a_negative_value() {
    let cheap = measure(ArmKind::CortexLoom, "needle", 1, ANCHORS);
    let expensive = measure(ArmKind::Naive, &"needle ".repeat(200), 1, ANCHORS);
    let saving = token_delta(&expensive, &cheap).unwrap();
    assert!(saving > 0.9, "expected a large saving, got {saving}");
    let regression = token_delta(&cheap, &expensive).unwrap();
    assert!(regression < 0.0, "a costlier arm must read negative");
}

#[test]
fn glob_matches_directory_sweeps_but_not_neighbours() {
    assert!(matches(
        "crates/cortex-run/src/*.rs",
        "crates/cortex-run/src/retry.rs"
    ));
    assert!(!matches(
        "crates/cortex-run/src/*.rs",
        "crates/cortex-run/src/deep/retry.rs"
    ));
    assert!(matches(
        "crates/cortex-run/**/*.rs",
        "crates/cortex-run/src/deep/retry.rs"
    ));
    assert!(!matches(
        "crates/cortex-run/src/*.rs",
        "crates/cortex-mcp/src/lib.rs"
    ));
    assert!(!matches(
        "crates/*/README.md",
        "crates/cortex-run/src/README.md"
    ));
    // A pattern without a wildcard is exact, not a prefix.
    assert!(!matches("crates/cortex", "crates/cortex-run"));
}

/// A fixture the generous naive arm cannot satisfy measures our fixture
/// authoring, not the arms. This test reads the real repository, so it is
/// skipped when the crate is checked out somewhere without the workspace.
#[test]
fn fixture_anchors_exist_in_the_repository() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    if !root.join("Cargo.toml").exists() {
        return;
    }
    for task in tasks() {
        let found = scan(&root, task.naive_globs).expect("scan the workspace");
        assert!(
            !found.files.is_empty(),
            "{}: no file matched {:?}",
            task.id,
            task.naive_globs
        );
        let arm = measure(
            ArmKind::Naive,
            &found.context(),
            found.files.len(),
            task.anchors,
        );
        assert!(
            arm.missing_anchors.is_empty(),
            "{}: naive sweep cannot satisfy {:?}",
            task.id,
            arm.missing_anchors
        );
    }
}
