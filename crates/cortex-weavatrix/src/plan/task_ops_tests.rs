use super::{CLASSIFIER_SEARCH, named_git_revision, task_specific_ops};
use crate::plan::{PlanPolicy, plan};

#[test]
fn a_named_file_is_read_whole_even_when_passed_as_the_compile_symbol() {
    let operations = plan(
        "Split the 506-line crates/sweeploom-cli/src/api.rs into cohesive modules.",
        Some("crates/sweeploom-cli/src/api.rs"),
        4_000,
    );
    assert!(
        operations
            .iter()
            .any(|operation| operation.tool == "read_source"
                && operation.arguments["path"] == "crates/sweeploom-cli/src/api.rs"),
        "named file must be a whole read_source, got {operations:?}"
    );
    assert!(
        operations
            .iter()
            .all(|operation| operation.tool != "context_bundle"),
        "a path is not a context_bundle label, got {operations:?}"
    );
}

#[test]
fn a_duplicate_classifier_ask_names_live_functions_and_clone_families() {
    let task = "Find, verify, and eliminate duplicate classifier logic in crates/sweeploom-ai \
                without merging the two baselines.";
    let operations = plan(task, None, 4_000);
    let search = operations
        .iter()
        .find(|operation| operation.id == "WX-SEARCH")
        .expect("classifier share still searches");
    assert_eq!(
        search.arguments["query"].as_str(),
        Some(CLASSIFIER_SEARCH),
        "search must name classifier definitions, not a generic fn dump"
    );
    assert!(
        operations
            .iter()
            .any(|operation| operation.tool == "find_duplicates"),
        "duplicate-share must plan find_duplicates, got {operations:?}"
    );
    let extras = task_specific_ops(
        task,
        &crate::plan::extract_identifiers(task),
        PlanPolicy::default(),
    );
    assert!(
        extras
            .iter()
            .any(|operation| operation.kind == crate::EvidenceKind::Duplicates)
    );
}

#[test]
fn a_dead_production_ask_stays_inside_the_named_crate() {
    let extras = task_specific_ops(
        "Find and fix one real bug or dead production path in crates/sweeploom-cli.",
        &["crates/sweeploom-cli".to_owned()],
        PlanPolicy::default(),
    );
    let dead = extras
        .iter()
        .find(|operation| operation.tool == "find_dead_code")
        .expect("dead production plans find_dead_code");
    assert_eq!(dead.arguments["path"], "crates/sweeploom-cli");
    assert_eq!(dead.arguments["include_tests"], false);
}

#[test]
fn a_named_revision_and_file_plans_git_read_blob() {
    let task = "What did crates/sweeploom-cli/src/api.rs look like at HEAD~3?";
    assert_eq!(named_git_revision(task).as_deref(), Some("HEAD~3"));
    let extras = task_specific_ops(
        task,
        &crate::plan::extract_identifiers(task),
        PlanPolicy::default(),
    );
    let blob = extras
        .iter()
        .find(|operation| operation.tool == "git_read_blob")
        .expect("historical file must use git_read_blob");
    assert_eq!(blob.arguments["path"], "crates/sweeploom-cli/src/api.rs");
    assert_eq!(blob.arguments["revision"], "HEAD~3");
    assert_eq!(named_git_revision("who changed api.rs last"), None);
    assert_eq!(named_git_revision("defaced helper"), None);
    assert_eq!(
        named_git_revision("compare e32b6c87f180 in api.rs").as_deref(),
        Some("e32b6c87f180")
    );
}

#[test]
fn a_coverage_ask_plans_ingest_only_coverage_map() {
    let extras = task_specific_ops(
        "What is the measured line coverage of crates/cortex-weavatrix?",
        &crate::plan::extract_identifiers(
            "What is the measured line coverage of crates/cortex-weavatrix?",
        ),
        PlanPolicy::default(),
    );
    let mapped = extras
        .iter()
        .find(|operation| operation.tool == "coverage_map")
        .expect("coverage ask must plan coverage_map");
    assert_eq!(mapped.kind, crate::EvidenceKind::CoverageMap);
    assert_eq!(mapped.arguments["path"], "crates/cortex-weavatrix");
    assert_eq!(mapped.arguments["top_n"], 24);

    let named = task_specific_ops(
        "Ingest line coverage from .weavatrix/coverage/lcov.info",
        &[".weavatrix/coverage/lcov.info".to_owned()],
        PlanPolicy::default(),
    );
    let report = named
        .iter()
        .find(|operation| operation.tool == "coverage_map")
        .expect("named lcov still plans coverage_map");
    assert_eq!(report.arguments.get("path"), None);
}
