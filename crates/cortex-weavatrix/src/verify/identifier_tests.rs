use super::*;
use crate::EvidenceFragment;

fn fragment(id: &str, kind: EvidenceKind, content: &str) -> EvidenceFragment {
    EvidenceFragment::new(id, kind, "test", content)
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
