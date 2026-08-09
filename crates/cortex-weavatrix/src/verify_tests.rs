use super::*;
use crate::EvidenceFragment;

fn fragment(id: &str, kind: EvidenceKind, content: &str) -> EvidenceFragment {
    EvidenceFragment {
        id: id.to_owned(),
        kind,
        source: "test".to_owned(),
        content: content.to_owned(),
        head: true,
    }
}

#[test]
fn config_context_is_thin_until_search_and_source_both_survive() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                r#"{"matches":[{"path":"config/a.json","line":2}]}"#,
            ),
            fragment("WX-SOURCE", EvidenceKind::SourceReads, "CORTEX_LLM"),
        ],
        warnings: Vec::new(),
    };
    let hints = PlanHints {
        intent: Some(crate::IntentHint::RuntimeConfig),
        source_followup: Some(true),
        skip_change_plan: true,
    };
    let thin = assess_compiled(
        &bundle,
        &["WX-SEARCH".to_owned()],
        "Inspect `CORTEX_LLM`",
        None,
        hints,
        true,
        false,
    );
    assert_eq!(
        thin.missing_evidence,
        ["source_reads", "source_term:identifier:CORTEX_LLM"]
    );
    let enough = assess_compiled(
        &bundle,
        &["WX-SEARCH".to_owned(), "WX-SOURCE".to_owned()],
        "Inspect `CORTEX_LLM`",
        None,
        hints,
        true,
        true,
    );
    assert!(enough.sufficient);
    assert!(enough.retry_performed);
}

#[test]
fn profile_gate_requires_semantic_source_coverage_not_just_source_presence() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                r#"{"matches":[{"path":"crates/cortex-llm/src/profile.rs","line":1}]}"#,
            ),
            fragment(
                "WX-SOURCE",
                EvidenceKind::SourceReads,
                "pub struct ProfileRegistry;",
            ),
        ],
        warnings: Vec::new(),
    };
    let hints = PlanHints {
        intent: Some(crate::IntentHint::RuntimeConfig),
        source_followup: Some(true),
        skip_change_plan: true,
    };
    let report = assess_compiled(
        &bundle,
        &["WX-SEARCH".to_owned(), "WX-SOURCE".to_owned()],
        "How does `ProfileRegistry` refuse an uncalibrated classification profile?",
        Some("ProfileRegistry"),
        hints,
        true,
        false,
    );
    assert!(!report.sufficient);
    assert!(
        report
            .missing_evidence
            .iter()
            .any(|item| item == "source_term:profile_gate_state")
    );
    assert!(
        report
            .missing_evidence
            .iter()
            .any(|item| item == "source_term:profile_rejection")
    );
}

#[test]
fn semantic_retry_rebuilds_the_complete_contract_packet() {
    let profile = retry_search_pattern(
        "How does `ProfileRegistry` refuse an uncalibrated classification profile?",
        Some("ProfileRegistry"),
        PlanHints::default(),
        &["source_term:profile_selection".to_owned()],
    );
    assert!(profile.contains("select"));
    assert!(profile.contains("gate_passed"));
    assert!(profile.contains("NotCalibrated"));

    let shadow = retry_search_pattern(
        "How is `ShadowHandle` spawned, and which env flag turns shadow mode on?",
        Some("ShadowHandle"),
        PlanHints::default(),
        &["source_term:spawn_lifecycle".to_owned()],
    );
    assert!(shadow.contains("spawn"));
    assert!(shadow.contains("observe"));
    assert!(shadow.contains("CORTEX_[A-Z0-9_]*SHADOW"));
}

#[test]
fn identifier_only_semantic_retry_keeps_its_search_query() {
    let identifier = ["compile", "Markdown"].concat();
    let task = format!("Where does the `{identifier}` client call live?");
    let queries = retry_search_queries(
        &task,
        None,
        PlanHints::default(),
        &[format!("source_term:identifier:{identifier}")],
    );
    assert_eq!(queries, [identifier]);
}
