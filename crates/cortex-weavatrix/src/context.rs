use cortex_context::{
    ContextError, ContextPacket, ContextRequest, EvidenceItem, EvidencePriority, EvidenceState,
    compile_context,
};
use serde::{Deserialize, Serialize};

use crate::{EvidenceBundle, EvidenceKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompiledEvidenceBundle {
    pub repository: String,
    pub task: String,
    pub evidence_count: usize,
    pub warnings: Vec<String>,
    pub context: ContextPacket,
}

pub fn compile_evidence_bundle(
    bundle: EvidenceBundle,
    task: &str,
    max_tokens: u32,
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
    });
    items.extend(evidence.into_iter().map(|fragment| {
        let (priority, state) = evidence_policy(fragment.kind);
        EvidenceItem {
            id: fragment.id,
            source: fragment.source,
            content: fragment.content,
            priority,
            state,
        }
    }));
    let context = compile_context(&ContextRequest { items, max_tokens })?;
    Ok(CompiledEvidenceBundle {
        repository,
        task: task.to_owned(),
        evidence_count,
        warnings,
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
        let compiled = compile_evidence_bundle(bundle, "change the symbol", 1_000).unwrap();
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
            compile_evidence_bundle(bundle, "task", 20),
            Err(ContextError::CriticalItemExceedsBudget { id, .. }) if id == "WX-SYMBOL"
        ));
    }
}
