use super::{
    TaskIntent, asks_for_dead_production, asks_for_duplicate_share, detect, is_broad, is_creation,
};

#[test]
fn enumerating_questions_are_broad_and_pointed_ones_are_not() {
    assert!(is_broad(
        "A regex matches a file on disk but returns nothing inside a .tar.gz. \
             List every mechanism in this crate that can silently cause that."
    ));
    assert!(is_broad("What can cause the collector to drop matches?"));
    assert!(!is_broad("Rename `read_limited` in containers.rs"));
    assert!(!is_broad(
        "Who depends on `route` if its signature changes?"
    ));
}

#[test]
fn creation_cues_are_limited_to_the_task_opening() {
    assert!(is_creation("Implement `ArchiveOptions::disabled()`"));
    assert!(is_creation("Please add `ArchiveOptions::disabled()`"));
    assert!(!is_creation(
        "Who calls `ArchiveOptions::disabled()` after the change?"
    ));
}

#[test]
fn blast_contract_and_topology_cues_are_recognised() {
    let impact = "How do changes to `compile_evidence_bundle` affect its callers and the MCP/HTTP entry points used by `cortex_prepare`?";
    assert_eq!(detect(impact), TaskIntent::BlastRadius);
    assert!(super::asks_for_caller_impact(impact));
    assert!(super::asks_for_endpoint_impact(impact));
    assert!(super::asks_for_endpoint_impact(
        "Trace compile_context rejection effects through the MCP agent profile and HTTP transport"
    ));
    assert!(super::asks_for_caller_impact(
        "Investigate the call chain from compile_evidence_bundle through the MCP agent profile"
    ));
    assert_eq!(
        detect("Who depends on compile_context if its signature changes?"),
        TaskIntent::BlastRadius
    );
    assert_eq!(
        detect("What breaks if the POST /api/skills/compile HTTP contract changes?"),
        TaskIntent::ApiContract
    );
    assert_eq!(
        detect("Which services read the Streamable HTTP MCP transport at `/mcp`?"),
        TaskIntent::ApiContract
    );
    assert_eq!(
        detect("Which module owns compile_context, and where does the crate layout put it?"),
        TaskIntent::ModuleTopology
    );
    assert_eq!(
        detect("Rename RetryLimitTooLarge in retry.rs"),
        TaskIntent::IdentifierChange
    );
    assert_eq!(
        detect("How does CORTEX_LLM read config/llm-profiles.json?"),
        TaskIntent::RuntimeConfig
    );
    assert_eq!(
        detect("Which env flag enables ShadowHandle?"),
        TaskIntent::RuntimeConfig
    );
    assert_eq!(
        detect("Who changed `compile_context` last?"),
        TaskIntent::GitHistory
    );
    assert_eq!(
        detect("Which tests should I run after changing compile_context?"),
        TaskIntent::TestSelection
    );
    assert_eq!(
        detect("thread 'main' panicked at src/retry.rs:12:1:\nstack backtrace:"),
        TaskIntent::StackTrace
    );
    assert_eq!(
        detect("Fix the failing unit test in the graph store"),
        TaskIntent::IdentifierChange
    );
    assert_eq!(
        detect("Still failing compile_context after the last attempt"),
        TaskIntent::PriorAttempt
    );
    assert_eq!(
        detect("какие тесты запустить после compile_context"),
        TaskIntent::TestSelection
    );
    assert_eq!(
        detect("кто вызывает `route` и что сломается"),
        TaskIntent::BlastRadius
    );
    assert!(is_broad("Перечисли все механизмы молчаливого пропуска"));
    assert_eq!(
        detect("предыдущая попытка не собрала formatGroupedResult"),
        TaskIntent::PriorAttempt
    );
    assert!(asks_for_dead_production(
        "Find and fix one real bug or dead production path in crates/sweeploom-cli."
    ));
    assert!(asks_for_duplicate_share(
        "Find, verify, and eliminate duplicate classifier logic in crates/sweeploom-ai."
    ));
    assert!(!asks_for_duplicate_share(
        "Find and fix one real bug or dead production path."
    ));
}

#[test]
fn coding_task_with_test_instruction_keeps_code_change_intent() {
    let task = "Add an Elevated priority to the evidence compiler; update the schema and run relevant tests.";
    assert_eq!(detect(task), TaskIntent::IdentifierChange);
    assert!(super::is_coding_change(task));
    assert_eq!(
        detect("Which tests should I run after I fix the priority compiler?"),
        TaskIntent::TestSelection
    );
    assert!(!super::asks_for_ui("build a compiler"));
    assert!(super::asks_for_ui("update the UI help text"));
}
