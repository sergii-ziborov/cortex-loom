use std::collections::{HashSet, VecDeque};

use cortex_domain::{GraphDocument, NodeKind};

use crate::flow::{edge_matches, is_flow_edge};
use crate::{
    EdgeRunStatus, MAX_RETRY_ATTEMPTS, MAX_RUN_DETAIL_BYTES, NodeOutcome, NodeRunStatus,
    RunDocument, RunError, RunStatus,
};

pub(super) fn validate_retry_nodes(graph: &GraphDocument) -> Result<(), RunError> {
    for node in graph
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Retry)
    {
        let (target, max_attempts) = retry_config(node)?;
        if !graph.nodes.iter().any(|candidate| candidate.id == target) {
            return Err(RunError::InvalidRetry(format!(
                "{} targets missing node {target}",
                node.id
            )));
        }
        if !(2..=MAX_RETRY_ATTEMPTS).contains(&max_attempts) {
            return Err(RunError::RetryLimitTooLarge(max_attempts));
        }
        let incoming = graph
            .edges
            .iter()
            .filter(|edge| edge.to == node.id && is_flow_edge(edge.kind))
            .collect::<Vec<_>>();
        if incoming.is_empty() {
            return Err(RunError::InvalidRetry(format!(
                "{} requires a failure transition from {target}",
                node.id
            )));
        }
        if incoming
            .iter()
            .any(|edge| edge.from != target || !edge_matches(edge.kind, NodeOutcome::Failed))
        {
            return Err(RunError::InvalidRetry(format!(
                "{} accepts only failure transitions from {target}",
                node.id
            )));
        }
    }
    Ok(())
}

pub(super) fn trigger_retry(
    run: &mut RunDocument,
    graph: &GraphDocument,
    retry_node_id: &str,
    reason: &str,
) -> Result<String, RunError> {
    validate_reason(reason)?;
    let retry_node = graph
        .nodes
        .iter()
        .find(|node| node.id == retry_node_id)
        .ok_or_else(|| RunError::NodeNotFound(retry_node_id.to_owned()))?;
    if retry_node.kind != NodeKind::Retry {
        return Err(RunError::InvalidRetry(format!(
            "{retry_node_id} is not a retry node"
        )));
    }
    let (target_id, max_attempts) = retry_config(retry_node)?;
    validate_retry_state(run, graph, retry_node_id, target_id, max_attempts)?;
    rewind(run, graph, target_id)?;
    let retry_state = run
        .nodes
        .iter_mut()
        .find(|state| state.node_id == retry_node_id)
        .ok_or_else(|| RunError::NodeNotFound(retry_node_id.to_owned()))?;
    retry_state.attempt = retry_state.attempt.saturating_add(1);
    run.status = RunStatus::Running;
    Ok(target_id.to_owned())
}

fn retry_config(node: &cortex_domain::GraphNode) -> Result<(&str, u32), RunError> {
    let target = node
        .config
        .get("targetNodeId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            RunError::InvalidRetry(format!("{} requires config.targetNodeId", node.id))
        })?;
    let max = node
        .config
        .get("maxAttempts")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            RunError::InvalidRetry(format!("{} requires config.maxAttempts", node.id))
        })?;
    Ok((target, max))
}

fn validate_retry_state(
    run: &RunDocument,
    graph: &GraphDocument,
    retry_id: &str,
    target_id: &str,
    max_attempts: u32,
) -> Result<(), RunError> {
    let retry = state(run, retry_id)?;
    if retry.status != NodeRunStatus::Ready {
        return Err(RunError::InvalidNodeState {
            node: retry_id.to_owned(),
            expected: NodeRunStatus::Ready,
            current: retry.status,
        });
    }
    let target = state(run, target_id)?;
    if target.status != NodeRunStatus::Failed {
        return Err(RunError::InvalidRetry(format!(
            "target {target_id} must be failed"
        )));
    }
    if target.attempt >= max_attempts {
        return Err(RunError::RetryLimitReached {
            node: target_id.to_owned(),
            limit: max_attempts,
        });
    }
    let linked = graph.edges.iter().any(|edge| {
        edge.from == target_id
            && edge.to == retry_id
            && is_flow_edge(edge.kind)
            && edge_matches(edge.kind, NodeOutcome::Failed)
    });
    if !linked {
        return Err(RunError::InvalidRetry(format!(
            "{retry_id} must have a failure transition from {target_id}"
        )));
    }
    Ok(())
}

fn rewind(run: &mut RunDocument, graph: &GraphDocument, target_id: &str) -> Result<(), RunError> {
    let closure = forward_closure(graph, target_id);
    for state in &mut run.nodes {
        if !closure.contains(state.node_id.as_str()) {
            continue;
        }
        state.status = if state.node_id == target_id {
            NodeRunStatus::Ready
        } else {
            NodeRunStatus::Pending
        };
        state.activated_by.clear();
        state.evidence_ids.clear();
        state.detail = None;
        state.human_decision = None;
        // A reopened attempt must not stay pinned to the previous executor.
        state.lease = None;
    }
    for edge in &mut run.edges {
        let definition = graph
            .edges
            .iter()
            .find(|definition| definition.id == edge.edge_id)
            .ok_or_else(|| RunError::EdgeNotFound(edge.edge_id.clone()))?;
        if closure.contains(definition.from.as_str()) && is_flow_edge(definition.kind) {
            edge.status = EdgeRunStatus::Pending;
        }
    }
    Ok(())
}

fn forward_closure<'a>(graph: &'a GraphDocument, start: &'a str) -> HashSet<&'a str> {
    let mut closure = HashSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(current) = queue.pop_front() {
        if !closure.insert(current) {
            continue;
        }
        queue.extend(
            graph
                .edges
                .iter()
                .filter(|edge| edge.from == current && is_flow_edge(edge.kind))
                .map(|edge| edge.to.as_str()),
        );
    }
    closure
}

fn state<'a>(run: &'a RunDocument, node_id: &str) -> Result<&'a crate::NodeRunState, RunError> {
    run.nodes
        .iter()
        .find(|state| state.node_id == node_id)
        .ok_or_else(|| RunError::NodeNotFound(node_id.to_owned()))
}

fn validate_reason(reason: &str) -> Result<(), RunError> {
    if reason.trim().is_empty() {
        return Err(RunError::InvalidRetry(
            "reason must not be empty".to_owned(),
        ));
    }
    if reason.len() > MAX_RUN_DETAIL_BYTES {
        return Err(RunError::DetailTooLarge(reason.len()));
    }
    Ok(())
}
