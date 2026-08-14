use std::path::Path;

use crate::manifest::{BenchmarkManifest, McpManifest};
use crate::naive::{matches, scan};
use crate::probe_tasks;
use crate::schedule::alternating_orders;
use crate::scoreboard::{FailureClass, ScoreboardRow, has_unclassified_failures};
use crate::tasks::tasks;
use crate::{Anchor, ArmKind, BenchReport, TaskResult, measure, token_delta, unavailable};

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
    for task in tasks().iter().chain(probe_tasks::probe_tasks().iter()) {
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

#[test]
fn manifest_observes_versions_and_revisions_instead_of_accepting_labels() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let manifest = BenchmarkManifest::detect(
        "context-probe-v2",
        &root,
        &[
            "cortex-bench".to_owned(),
            "--set".to_owned(),
            "probe".to_owned(),
        ],
        McpManifest::in_process(),
    );

    assert_eq!(manifest.report_schema, "cortex-benchmark.v2");
    assert_eq!(
        manifest.target.commit.value.as_deref().map(str::len),
        Some(40)
    );
    assert_eq!(
        manifest.cortex.commit.value.as_deref().map(str::len),
        Some(40)
    );
    assert_eq!(
        manifest.engines["weavatrix-rust"].value.as_deref(),
        Some("2.6.0")
    );
    assert_eq!(
        manifest.engines["npm-weavatrix"].value.as_deref(),
        Some("1.8.0")
    );
    assert_eq!(
        manifest.mcp.payload_representation,
        "serialized-tool-payload"
    );
}

#[test]
fn competitive_schedule_changes_first_and_last_arms_across_three_trials() {
    let arms = vec!["a", "b", "c", "d"];
    let orders = alternating_orders(&arms, 3);

    assert_eq!(orders[0], ["a", "b", "c", "d"]);
    assert_eq!(orders[1], ["d", "c", "b", "a"]);
    assert_eq!(orders[2], ["c", "d", "a", "b"]);
    assert_ne!(orders[0].first(), orders[1].first());
    assert_ne!(orders[1].last(), orders[2].last());

    let paired = alternating_orders(&["a", "b"], 3);
    assert_eq!(paired, [["a", "b"], ["b", "a"], ["a", "b"]]);
}

#[test]
fn sufficient_failed_tasks_are_false_confidence_and_need_an_owner() {
    let mut row = ScoreboardRow::new("probe", "task", "cortex-source", 1, 3, 4);
    row.sufficient = Some(true);
    row.task_success = false;
    row.refresh_verdict();

    assert!(row.false_confidence);
    assert!(has_unclassified_failures(std::slice::from_ref(&row)));

    row.failure_class = Some(FailureClass::CortexBug);
    assert!(!has_unclassified_failures(&[row]));
}

#[test]
fn context_report_serializes_manifest_and_false_confidence_rows() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let mut arm = measure(ArmKind::CortexLoomSource, "unrelated", 1, ANCHORS);
    arm.sufficient = Some(true);
    arm.refresh_verdict();
    let report = BenchReport::new(
        &root,
        4_000,
        0,
        Some("p0-test".to_owned()),
        vec![TaskResult {
            task_id: "manifest-task".to_owned(),
            prompt: "test".to_owned(),
            budget: 4_000,
            anchor_count: ANCHORS.len(),
            arms: vec![arm],
        }],
        &[
            "cortex-bench".to_owned(),
            "--set".to_owned(),
            "probe".to_owned(),
        ],
    );
    let value = serde_json::to_value(report).unwrap();

    assert_eq!(value["schemaVersion"], "cortex-benchmark.v2");
    assert_eq!(value["historical"], false);
    assert_eq!(
        value["manifest"]["engines"]["weavatrix-rust"]["value"],
        "2.6.0"
    );
    assert_eq!(value["scoreboard"][0]["falseConfidence"], true);
    assert_eq!(value["scoreboard"][0]["failureClass"], "CORTEX_BUG");
}

#[test]
fn sequence_report_is_self_describing_too() {
    let report = crate::sequence_arms::run(None).unwrap();
    let value = serde_json::to_value(report).unwrap();

    assert_eq!(value["schemaVersion"], "cortex-benchmark.v2");
    assert_eq!(value["historical"], false);
    assert_eq!(value["manifest"]["reportSchema"], "cortex-benchmark.v2");
    assert!(
        value["scoreboard"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())
    );
}
