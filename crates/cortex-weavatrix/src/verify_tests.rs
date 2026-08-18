use super::*;
use crate::EvidenceFragment;

fn fragment(id: &str, kind: EvidenceKind, content: &str) -> EvidenceFragment {
    EvidenceFragment::new(id, kind, "test", content)
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
        ..EvidenceBundle::default()
    };
    let hints = PlanHints {
        intent: Some(crate::IntentHint::RuntimeConfig),
        source_followup: Some(true),
        skip_change_plan: true,
        has_prior_attempts: false,
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
        ..EvidenceBundle::default()
    };
    let hints = PlanHints {
        intent: Some(crate::IntentHint::RuntimeConfig),
        source_followup: Some(true),
        skip_change_plan: true,
        has_prior_attempts: false,
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
        ..EvidenceBundle::default()
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
        ..EvidenceBundle::default()
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
        ..EvidenceBundle::default()
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
fn a_graph_span_declares_completeness_without_braces() {
    let mut fragment = fragment(
        "WX-DEF",
        EvidenceKind::SourceReads,
        "def helper():\n    return 1\n",
    );
    fragment.facet = cortex_context::EvidenceFacet::Definition;
    fragment.declared_complete = Some(true);
    fragment.locator.path = Some("src/helper.py".to_owned());
    fragment.locator.start_line = Some(1);
    fragment.locator.end_line = Some(2);
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![fragment],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let report = assess_compiled(
        &bundle,
        &["WX-DEF".to_owned()],
        "Change `helper`",
        Some("helper"),
        PlanHints::default(),
        true,
        false,
    );
    assert!(
        report
            .present_evidence
            .iter()
            .any(|item| item == "definition:helper"),
        "{report:?}"
    );
}

#[test]
fn split_definition_pieces_complete_when_they_share_a_group() {
    let first_half = "pub struct ArchiveOptions {\n    pub enabled: bool,\n";
    let second_half = "    pub max_entries: usize,\n}\n";
    let mut first = fragment("WX-DEF-1", EvidenceKind::SourceReads, first_half);
    let mut second = fragment("WX-DEF-2", EvidenceKind::SourceReads, second_half);
    first.group_id = Some("def-archive".to_owned());
    second.group_id = Some("def-archive".to_owned());
    first.facet = cortex_context::EvidenceFacet::Definition;
    second.facet = cortex_context::EvidenceFacet::Definition;
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![first, second],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let report = assess_compiled(
        &bundle,
        &["WX-DEF-1".to_owned(), "WX-DEF-2".to_owned()],
        "Change `ArchiveOptions`",
        Some("ArchiveOptions"),
        PlanHints::default(),
        true,
        false,
    );
    assert!(
        report
            .present_evidence
            .iter()
            .any(|item| item == "definition:archiveoptions"),
        "{report:?}"
    );
}

#[test]
fn first_pass_implied_queries_include_probe_retry_terms() {
    let profile = implied_coverage_queries(
        "How does `ProfileRegistry` refuse an uncalibrated classification profile?",
        Some("ProfileRegistry"),
        PlanHints::default(),
    );
    let joined = profile.join(" ");
    assert!(joined.contains("gate_passed"), "{profile:?}");
    assert!(joined.contains("NotCalibrated"), "{profile:?}");
    assert!(joined.contains("fn select"), "{profile:?}");

    let compile = implied_coverage_queries(
        "Who calls `compile_evidence_bundle` versus `compile_probe_bundle`?",
        Some("compile_evidence_bundle"),
        PlanHints::default(),
    );
    assert!(
        !compile.iter().any(|query| query.contains("build_server")),
        "compile-bundle probe must not pick up compile_context siblings: {compile:?}"
    );
}

#[test]
fn named_identifiers_may_close_from_search_hits() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                "search matches: 1\ncrates/cortex-mcp/src/route_metric_tools.rs:189: quality_summary",
            ),
            fragment(
                "WX-SOURCE",
                EvidenceKind::SourceReads,
                "tools/list\nweavatrix_context_compile\n",
            ),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let report = assess_compiled(
        &bundle,
        &["WX-SEARCH".to_owned(), "WX-SOURCE".to_owned()],
        "Expose the token-accounting `quality_summary` as a bounded MCP \
tool alongside the existing `usage_read` and `usage_report` tools.",
        None,
        PlanHints::default(),
        true,
        false,
    );
    assert!(
        !report
            .missing_evidence
            .iter()
            .any(|item| item == "source_term:identifier:quality_summary"),
        "identifier still missing from search: {:?}",
        report.missing_evidence
    );
    assert!(
        report
            .missing_evidence
            .iter()
            .any(|item| item == "source_term:identifier:usage_read"),
        "semantic-style identifiers without a search hit must stay open: {:?}",
        report.missing_evidence
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

#[test]
fn quiet_result_mode_requires_the_quiet_path() {
    let task =
        "How does multiline search group matches, and what does quiet result mode do instead?";
    let thin = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                "search matches: 1\nsrc/multiline/mod.rs:10: fn finish_block",
            ),
            fragment(
                "WX-DEF",
                EvidenceKind::SourceReads,
                "fn finish_block() { if end_line { } }",
            ),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let report = assess_compiled(
        &thin,
        &["WX-SEARCH".to_owned(), "WX-DEF".to_owned()],
        task,
        Some("finish_block"),
        PlanHints::default(),
        true,
        false,
    );
    assert!(
        report
            .missing_evidence
            .iter()
            .any(|item| item == "source_term:quiet_path"),
        "missing was {:?}",
        report.missing_evidence
    );
}

#[test]
fn a_block_join_question_requires_block_and_end_line() {
    let task = "How does multiline search group matches into a single reported block when a new match joins?";
    let thin = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![fragment(
            "WX-DEF",
            EvidenceKind::SourceReads,
            "fn finish_block() {}",
        )],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let report = assess_compiled(
        &thin,
        &["WX-DEF".to_owned()],
        task,
        Some("finish_block"),
        PlanHints::default(),
        true,
        false,
    );
    for term in ["block_type", "join_condition"] {
        assert!(
            report
                .missing_evidence
                .iter()
                .any(|item| item == &format!("source_term:{term}")),
            "missing {term} from {:?}",
            report.missing_evidence
        );
    }
}

#[test]
fn a_broad_silent_miss_packet_is_thin_without_option_limits_and_path_guard() {
    let task = "A regex matches a file on disk but returns nothing when the same file sits inside a .tar.gz. \
         List every mechanism in this crate that can silently cause that.";
    let thin = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                "search matches: 1\nsrc/archive/compression.rs:9: fn search_compressed_tar",
            ),
            fragment(
                "WX-DEF",
                EvidenceKind::SourceReads,
                "fn search_compressed_tar() { read_limited(options.archives.max_expanded_bytes) }",
            ),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let included = ["WX-SEARCH".to_owned(), "WX-DEF".to_owned()];
    let report = assess_compiled(
        &thin,
        &included,
        task,
        Some("search_compressed_tar"),
        PlanHints::default(),
        true,
        false,
    );
    assert!(!report.sufficient);
    for term in ["option_enabled", "count_limit", "path_guard"] {
        assert!(
            report
                .missing_evidence
                .iter()
                .any(|item| item == &format!("source_term:{term}")),
            "missing {term} from {:?}",
            report.missing_evidence
        );
    }

    let enough = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                "search matches: 1\nsrc/archive/compression.rs:9: fn search_compressed_tar",
            ),
            fragment(
                "WX-DEF",
                EvidenceKind::SourceReads,
                "fn search_compressed_tar() {}",
            ),
            fragment(
                "WX-TYPE-1",
                EvidenceKind::TypeExpansion,
                "pub struct ArchiveOptions {\n    pub enabled: bool,\n    pub max_entries: usize,\n}",
            ),
            fragment(
                "WX-SOURCE-2",
                EvidenceKind::SourceReads,
                "fn safe_virtual_path(path: &str) -> Option<&str> { path.contains(\"../\") }",
            ),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let filled = assess_compiled(
        &enough,
        &[
            "WX-SEARCH".to_owned(),
            "WX-DEF".to_owned(),
            "WX-TYPE-1".to_owned(),
            "WX-SOURCE-2".to_owned(),
        ],
        task,
        Some("search_compressed_tar"),
        PlanHints::default(),
        true,
        false,
    );
    assert!(
        filled.sufficient,
        "unexpected missing evidence: {:?}",
        filled.missing_evidence
    );
}

#[test]
fn git_stack_and_test_intents_require_their_native_kinds() {
    let empty = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: Vec::new(),
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let git = assess_compiled(
        &empty,
        &[],
        "Who changed this file last?",
        None,
        PlanHints::default(),
        false,
        false,
    );
    assert!(git.required_evidence.contains(&"git_history".to_owned()));
    assert!(git.missing_evidence.contains(&"git_history".to_owned()));

    let stack = assess_compiled(
        &empty,
        &[],
        "thread 'main' panicked at src/retry.rs:12:1",
        None,
        PlanHints::default(),
        false,
        false,
    );
    assert!(stack.required_evidence.contains(&"stack_trace".to_owned()));

    let tests = assess_compiled(
        &empty,
        &[],
        "Which tests should I run after this change?",
        None,
        PlanHints::default(),
        false,
        false,
    );
    assert!(
        tests
            .required_evidence
            .contains(&"test_selection".to_owned())
    );

    let hints = PlanHints {
        has_prior_attempts: true,
        ..PlanHints::default()
    };
    let memory = assess_compiled(
        &empty,
        &[],
        "Still failing after the last attempt",
        None,
        hints,
        false,
        false,
    );
    assert!(memory.required_evidence.contains(&"memory".to_owned()));
    assert!(memory.missing_evidence.contains(&"memory".to_owned()));
}
