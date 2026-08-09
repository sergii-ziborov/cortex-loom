use std::collections::HashSet;

use cortex_domain::{EdgeKind, GraphDocument, NodeKind};

use crate::evidence::{
    EvidenceInput, invalidate_evidence, submit_evidence, validate_evidence_references,
};
use crate::flow::{ensure_acyclic_flow, incoming_flow, is_flow_edge};
use crate::human::{HumanDecisionInput, decide_human_gate};
use crate::lease::{claim_lease, clear_lease, enforce_lease, release_lease};
use crate::retry::{trigger_retry, validate_retry_nodes};
use crate::transition::{cancel_run, complete_node};
use crate::{
    EdgeRunState, EdgeRunStatus, ExecutorIdentity, HumanDecision, MAX_RUN_DETAIL_BYTES,
    MAX_RUN_ID_BYTES, NodeOutcome, NodeRunState, NodeRunStatus, RUN_SCHEMA_VERSION, RunCommand,
    RunDocument, RunError, RunEvent, RunEventKind, RunStatus,
};

struct Applied {
    kind: RunEventKind,
    node_id: Option<String>,
    edge_ids: Vec<String>,
    evidence_ids: Vec<String>,
    detail: Option<String>,
}

pub fn create_run(
    graph: &GraphDocument,
    id: impl Into<String>,
    now: i64,
) -> Result<(RunDocument, RunEvent), RunError> {
    graph.validate()?;
    let id = id.into();
    validate_new_run(graph, &id)?;
    let incoming = incoming_flow(graph);
    let nodes = graph
        .nodes
        .iter()
        .map(|node| NodeRunState {
            node_id: node.id.clone(),
            status: if incoming.get(node.id.as_str()).is_none_or(Vec::is_empty) {
                NodeRunStatus::Ready
            } else {
                NodeRunStatus::Pending
            },
            attempt: 0,
            activated_by: Vec::new(),
            evidence_ids: Vec::new(),
            detail: None,
            human_decision: None,
            lease: None,
        })
        .collect();
    let edges = graph
        .edges
        .iter()
        .map(|edge| EdgeRunState {
            edge_id: edge.id.clone(),
            status: if is_flow_edge(edge.kind) {
                EdgeRunStatus::Pending
            } else {
                EdgeRunStatus::Dormant
            },
        })
        .collect();
    let run = RunDocument {
        schema_version: RUN_SCHEMA_VERSION.to_owned(),
        id,
        graph_id: graph.id.clone(),
        graph_revision: graph.revision,
        revision: 1,
        status: RunStatus::Running,
        nodes,
        edges,
        evidence: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    let event = event(&run, RunEventKind::Created, None, None, now);
    Ok((run, event))
}

pub fn apply_command(
    run: &mut RunDocument,
    graph: &GraphDocument,
    command: &RunCommand,
    now: i64,
) -> Result<RunEvent, RunError> {
    validate_command_context(run, graph, command)?;
    let applied = dispatch_command(run, graph, command, now)?;
    run.revision = run.revision.saturating_add(1);
    run.updated_at = now;
    Ok(event(
        run,
        applied.kind,
        Some(command.clone()),
        Some(applied),
        now,
    ))
}

// A pure delegation match: every arm forwards to one apply_* helper.
#[allow(clippy::too_many_lines)]
fn dispatch_command(
    run: &mut RunDocument,
    graph: &GraphDocument,
    command: &RunCommand,
    now: i64,
) -> Result<Applied, RunError> {
    match command {
        RunCommand::StartNode {
            node_id, executor, ..
        } => {
            enforce_lease(run, node_id, executor.as_ref(), now)?;
            start_node(run, graph, node_id)?;
            Ok(Applied::node(RunEventKind::NodeStarted, node_id))
        }
        RunCommand::SubmitEvidence {
            node_id,
            evidence_id,
            submitted_by,
            source,
            locator,
            digest,
            summary,
            executor,
            ..
        } => apply_evidence(
            run,
            graph,
            &EvidenceInput {
                node_id,
                evidence_id,
                submitted_by,
                source,
                locator,
                digest: digest.as_ref(),
                summary,
            },
            executor.as_ref(),
            now,
        ),
        RunCommand::CompleteNode {
            node_id,
            outcome,
            selected_edge_ids,
            evidence_ids,
            detail,
            executor,
            ..
        } => apply_completion(
            run,
            graph,
            node_id,
            *outcome,
            selected_edge_ids,
            evidence_ids,
            detail.as_ref(),
            executor.as_ref(),
            now,
        ),
        RunCommand::DecideHumanGate {
            node_id,
            decision,
            actor,
            reason,
            selected_edge_ids,
            evidence_ids,
            executor,
            ..
        } => apply_human_decision(
            run,
            graph,
            node_id,
            *decision,
            actor,
            reason,
            selected_edge_ids,
            evidence_ids,
            executor.as_ref(),
            now,
        ),
        RunCommand::ClaimLease {
            node_id,
            executor,
            ttl_seconds,
            ..
        } => apply_lease_claim(run, graph, node_id, executor, *ttl_seconds, now),
        RunCommand::ReleaseLease {
            node_id, executor, ..
        } => {
            release_lease(run, node_id, executor)?;
            Ok(Applied::node(RunEventKind::LeaseReleased, node_id))
        }
        RunCommand::InvalidateEvidence {
            evidence_id,
            actor,
            reason,
            ..
        } => {
            invalidate_evidence(run, evidence_id, actor, reason, now)?;
            Ok(Applied {
                kind: RunEventKind::EvidenceInvalidated,
                node_id: None,
                edge_ids: Vec::new(),
                evidence_ids: vec![evidence_id.clone()],
                detail: Some(reason.clone()),
            })
        }
        RunCommand::TriggerRetry {
            retry_node_id,
            reason,
            ..
        } => apply_retry(run, graph, retry_node_id, reason),
        RunCommand::Cancel { reason, .. } => apply_cancel(run, reason),
    }
}

fn apply_lease_claim(
    run: &mut RunDocument,
    graph: &GraphDocument,
    node_id: &str,
    executor: &ExecutorIdentity,
    ttl_seconds: u32,
    now: i64,
) -> Result<Applied, RunError> {
    claim_lease(run, graph, node_id, executor, ttl_seconds, now)?;
    Ok(Applied {
        kind: RunEventKind::LeaseClaimed,
        node_id: Some(node_id.to_owned()),
        edge_ids: Vec::new(),
        evidence_ids: Vec::new(),
        detail: Some(format!("{:?}:{}", executor.kind, executor.id)),
    })
}

fn apply_evidence(
    run: &mut RunDocument,
    graph: &GraphDocument,
    input: &EvidenceInput<'_>,
    executor: Option<&ExecutorIdentity>,
    now: i64,
) -> Result<Applied, RunError> {
    enforce_lease(run, input.node_id, executor, now)?;
    submit_evidence(run, graph, input, now)?;
    Ok(Applied {
        kind: RunEventKind::EvidenceSubmitted,
        node_id: Some(input.node_id.to_owned()),
        edge_ids: Vec::new(),
        evidence_ids: vec![input.evidence_id.to_owned()],
        detail: Some(input.summary.to_owned()),
    })
}

fn apply_retry(
    run: &mut RunDocument,
    graph: &GraphDocument,
    retry_node_id: &str,
    reason: &str,
) -> Result<Applied, RunError> {
    trigger_retry(run, graph, retry_node_id, reason)?;
    Ok(Applied {
        kind: RunEventKind::RetryTriggered,
        node_id: Some(retry_node_id.to_owned()),
        edge_ids: Vec::new(),
        evidence_ids: Vec::new(),
        detail: Some(reason.to_owned()),
    })
}

fn apply_cancel(run: &mut RunDocument, reason: &str) -> Result<Applied, RunError> {
    validate_detail(reason, true)?;
    cancel_run(run);
    Ok(Applied {
        kind: RunEventKind::Cancelled,
        node_id: None,
        edge_ids: Vec::new(),
        evidence_ids: Vec::new(),
        detail: Some(reason.to_owned()),
    })
}

fn validate_new_run(graph: &GraphDocument, id: &str) -> Result<(), RunError> {
    if id.trim().is_empty() {
        return Err(RunError::EmptyRunId);
    }
    if id.len() > MAX_RUN_ID_BYTES {
        return Err(RunError::RunIdTooLarge(id.len()));
    }
    if graph.nodes.is_empty() {
        return Err(RunError::EmptyGraph);
    }
    ensure_acyclic_flow(graph)?;
    validate_retry_nodes(graph)
}

fn validate_command_context(
    run: &RunDocument,
    graph: &GraphDocument,
    command: &RunCommand,
) -> Result<(), RunError> {
    if run.graph_id != graph.id || run.graph_revision != graph.revision {
        return Err(RunError::GraphMismatch);
    }
    if command.expected_revision() != run.revision {
        return Err(RunError::RevisionConflict {
            expected: command.expected_revision(),
            current: run.revision,
        });
    }
    if run.status != RunStatus::Running {
        return Err(RunError::RunFinished(run.status));
    }
    Ok(())
}

fn start_node(run: &mut RunDocument, graph: &GraphDocument, node_id: &str) -> Result<(), RunError> {
    let definition = graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| RunError::NodeNotFound(node_id.to_owned()))?;
    if definition.kind == NodeKind::Retry {
        return Err(RunError::RetryCommandRequired(node_id.to_owned()));
    }
    let node = run
        .nodes
        .iter_mut()
        .find(|node| node.node_id == node_id)
        .ok_or_else(|| RunError::NodeNotFound(node_id.to_owned()))?;
    if node.status != NodeRunStatus::Ready {
        return Err(RunError::InvalidNodeState {
            node: node_id.to_owned(),
            expected: NodeRunStatus::Ready,
            current: node.status,
        });
    }
    node.status = NodeRunStatus::Running;
    node.attempt = node.attempt.saturating_add(1);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_completion(
    run: &mut RunDocument,
    graph: &GraphDocument,
    node_id: &str,
    outcome: NodeOutcome,
    selected: &[String],
    evidence_ids: &[String],
    detail: Option<&String>,
    executor: Option<&ExecutorIdentity>,
    now: i64,
) -> Result<Applied, RunError> {
    enforce_lease(run, node_id, executor, now)?;
    let node = graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| RunError::NodeNotFound(node_id.to_owned()))?;
    if matches!(node.kind, NodeKind::HumanGate | NodeKind::ReviewGate) {
        return Err(RunError::HumanDecisionRequired(node_id.to_owned()));
    }
    if node.kind == NodeKind::Retry {
        return Err(RunError::RetryCommandRequired(node_id.to_owned()));
    }
    validate_evidence_references(run, node_id, evidence_ids)?;
    validate_detail_option(detail)?;
    if outcome == NodeOutcome::Succeeded
        && (node.kind == NodeKind::EvidenceGate
            || node
                .execution
                .as_ref()
                .is_some_and(|policy| policy.require_evidence))
        && evidence_ids.is_empty()
    {
        return Err(RunError::EvidenceRequired(node_id.to_owned()));
    }
    validate_selected_edges(graph, node_id, node.kind, selected)?;
    let traversed = complete_node(
        run,
        graph,
        node_id,
        outcome,
        selected,
        evidence_ids,
        detail.cloned(),
    )?;
    clear_lease(run, node_id);
    Ok(Applied {
        kind: match outcome {
            NodeOutcome::Succeeded => RunEventKind::NodeSucceeded,
            NodeOutcome::Failed => RunEventKind::NodeFailed,
        },
        node_id: Some(node_id.to_owned()),
        edge_ids: traversed,
        evidence_ids: evidence_ids.to_vec(),
        detail: detail.cloned(),
    })
}

#[allow(clippy::too_many_arguments)]
fn apply_human_decision(
    run: &mut RunDocument,
    graph: &GraphDocument,
    node_id: &str,
    decision: HumanDecision,
    actor: &str,
    reason: &str,
    selected: &[String],
    evidence_ids: &[String],
    executor: Option<&ExecutorIdentity>,
    now: i64,
) -> Result<Applied, RunError> {
    enforce_lease(run, node_id, executor, now)?;
    let node = graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| RunError::NodeNotFound(node_id.to_owned()))?;
    validate_selected_edges(graph, node_id, node.kind, selected)?;
    let traversed = decide_human_gate(
        run,
        graph,
        &HumanDecisionInput {
            node_id,
            decision,
            actor,
            reason,
            selected_edge_ids: selected,
            evidence_ids,
        },
        now,
    )?;
    clear_lease(run, node_id);
    Ok(Applied {
        kind: match decision {
            HumanDecision::Approved => RunEventKind::HumanApproved,
            HumanDecision::Rejected => RunEventKind::HumanRejected,
        },
        node_id: Some(node_id.to_owned()),
        edge_ids: traversed,
        evidence_ids: evidence_ids.to_vec(),
        detail: Some(reason.to_owned()),
    })
}

fn validate_selected_edges(
    graph: &GraphDocument,
    node_id: &str,
    node_kind: NodeKind,
    selected: &[String],
) -> Result<(), RunError> {
    let unique = selected.iter().collect::<HashSet<_>>();
    if unique.len() != selected.len() {
        return Err(RunError::InvalidConditionalSelection(
            "edge ids must be unique".to_owned(),
        ));
    }
    let conditional = graph
        .edges
        .iter()
        .filter(|edge| edge.from == node_id && edge.kind == EdgeKind::Conditional)
        .collect::<Vec<_>>();
    for id in selected {
        if !conditional.iter().any(|edge| edge.id == *id) {
            return Err(RunError::InvalidConditionalSelection(format!(
                "{id} is not an outgoing conditional edge of {node_id}"
            )));
        }
    }
    if node_kind == NodeKind::Branch && !conditional.is_empty() && selected.len() != 1 {
        return Err(RunError::InvalidConditionalSelection(
            "a branch with conditional edges must select exactly one".to_owned(),
        ));
    }
    Ok(())
}

fn validate_detail_option(detail: Option<&String>) -> Result<(), RunError> {
    if let Some(detail) = detail {
        validate_detail(detail, false)?;
    }
    Ok(())
}

fn validate_detail(detail: &str, required: bool) -> Result<(), RunError> {
    if required && detail.trim().is_empty() {
        return Err(RunError::EmptyCancellationReason);
    }
    if detail.len() > MAX_RUN_DETAIL_BYTES {
        return Err(RunError::DetailTooLarge(detail.len()));
    }
    Ok(())
}

fn event(
    run: &RunDocument,
    kind: RunEventKind,
    command: Option<RunCommand>,
    applied: Option<Applied>,
    now: i64,
) -> RunEvent {
    let applied = applied.unwrap_or_else(|| Applied {
        kind,
        node_id: None,
        edge_ids: Vec::new(),
        evidence_ids: Vec::new(),
        detail: None,
    });
    RunEvent {
        run_id: run.id.clone(),
        graph_id: run.graph_id.clone(),
        graph_revision: run.graph_revision,
        sequence: run.revision,
        kind,
        command,
        node_id: applied.node_id,
        edge_ids: applied.edge_ids,
        evidence_ids: applied.evidence_ids,
        detail: applied.detail,
        run_status: run.status,
        recorded_at: now,
    }
}

impl Applied {
    fn node(kind: RunEventKind, node_id: &str) -> Self {
        Self {
            kind,
            node_id: Some(node_id.to_owned()),
            edge_ids: Vec::new(),
            evidence_ids: Vec::new(),
            detail: None,
        }
    }
}
