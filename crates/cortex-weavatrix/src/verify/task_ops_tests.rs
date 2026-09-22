use super::*;
use crate::EvidenceFragment;

fn fragment(id: &str, kind: EvidenceKind, content: &str) -> EvidenceFragment {
    EvidenceFragment::new(id, kind, "test", content)
}

#[test]
fn a_named_file_does_not_require_a_symbol_definition() {
    let bundle = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                "search matches: 1\ncrates/sweeploom-cli/src/api.rs:1: pub mod routes",
            ),
            fragment(
                "WX-FILE",
                EvidenceKind::SourceReads,
                "crates/sweeploom-cli/src/api.rs\npub fn serve_http() {}\n",
            ),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let report = assess_compiled(
        &bundle,
        &["WX-SEARCH".to_owned(), "WX-FILE".to_owned()],
        "Split crates/sweeploom-cli/src/api.rs so api.rs is under 300 lines.",
        Some("crates/sweeploom-cli/src/api.rs"),
        PlanHints::default(),
        true,
        false,
    );
    assert!(
        report
            .required_evidence
            .iter()
            .all(|item| !item.starts_with("definition:")),
        "a path is not a definition target, required {:?}",
        report.required_evidence
    );
}

#[test]
fn dead_production_requires_unreferenced_symbols_not_catalog_quotes() {
    let empty = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![fragment(
            "WX-SEARCH",
            EvidenceKind::SearchHits,
            "search matches: 1\ncrates/sweeploom-cli/src/lib.rs:1: pub fn run",
        )],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let missing = assess_compiled(
        &empty,
        &["WX-SEARCH".to_owned()],
        "Find and fix one real bug or dead production path in crates/sweeploom-cli.",
        None,
        PlanHints::default(),
        false,
        false,
    );
    assert!(
        missing.required_evidence.contains(&"dead_code".to_owned()),
        "required {:?}",
        missing.required_evidence
    );

    let filled = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                "search matches: 1\ncrates/sweeploom-cli/src/cleanup.rs:40: fn leftover_path",
            ),
            fragment(
                "WX-DEAD",
                EvidenceKind::DeadCode,
                "dead_code: 1\n- leftover_path (function) crates/sweeploom-cli/src/cleanup.rs:40\n",
            ),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let report = assess_compiled(
        &filled,
        &["WX-SEARCH".to_owned(), "WX-DEAD".to_owned()],
        "Find and fix one real bug or dead production path in crates/sweeploom-cli.",
        None,
        PlanHints::default(),
        false,
        false,
    );
    assert!(
        !report
            .missing_evidence
            .iter()
            .any(|item| item == "dead_code")
    );
}

#[test]
fn duplicate_share_needs_two_live_classifier_sites() {
    let task = "Find, verify, and eliminate duplicate classifier logic in crates/sweeploom-ai.";
    let one = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                "search matches: 1\nsrc/a.rs:3: fn classify_legacy",
            ),
            fragment("WX-DUP", EvidenceKind::Duplicates, "duplicates: 0\n"),
            fragment(
                "WX-SOURCE",
                EvidenceKind::SourceReads,
                "fn classify_legacy() {}",
            ),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let thin = assess_compiled(
        &one,
        &[
            "WX-SEARCH".to_owned(),
            "WX-DUP".to_owned(),
            "WX-SOURCE".to_owned(),
        ],
        task,
        None,
        PlanHints::default(),
        true,
        false,
    );
    assert!(
        thin.missing_evidence
            .contains(&"classifier_pair".to_owned()),
        "one site is not a pair, missing {:?}",
        thin.missing_evidence
    );

    let pair = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                "search matches: 2\ncrates/sweeploom-ai/src/a.rs:3: fn classify_legacy\ncrates/sweeploom-ai/src/b.rs:3: fn classify_without",
            ),
            fragment(
                "WX-DUP",
                EvidenceKind::Duplicates,
                "duplicates: 1\n- type2 crates/sweeploom-ai/src/a.rs:3 | crates/sweeploom-ai/src/b.rs:3\n",
            ),
            fragment(
                "WX-SOURCE",
                EvidenceKind::SourceReads,
                "crates/sweeploom-ai\nfn classify_legacy() {}\nfn classify_without() {}\n",
            ),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let report = assess_compiled(
        &pair,
        &[
            "WX-SEARCH".to_owned(),
            "WX-DUP".to_owned(),
            "WX-SOURCE".to_owned(),
        ],
        task,
        None,
        PlanHints::default(),
        true,
        false,
    );
    assert!(
        !report
            .missing_evidence
            .iter()
            .any(|item| item == "classifier_pair"),
        "missing {:?}",
        report.missing_evidence
    );
}

#[test]
fn a_coverage_ask_requires_the_ingest_fragment() {
    let task = "What is the measured line coverage of crates/cortex-weavatrix?";
    let empty = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![fragment(
            "WX-SEARCH",
            EvidenceKind::SearchHits,
            "search matches: 1\ncrates/cortex-weavatrix/src/lib.rs:1: pub fn plan",
        )],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let missing = assess_compiled(
        &empty,
        &["WX-SEARCH".to_owned()],
        task,
        None,
        PlanHints::default(),
        false,
        false,
    );
    assert!(
        missing
            .required_evidence
            .contains(&"coverage_map".to_owned()),
        "required {:?}",
        missing.required_evidence
    );

    let filled = EvidenceBundle {
        repository: "repo".to_owned(),
        evidence: vec![
            fragment(
                "WX-SEARCH",
                EvidenceKind::SearchHits,
                "search matches: 1\ncrates/cortex-weavatrix/src/lib.rs:1: pub fn plan",
            ),
            fragment(
                "WX-COV",
                EvidenceKind::CoverageMap,
                "coverage: unmeasured\nreason: no supported report\n",
            ),
        ],
        warnings: Vec::new(),
        ..EvidenceBundle::default()
    };
    let report = assess_compiled(
        &filled,
        &["WX-SEARCH".to_owned(), "WX-COV".to_owned()],
        task,
        None,
        PlanHints::default(),
        false,
        false,
    );
    assert!(
        !report
            .missing_evidence
            .iter()
            .any(|item| item == "coverage_map")
    );
}
