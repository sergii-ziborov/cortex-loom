use cortex_domain::{GraphDocument, NodeKind};

use crate::evidence::validate_evidence_references;
use crate::transition::complete_node;
use crate::{
    HumanDecision, HumanDecisionRecord, MAX_RUN_DETAIL_BYTES, NodeOutcome, RunDocument, RunError,
};

pub(super) struct HumanDecisionInput<'a> {
    pub node_id: &'a str,
    pub decision: HumanDecision,
    pub actor: &'a str,
    pub reason: &'a str,
    pub selected_edge_ids: &'a [String],
    pub evidence_ids: &'a [String],
}

pub(super) fn decide_human_gate(
    run: &mut RunDocument,
    graph: &GraphDocument,
    input: &HumanDecisionInput<'_>,
    now: i64,
) -> Result<Vec<String>, RunError> {
    let node = graph
        .nodes
        .iter()
        .find(|node| node.id == input.node_id)
        .ok_or_else(|| RunError::NodeNotFound(input.node_id.to_owned()))?;
    if !matches!(node.kind, NodeKind::HumanGate | NodeKind::ReviewGate) {
        return Err(RunError::InvalidHumanGate(format!(
            "{} is {:?}, not a human or review gate",
            input.node_id, node.kind
        )));
    }
    validate_actor_reason(input.actor, input.reason)?;
    validate_evidence_references(run, input.node_id, input.evidence_ids)?;
    if input.decision == HumanDecision::Approved
        && node
            .execution
            .as_ref()
            .is_some_and(|policy| policy.require_evidence)
        && input.evidence_ids.is_empty()
    {
        return Err(RunError::EvidenceRequired(input.node_id.to_owned()));
    }
    let outcome = match input.decision {
        HumanDecision::Approved => NodeOutcome::Succeeded,
        HumanDecision::Rejected => NodeOutcome::Failed,
    };
    let traversed = complete_node(
        run,
        graph,
        input.node_id,
        outcome,
        input.selected_edge_ids,
        input.evidence_ids,
        Some(input.reason.to_owned()),
    )?;
    let state = run
        .nodes
        .iter_mut()
        .find(|state| state.node_id == input.node_id)
        .ok_or_else(|| RunError::NodeNotFound(input.node_id.to_owned()))?;
    state.human_decision = Some(HumanDecisionRecord {
        decision: input.decision,
        actor: input.actor.to_owned(),
        reason: input.reason.to_owned(),
        evidence_ids: input.evidence_ids.to_vec(),
        decided_at: now,
    });
    Ok(traversed)
}

fn validate_actor_reason(actor: &str, reason: &str) -> Result<(), RunError> {
    if actor.trim().is_empty() {
        return Err(RunError::InvalidHumanGate(
            "actor must not be empty".to_owned(),
        ));
    }
    if reason.trim().is_empty() {
        return Err(RunError::InvalidHumanGate(
            "reason must not be empty".to_owned(),
        ));
    }
    for (field, value) in [("actor", actor), ("reason", reason)] {
        if value.len() > MAX_RUN_DETAIL_BYTES {
            return Err(RunError::InvalidHumanGate(format!(
                "{field} exceeds {MAX_RUN_DETAIL_BYTES} bytes"
            )));
        }
    }
    Ok(())
}
