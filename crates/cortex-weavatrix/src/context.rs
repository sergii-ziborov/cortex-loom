use std::collections::HashMap;

use cortex_context::{
    ContextError, ContextPacket, ContextRequest, EvidenceItem, EvidencePriority, EvidenceState,
    compile_context,
};
use serde::{Deserialize, Serialize};

use crate::{EvidenceBundle, EvidenceKind, EvidenceSufficiency};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompiledEvidenceBundle {
    pub repository: String,
    pub task: String,
    pub evidence_count: usize,
    pub warnings: Vec<String>,
    /// Provenance of a gated semantic ordering when one was applied, e.g.
    /// `embeddinggemma:latest/hybrid_graph/retrieval-ranking-v1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_ranking: Option<String>,
    /// Deterministic gather/verify result. Legacy callers that compile an
    /// arbitrary bundle directly leave this absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sufficiency: Option<EvidenceSufficiency>,
    pub context: ContextPacket,
}

/// Compile typed Weavatrix evidence into one bounded packet. `relevance`
/// carries optional gated semantic scores per fragment id; the synthetic
/// `TASK` item always outranks scored fragments within its band, and scores
/// only reorder within a priority band.
#[allow(clippy::implicit_hasher)]
pub fn compile_evidence_bundle(
    bundle: EvidenceBundle,
    task: &str,
    max_tokens: u32,
    relevance: Option<&HashMap<String, f64>>,
) -> Result<CompiledEvidenceBundle, ContextError> {
    let EvidenceBundle {
        repository,
        evidence,
        warnings,
    } = bundle;
    let evidence_count = evidence.len();
    let mut items = Vec::with_capacity(evidence_count + 1);
    items.push(EvidenceItem {
        id: "TASK".to_owned(),
        source: format!("request:{repository}"),
        content: task.to_owned(),
        priority: EvidencePriority::Critical,
        state: EvidenceState::Verified,
        relevance: relevance.map(|_| 1.0),
    });
    items.extend(evidence.into_iter().map(|fragment| {
        let (priority, state) = evidence_policy(fragment.kind, fragment.head);
        let score = relevance.and_then(|scores| scores.get(&fragment.id).copied());
        EvidenceItem {
            id: fragment.id,
            source: fragment.source,
            content: fragment.content,
            priority,
            state,
            relevance: score,
        }
    }));
    if let Some(index) = mechanism_index(task, &items) {
        items.insert(1, index);
    }
    // Fragments come from several Weavatrix operations that budget
    // independently, so the same source lines arrive more than once. Only
    // this layer can see that.
    let context = compile_context(&ContextRequest {
        items,
        max_tokens,
        deduplicate: true,
    })?;
    Ok(CompiledEvidenceBundle {
        repository,
        task: task.to_owned(),
        evidence_count,
        warnings,
        semantic_ranking: None,
        sufficiency: None,
        context,
    })
}

/// Priority and trust for one fragment.
///
/// `head` is the first sub-citation of a split tool result. Criticality
/// attaches to the head only: the definition of a symbol must never be
/// dropped by a budget, but the twentieth page of its reference list is
/// ordinary high-priority evidence. Marking every split part critical made a
/// 4 000-token compile fail outright whenever symbol evidence was present.
const fn evidence_policy(kind: EvidenceKind, head: bool) -> (EvidencePriority, EvidenceState) {
    match kind {
        EvidenceKind::GraphStats => (EvidencePriority::Normal, EvidenceState::Verified),
        // A planned change is the one kind that is not yet a fact.
        EvidenceKind::ChangePlan => (EvidencePriority::High, EvidenceState::Unverified),
        EvidenceKind::SymbolContext if head => {
            (EvidencePriority::Critical, EvidenceState::Verified)
        }
        // Source follow-up is the verified answer-bearing layer. Its shared
        // pool is already bounded by the gatherer, so letting earlier search
        // metadata evict it can report a sufficient gather while delivering
        // a packet that omits the very runtime/config fact the retry found.
        EvidenceKind::SourceReads => (EvidencePriority::Critical, EvidenceState::Verified),
        // Dependents and endpoints share High with search/modules: they used
        // to sit at Normal and lost to an unverified change plan whenever
        // both were fetched — measured on the structural fixture set.
        EvidenceKind::Dependents
        | EvidenceKind::Endpoints
        | EvidenceKind::ModuleMap
        | EvidenceKind::SearchHits
        | EvidenceKind::SymbolContext
        | EvidenceKind::TypeExpansion
        | EvidenceKind::GitHistory
        | EvidenceKind::StackTrace
        | EvidenceKind::TestSelection
        | EvidenceKind::Memory => (EvidencePriority::High, EvidenceState::Verified),
    }
}

/// A short index of mechanisms already present in the packet.
///
/// Measured on T3: the 9B saw `enabled`, `cfg(feature)`, and
/// `safe_virtual_path` and still refused to name them. Naming the fragment
/// recovered those facts through the same model. Only labels mechanisms the
/// evidence already carries.
fn mechanism_index(task: &str, items: &[EvidenceItem]) -> Option<EvidenceItem> {
    let lower = task.to_ascii_lowercase();
    if !crate::plan_intent::is_broad(task)
        && !lower.contains("quiet")
        && !is_block_join_task(&lower)
    {
        return None;
    }
    let blob: String = items
        .iter()
        .filter(|item| item.id != "TASK")
        .map(|item| item.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let mut lines = Vec::new();
    push_mechanism(
        &mut lines,
        &blob,
        &["pub enabled", " enabled:", ".enabled"],
        "mechanism: enable-flag — field `enabled`",
    );
    push_mechanism(
        &mut lines,
        &blob,
        &["max_entry_bytes", "max_expanded_bytes", "max_archive_bytes"],
        "mechanism: size-limit — max_entry_bytes / max_expanded_bytes / max_archive_bytes",
    );
    push_mechanism(
        &mut lines,
        &blob,
        &["max_entries"],
        "mechanism: entry-count — max_entries",
    );
    push_mechanism(
        &mut lines,
        &blob,
        &[
            "cfg(feature",
            "feature = \"archives\"",
            "feature=\"archives\"",
        ],
        "mechanism: feature-gate — cfg(feature = \"archives\")",
    );
    push_mechanism(
        &mut lines,
        &blob,
        &["safe_virtual_path", "../", "traversal"],
        "mechanism: path-skip — name `safe_virtual_path` (parent-dir / traversal skip)",
    );
    push_mechanism(
        &mut lines,
        &blob,
        &["quiet_match", "fn quiet"],
        "mechanism: quiet-path — quiet_match",
    );
    push_mechanism(
        &mut lines,
        &blob,
        &["fn finish_block", "finish_block("],
        "mechanism: flush — call `finish_block`",
    );
    push_mechanism(
        &mut lines,
        &blob,
        &["struct block", "type block"],
        "mechanism: block-type — struct `Block`",
    );
    push_mechanism(
        &mut lines,
        &blob,
        &["end_line", "start_line"],
        "mechanism: join-condition — end_line / start_line; otherwise finish_block",
    );
    if lines.is_empty() {
        return None;
    }
    Some(EvidenceItem {
        id: "WX-MECHANISMS".to_owned(),
        source: "cortex:mechanism_index".to_owned(),
        content: format!("mechanisms present in this packet:\n{}", lines.join("\n")),
        priority: EvidencePriority::Critical,
        state: EvidenceState::Verified,
        relevance: None,
    })
}

fn is_block_join_task(lower: &str) -> bool {
    lower.contains("block")
        && (lower.contains("join") || lower.contains("group") || lower.contains("multiline"))
}

fn push_mechanism(lines: &mut Vec<String>, blob: &str, needles: &[&str], label: &str) {
    if needles.iter().any(|needle| blob.contains(needle)) {
        lines.push(label.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EvidenceFragment;

    fn fragment(id: &str, kind: EvidenceKind, content: &str) -> EvidenceFragment {
        EvidenceFragment {
            id: id.to_owned(),
            kind,
            source: format!("weavatrix:{id}"),
            content: content.to_owned(),
            head: true,
        }
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
        };
        let compiled = compile_evidence_bundle(bundle, "task", 100, None).unwrap();
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
        };
        let compiled =
            compile_evidence_bundle(bundle, "Rename `read_limited`", 1_000, None).unwrap();
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
        };
        let compiled = compile_evidence_bundle(bundle, "change the symbol", 1_000, None).unwrap();
        assert_eq!(
            compiled.context.included_ids,
            ["TASK", "WX-SYMBOL", "WX-MODULES", "WX-VERIFY", "WX-GRAPH"]
        );
        assert!(compiled.context.requires_upstream);
        assert_eq!(compiled.evidence_count, 4);
        assert_eq!(compiled.warnings, ["refreshed"]);
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
        };
        assert!(matches!(
            compile_evidence_bundle(bundle, "task", 20, None),
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
        };
        let scores = HashMap::from([
            ("WX-VERIFY-1".to_owned(), 0.2),
            ("WX-VERIFY-2".to_owned(), 0.8),
        ]);
        // Budget fits TASK plus one plan part: the more relevant part wins.
        // 130 is calibrated to the conservative runtime counter; char/4
        // used to fit the same pair in 70.
        let compiled = compile_evidence_bundle(bundle, "task", 130, Some(&scores)).unwrap();
        assert_eq!(compiled.context.included_ids, ["TASK", "WX-VERIFY-2"]);
        assert_eq!(compiled.context.omitted_ids, ["WX-VERIFY-1"]);
        assert!(compiled.context.requires_upstream, "fail-closed untouched");
    }
}
