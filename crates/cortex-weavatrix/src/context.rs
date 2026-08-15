use std::collections::HashMap;

use cortex_context::{
    ContextError, ContextPacket, ContextRequest, EvidenceDerivation, EvidenceFacet, EvidenceItem,
    EvidencePriority, EvidenceState, compile_context,
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
    /// `qwen3-embed/hybrid_graph/retrieval-ranking-v2/evidence_spans`.
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
    compile_layered(bundle, task, max_tokens, relevance, None, false)
}

/// Same compile, with an L0 decision map and L2 expansion handles when a
/// coverage certificate is supplied.
#[allow(clippy::implicit_hasher)]
pub fn compile_evidence_bundle_layered(
    bundle: EvidenceBundle,
    task: &str,
    max_tokens: u32,
    relevance: Option<&HashMap<String, f64>>,
    certificate: Option<&cortex_context::CoverageCertificate>,
) -> Result<CompiledEvidenceBundle, ContextError> {
    compile_layered(bundle, task, max_tokens, relevance, certificate, false)
}

/// Bench/probe compile. Applies the T2/T3 mechanism labels that must not
/// run on the generic engine.
#[allow(clippy::implicit_hasher)]
pub fn compile_probe_bundle(
    bundle: EvidenceBundle,
    task: &str,
    max_tokens: u32,
    relevance: Option<&HashMap<String, f64>>,
) -> Result<CompiledEvidenceBundle, ContextError> {
    compile_layered(bundle, task, max_tokens, relevance, None, true)
}

#[allow(clippy::implicit_hasher)]
fn compile_layered(
    bundle: EvidenceBundle,
    task: &str,
    max_tokens: u32,
    relevance: Option<&HashMap<String, f64>>,
    certificate: Option<&cortex_context::CoverageCertificate>,
    probe_mechanisms: bool,
) -> Result<CompiledEvidenceBundle, ContextError> {
    let EvidenceBundle {
        repository,
        evidence,
        warnings,
        snapshot_id,
    } = bundle;
    let evidence_count = evidence.len();
    let mut items = Vec::with_capacity(evidence_count + 1);
    items.push({
        let mut task_item = EvidenceItem::new(
            "TASK",
            format!("request:{repository}"),
            task,
            EvidencePriority::Critical,
            EvidenceState::Verified,
        );
        task_item.relevance = relevance.map(|_| 1.0);
        if let Some(locator) = task_item.locator.as_mut() {
            locator.snapshot_id.clone_from(&snapshot_id);
        }
        task_item
    });
    items.extend(evidence.into_iter().map(|fragment| {
        let (priority, state) = evidence_policy(fragment.kind, fragment.head, fragment.facet);
        let score = relevance.and_then(|scores| scores.get(&fragment.id).copied());
        let mut item = EvidenceItem::new(
            fragment.id,
            fragment.source,
            fragment.content,
            priority,
            state,
        );
        item.relevance = score;
        item.derivation = Some(derivation_for(fragment.kind, state));
        item.facet = Some(fragment.facet);
        item.group_id = fragment.group_id;
        let mut locator = fragment.locator;
        if locator.snapshot_id.is_none() {
            locator.snapshot_id.clone_from(&snapshot_id);
        }
        item.locator = (!locator.is_empty()).then_some(locator);
        item
    }));
    if let Some(certificate) = certificate {
        items.insert(
            1,
            crate::layers::decision_map_item(
                &repository,
                task,
                snapshot_id.as_deref(),
                certificate,
            ),
        );
        if let Some(expands) = crate::layers::expansion_item(certificate) {
            items.push(expands);
        }
    }
    if probe_mechanisms {
        crate::mechanisms::insert_probe_index(task, &mut items);
    }
    let snapshot_for_packet = snapshot_id.clone();
    // Fragments come from several Weavatrix operations that budget
    // independently, so the same source lines arrive more than once. Only
    // this layer can see that.
    let mut context = compile_context(&ContextRequest {
        items,
        max_tokens,
        deduplicate: true,
    })?;
    if context.snapshot_id.is_none() {
        context.snapshot_id = snapshot_for_packet;
    }
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
const fn evidence_policy(
    kind: EvidenceKind,
    head: bool,
    facet: EvidenceFacet,
) -> (EvidencePriority, EvidenceState) {
    match (kind, facet, head) {
        (EvidenceKind::ChangePlan, _, _) => (EvidencePriority::High, EvidenceState::Unverified),
        (_, EvidenceFacet::Definition | EvidenceFacet::CallerSignature, true)
        | (EvidenceKind::SymbolContext, _, true) => {
            (EvidencePriority::Critical, EvidenceState::Verified)
        }
        (_, EvidenceFacet::SourceWindow, _) | (EvidenceKind::SourceReads, _, false) => {
            (EvidencePriority::Normal, EvidenceState::Verified)
        }
        (_, EvidenceFacet::References, _) | (EvidenceKind::SourceReads, _, true) => {
            (EvidencePriority::High, EvidenceState::Verified)
        }
        (EvidenceKind::GraphStats, _, _) => (EvidencePriority::Low, EvidenceState::Verified),
        (
            EvidenceKind::Dependents
            | EvidenceKind::Endpoints
            | EvidenceKind::ModuleMap
            | EvidenceKind::SearchHits
            | EvidenceKind::SymbolContext
            | EvidenceKind::TypeExpansion
            | EvidenceKind::GitHistory
            | EvidenceKind::StackTrace
            | EvidenceKind::TestSelection
            | EvidenceKind::Memory,
            _,
            _,
        ) => (EvidencePriority::High, EvidenceState::Verified),
    }
}

const fn derivation_for(kind: EvidenceKind, state: EvidenceState) -> EvidenceDerivation {
    match (kind, state) {
        (EvidenceKind::ChangePlan, _) => EvidenceDerivation::Plan,
        (EvidenceKind::Memory, _) => EvidenceDerivation::Memory,
        (EvidenceKind::SearchHits, _) => EvidenceDerivation::Search,
        (
            EvidenceKind::GraphStats
            | EvidenceKind::ModuleMap
            | EvidenceKind::Dependents
            | EvidenceKind::Endpoints
            | EvidenceKind::GitHistory
            | EvidenceKind::StackTrace
            | EvidenceKind::TestSelection,
            _,
        ) => EvidenceDerivation::Graph,
        (_, EvidenceState::Unverified) => EvidenceDerivation::Inferred,
        _ => EvidenceDerivation::ExactSource,
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
