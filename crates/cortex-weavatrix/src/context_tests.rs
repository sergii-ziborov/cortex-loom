use super::*;
use crate::EvidenceFragment;

fn fragment(id: &str, kind: EvidenceKind, content: &str) -> EvidenceFragment {
    EvidenceFragment::new(id, kind, format!("weavatrix:{id}"), content)
}

fn tail(id: &str, kind: EvidenceKind, content: &str) -> EvidenceFragment {
    EvidenceFragment {
        head: false,
        ..fragment(id, kind, content)
    }
}

#[test]
fn a_split_symbol_bundle_no_longer_refuses_a_small_budget() {
    // The head is critical and survives; the tail is high priority and is
    // truncated by the budget instead of failing the whole compile.
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment("WX-SYMBOL-1", EvidenceKind::SymbolContext, "definition"),
            tail(
                "WX-SYMBOL-2",
                EvidenceKind::SymbolContext,
                &"x".repeat(4_000),
            ),
            tail(
                "WX-SYMBOL-3",
                EvidenceKind::SymbolContext,
                &"y".repeat(4_000),
            ),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let compiled = compile_evidence_bundle(bundle, "task", 130, None).unwrap();
    assert_eq!(compiled.context.included_ids, ["TASK", "WX-SYMBOL-1"]);
    assert_eq!(
        compiled.context.omitted_ids,
        ["WX-SYMBOL-2", "WX-SYMBOL-3"],
        "the tail is omitted and reported, not fatal"
    );
}

#[test]
fn search_and_dependents_outrank_graph_stats_but_never_the_task() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        // Graph first in submission order; High-band facts must still rise.
        evidence: vec![
            fragment("WX-GRAPH", EvidenceKind::GraphStats, "stats"),
            fragment("WX-DEPENDENTS", EvidenceKind::Dependents, "callers"),
            fragment("WX-SEARCH", EvidenceKind::SearchHits, "MAX_RETRY_ATTEMPTS"),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let compiled = compile_evidence_bundle(bundle, "task", 1_000, None).unwrap();
    assert_eq!(
        compiled.context.included_ids,
        ["TASK", "WX-DEPENDENTS", "WX-SEARCH", "WX-GRAPH"],
        "dependents and search share High; graph stats stay Normal"
    );
    assert!(
        !compiled.context.requires_upstream,
        "search hits are verified evidence"
    );
}

#[test]
fn a_broad_packet_labels_only_mechanisms_already_present() {
    let task = "List every mechanism that can silently cause an archive miss.";
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-DEF",
                EvidenceKind::SourceReads,
                "pub enabled: bool,\n    pub max_entries: usize,\nfn safe_virtual_path() {}",
            ),
            fragment("WX-SEARCH", EvidenceKind::SearchHits, "search matches: 1"),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let compiled = compile_evidence_bundle(bundle, task, 2_000, None).unwrap();
    assert_eq!(compiled.context.included_ids[0], "TASK");
    assert_eq!(compiled.context.included_ids[1], "WX-MECHANISMS");
    assert!(compiled.context.content.contains("mechanism: enable-flag"));
    assert!(compiled.context.content.contains("mechanism: entry-count"));
    assert!(compiled.context.content.contains("mechanism: path-skip"));
    assert!(!compiled.context.content.contains("mechanism: feature-gate"));
}

#[test]
fn a_multiline_packet_labels_block_and_join() {
    let task = "How does multiline search group matches into a single reported block?";
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![fragment(
            "WX-DEF",
            EvidenceKind::SourceReads,
            "struct Block { end_line: usize }\nfn finish_block() {}",
        )],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let compiled = compile_evidence_bundle(bundle, task, 2_000, None).unwrap();
    assert!(
        compiled
            .context
            .content
            .contains("mechanism: block-type — struct `Block`")
    );
    assert!(
        compiled
            .context
            .content
            .contains("mechanism: join-condition")
    );
    assert!(compiled.context.content.contains("end_line"));
}

#[test]
fn pointed_tasks_do_not_grow_a_mechanism_index() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![fragment(
            "WX-DEF",
            EvidenceKind::SourceReads,
            "pub enabled: bool",
        )],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let compiled = compile_evidence_bundle(bundle, "Rename `read_limited`", 1_000, None).unwrap();
    assert!(
        !compiled
            .context
            .included_ids
            .iter()
            .any(|id| id == "WX-MECHANISMS")
    );
}

#[test]
fn compiles_typed_weavatrix_evidence_in_fail_closed_order() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment("WX-GRAPH", EvidenceKind::GraphStats, "graph"),
            fragment("WX-MODULES", EvidenceKind::ModuleMap, "modules"),
            fragment("WX-VERIFY", EvidenceKind::ChangePlan, "planned"),
            fragment("WX-SYMBOL", EvidenceKind::SymbolContext, "symbol"),
        ],
        warnings: vec!["refreshed".to_owned()],
        ..EvidenceBundle::default()
    };
    let compiled = compile_evidence_bundle(bundle, "change the symbol", 1_000, None).unwrap();
    assert_eq!(
        compiled.context.included_ids,
        ["TASK", "WX-SYMBOL", "WX-MODULES", "WX-VERIFY", "WX-GRAPH"]
    );
    assert!(compiled.context.requires_upstream);
    assert_eq!(compiled.evidence_count, 4);
    assert_eq!(compiled.warnings, ["refreshed"]);
    assert!(
        compiled
            .context
            .packet_id
            .as_deref()
            .is_some_and(|id| id.starts_with("pk_"))
    );
}

#[test]
fn source_windows_are_omitted_instead_of_blocking_compile() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![fragment(
            "WX-SOURCE",
            EvidenceKind::SourceReads,
            &"x".repeat(4_000),
        )],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let compiled = compile_evidence_bundle(bundle, "task", 80, None).unwrap();
    assert_eq!(compiled.context.included_ids, ["TASK"]);
    assert!(!compiled.context.omitted_ids.is_empty());
}

#[test]
fn symbol_context_cannot_disappear_under_a_small_budget() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![fragment(
            "WX-SYMBOL",
            EvidenceKind::SymbolContext,
            &"x".repeat(200),
        )],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    assert!(matches!(
        compile_evidence_bundle(bundle, "task", 80, None),
        Err(ContextError::CriticalItemExceedsBudget { id, .. }) if id == "WX-SYMBOL"
    ));
}

#[test]
fn relevance_scores_reorder_fragments_but_task_stays_first() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment("WX-VERIFY-1", EvidenceKind::ChangePlan, &"a".repeat(200)),
            fragment("WX-VERIFY-2", EvidenceKind::ChangePlan, &"b".repeat(200)),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let scores = HashMap::from([
        ("WX-VERIFY-1".to_owned(), 0.2),
        ("WX-VERIFY-2".to_owned(), 0.8),
    ]);
    // Budget fits TASK plus one plan part: the more relevant part wins.
    let compiled = compile_evidence_bundle(bundle, "task", 220, Some(&scores)).unwrap();
    assert_eq!(compiled.context.included_ids, ["TASK", "WX-VERIFY-2"]);
    assert_eq!(compiled.context.omitted_ids, ["WX-VERIFY-1"]);
    assert!(compiled.context.requires_upstream, "fail-closed untouched");
}

#[test]
fn a_certificate_becomes_a_decision_map_and_expand_handles() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![fragment(
            "ev_def",
            EvidenceKind::SourceReads,
            "pub struct ArchiveOptions { pub enabled: bool }",
        )],
        warnings: Vec::new(),
        snapshot_id: Some("git:abc+dirty:0".to_owned()),
    };
    let mut certificate = cortex_context::CoverageCertificate {
        required: vec![
            cortex_context::FACET_DEFINITION.to_owned(),
            cortex_context::FACET_CALLERS.to_owned(),
        ],
        missing: vec![cortex_context::FACET_CALLERS.to_owned()],
        sufficient: false,
        ..cortex_context::CoverageCertificate::default()
    };
    certificate.satisfied.insert(
        cortex_context::FACET_DEFINITION.to_owned(),
        vec!["ev_def".to_owned()],
    );
    let compiled = compile_evidence_bundle_layered(
        bundle,
        "Rename ArchiveOptions",
        2_000,
        None,
        Some(&certificate),
    )
    .unwrap();
    assert_eq!(compiled.context.included_ids[0], "TASK");
    assert_eq!(compiled.context.included_ids[1], "WX-MAP");
    assert!(compiled.context.content.contains("id=\"WX-MAP\""));
    assert!(
        compiled
            .context
            .content
            .contains("intent: identifier_change")
    );
    assert!(compiled.context.content.contains("EXPAND callers"));
    assert!(
        compiled
            .context
            .included_ids
            .contains(&"WX-EXPAND".to_owned())
    );
}

#[test]
fn compiled_packet_carries_the_bundle_snapshot() {
    let mut fragment = fragment("WX-SOURCE", EvidenceKind::SourceReads, "fn helper() {}");
    fragment.locator.snapshot_id = Some("git:deadbeef+dirty:0".to_owned());
    fragment.locator.path = Some("src/lib.rs".to_owned());
    fragment.locator.start_line = Some(1);
    fragment.locator.end_line = Some(1);
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![fragment],
        warnings: Vec::new(),
        snapshot_id: Some("git:deadbeef+dirty:0".to_owned()),
    };
    let compiled = compile_evidence_bundle(bundle, "task", 1_000, None).unwrap();
    assert_eq!(
        compiled.context.snapshot_id.as_deref(),
        Some("git:deadbeef+dirty:0")
    );
    assert!(cortex_context::snapshot_is_stale(
        compiled.context.snapshot_id.as_deref(),
        "git:deadbeef+dirty:ffff"
    ));
}
