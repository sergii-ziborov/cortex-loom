use std::collections::HashMap;

use cortex_context::{
    ContextError, ContextPacket, ContextRequest, EvidenceItem, EvidencePriority, EvidenceState,
    compile_context,
};
use serde::{Deserialize, Serialize};

use crate::{EvidenceBundle, EvidenceKind};

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
        let (priority, state) = evidence_policy(fragment.kind);
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
    let context = compile_context(&ContextRequest { items, max_tokens })?;
    Ok(CompiledEvidenceBundle {
        repository,
        task: task.to_owned(),
        evidence_count,
        warnings,
        semantic_ranking: None,
        context,
    })
}

const fn evidence_policy(kind: EvidenceKind) -> (EvidencePriority, EvidenceState) {
    match kind {
        EvidenceKind::GraphStats => (EvidencePriority::Normal, EvidenceState::Verified),
        EvidenceKind::ModuleMap => (EvidencePriority::High, EvidenceState::Verified),
        EvidenceKind::ChangePlan => (EvidencePriority::High, EvidenceState::Unverified),
        EvidenceKind::SymbolContext => (EvidencePriority::Critical, EvidenceState::Verified),
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
        }
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
        let compiled = compile_evidence_bundle(bundle, "task", 70, Some(&scores)).unwrap();
        assert_eq!(compiled.context.included_ids, ["TASK", "WX-VERIFY-2"]);
        assert_eq!(compiled.context.omitted_ids, ["WX-VERIFY-1"]);
        assert!(compiled.context.requires_upstream, "fail-closed untouched");
    }
}
