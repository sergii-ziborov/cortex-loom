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

/// The measured failure: a six-field struct clipped after four fields passed
/// sufficiency, and the model faithfully implemented the four fields it saw.
#[test]
fn a_truncated_definition_of_the_named_symbol_is_insufficient() {
    let truncated = "pub struct ArchiveOptions {\n    pub enabled: bool,\n    pub max_archive_bytes: u64,\n    pub max_entry_bytes: u64,\n    pub max_expanded_bytes: u64,\n";
    let complete = "pub struct ArchiveOptions {\n    pub enabled: bool,\n    pub max_archive_bytes: u64,\n    pub max_entry_bytes: u64,\n    pub max_expanded_bytes: u64,\n    pub max_entries: usize,\n    pub max_decoder_memory_bytes: usize,\n}\n";
    let bundle = |body: &str| EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                r#"{"matches":[{"path":"src/options/types.rs","line":87}]}"#,
            ),
            fragment("WX-DEF", EvidenceKind::SourceReads, body),
        ],
        warnings: Vec::new(),
    };
    let included = ["WX-SEARCH".to_owned(), "WX-DEF".to_owned()];
    let task = "Add a constructor to `ArchiveOptions` with every limit zero";

    let clipped = assess_compiled(
        &bundle(truncated),
        &included,
        task,
        Some("ArchiveOptions"),
        PlanHints::default(),
        true,
        false,
    );
    assert!(!clipped.sufficient);
    assert!(
        clipped
            .missing_evidence
            .iter()
            .any(|item| item == "definition:archiveoptions"),
        "missing was {:?}",
        clipped.missing_evidence
    );

    let whole = assess_compiled(
        &bundle(complete),
        &included,
        task,
        Some("ArchiveOptions"),
        PlanHints::default(),
        true,
        false,
    );
    assert!(
        !whole
            .missing_evidence
            .iter()
            .any(|item| item.starts_with("definition:")),
        "missing was {:?}",
        whole.missing_evidence
    );
}

#[test]
fn creating_a_named_member_requires_its_owner_not_the_future_member() {
    let complete = "pub struct ArchiveOptions {\n    pub enabled: bool,\n    pub max_archive_bytes: u64,\n    pub max_entry_bytes: u64,\n    pub max_expanded_bytes: u64,\n    pub max_entries: usize,\n    pub max_decoder_memory_bytes: usize,\n}\n";
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                r#"{"matches":[{"path":"src/options/types.rs","line":87,"text":"pub struct ArchiveOptions"}]}"#,
            ),
            fragment("WX-DEF", EvidenceKind::SourceReads, complete),
        ],
        warnings: Vec::new(),
    };

    let report = assess_compiled(
        &bundle,
        &["WX-SEARCH".to_owned(), "WX-DEF".to_owned()],
        "Implement `ArchiveOptions::disabled()` with every limit zero",
        Some("ArchiveOptions"),
        PlanHints::default(),
        true,
        false,
    );

    assert!(report.sufficient, "unexpected missing evidence: {report:?}");
    assert!(
        !report
            .required_evidence
            .iter()
            .any(|item| item.contains("ArchiveOptions::disabled"))
    );
}

/// The definition must balance inside one fragment: two windows that each
/// hold half the struct do not add up to a definition anyone can read.
#[test]
fn definition_completeness_does_not_sum_across_fragments() {
    let first_half = "pub struct ArchiveOptions {\n    pub enabled: bool,\n";
    let second_half = "    pub max_entries: usize,\n}\n";
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment("WX-SOURCE-1", EvidenceKind::SourceReads, first_half),
            fragment("WX-SOURCE-2", EvidenceKind::SourceReads, second_half),
        ],
        warnings: Vec::new(),
    };
    let report = assess_compiled(
        &bundle,
        &["WX-SOURCE-1".to_owned(), "WX-SOURCE-2".to_owned()],
        "Change `ArchiveOptions`",
        Some("ArchiveOptions"),
        PlanHints::default(),
        true,
        false,
    );
    assert!(
        report
            .missing_evidence
            .iter()
            .any(|item| item == "definition:archiveoptions")
    );
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
