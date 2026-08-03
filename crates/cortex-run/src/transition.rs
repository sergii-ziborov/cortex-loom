use std::collections::HashSet;

use cortex_domain::GraphDocument;

use crate::flow::{edge_matches, is_flow_edge};
use crate::{EdgeRunStatus, NodeOutcome, NodeRunStatus, RunDocument, RunError, RunStatus};

pub(super) fn complete_node(
    run: &mut RunDocument,
    graph: &GraphDocument,
    node_id: &str,
    outcome: NodeOutcome,
    selected: &[String],
    evidence: &[String],
    detail: Option<String>,
) -> Result<Vec<String>, RunError> {
    let node = run
        .nodes
        .iter_mut()
        .find(|node| node.node_id == node_id)
        .ok_or_else(|| RunError::NodeNotFound(node_id.to_owned()))?;
    if node.status != NodeRunStatus::Running {
        return Err(RunError::InvalidNodeState {
            node: node_id.to_owned(),
            expected: NodeRunStatus::Running,
            current: node.status,
        });
    }
    node.status = match outcome {
        NodeOutcome::Succeeded => NodeRunStatus::Succeeded,
        NodeOutcome::Failed => NodeRunStatus::Failed,
    };
    node.evidence_ids.clear();
    node.evidence_ids.extend_from_slice(evidence);
    node.detail = detail;

    let selected = selected.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut traversed = Vec::new();
    for edge in graph.edges.iter().filter(|edge| edge.from == node_id) {
        if !is_flow_edge(edge.kind) {
            continue;
        }
        let take = (selected.contains(edge.id.as_str()) || edge_matches(edge.kind, outcome))
            && retry_available(run, graph, node_id, &edge.to);
        set_edge(
            run,
            &edge.id,
            if take {
                traversed.push(edge.id.clone());
                EdgeRunStatus::Traversed
            } else {
                EdgeRunStatus::NotTaken
            },
        )?;
    }

    stabilize(run, graph)?;
    update_run_status(run, graph);
    Ok(traversed)
}

fn retry_available(
    run: &RunDocument,
    graph: &GraphDocument,
    source_id: &str,
    target_id: &str,
) -> bool {
    let Some(retry) = graph
        .nodes
        .iter()
        .find(|node| node.id == target_id && node.kind == cortex_domain::NodeKind::Retry)
    else {
        return true;
    };
    if retry
        .config
        .get("targetNodeId")
        .and_then(serde_json::Value::as_str)
        != Some(source_id)
    {
        return true;
    }
    let max = retry
        .config
        .get("maxAttempts")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    run.nodes
        .iter()
        .find(|node| node.node_id == source_id)
        .is_none_or(|node| u64::from(node.attempt) < max)
}

pub(super) fn cancel_run(run: &mut RunDocument) {
    for node in &mut run.nodes {
        if matches!(
            node.status,
            NodeRunStatus::Pending | NodeRunStatus::Ready | NodeRunStatus::Running
        ) {
            node.status = NodeRunStatus::Cancelled;
        }
    }
    for edge in &mut run.edges {
        if edge.status == EdgeRunStatus::Pending {
            edge.status = EdgeRunStatus::NotTaken;
        }
    }
    run.status = RunStatus::Cancelled;
}

fn stabilize(run: &mut RunDocument, graph: &GraphDocument) -> Result<(), RunError> {
    loop {
        let mut changed = false;
        for node in &graph.nodes {
            if node_state(run, &node.id)? != NodeRunStatus::Pending {
                continue;
            }
            let incoming = graph
                .edges
                .iter()
                .filter(|edge| edge.to == node.id && is_flow_edge(edge.kind))
                .collect::<Vec<_>>();
            if incoming.is_empty() {
                continue;
            }
            let statuses = incoming
                .iter()
                .map(|edge| edge_state(run, &edge.id))
                .collect::<Result<Vec<_>, _>>()?;
            if statuses.contains(&EdgeRunStatus::Pending) {
                continue;
            }
            let activated_by = incoming
                .iter()
                .zip(&statuses)
                .filter(|(_, status)| **status == EdgeRunStatus::Traversed)
                .map(|(edge, _)| edge.id.clone())
                .collect::<Vec<_>>();
            let next = if activated_by.is_empty() {
                NodeRunStatus::Skipped
            } else {
                NodeRunStatus::Ready
            };
            let state = run
                .nodes
                .iter_mut()
                .find(|state| state.node_id == node.id)
                .ok_or_else(|| RunError::NodeNotFound(node.id.clone()))?;
            state.status = next;
            state.activated_by = activated_by;
            if next == NodeRunStatus::Skipped {
                skip_outgoing(run, graph, &node.id)?;
            }
            changed = true;
        }
        if !changed {
            return Ok(());
        }
    }
}

fn skip_outgoing(
    run: &mut RunDocument,
    graph: &GraphDocument,
    node_id: &str,
) -> Result<(), RunError> {
    for edge in graph
        .edges
        .iter()
        .filter(|edge| edge.from == node_id && is_flow_edge(edge.kind))
    {
        set_edge(run, &edge.id, EdgeRunStatus::NotTaken)?;
    }
    Ok(())
}

fn update_run_status(run: &mut RunDocument, graph: &GraphDocument) {
    if run.nodes.iter().any(|node| {
        matches!(
            node.status,
            NodeRunStatus::Pending | NodeRunStatus::Ready | NodeRunStatus::Running
        )
    }) {
        run.status = RunStatus::Running;
        return;
    }
    let active_sinks = graph.nodes.iter().filter_map(|node| {
        let has_outgoing = graph
            .edges
            .iter()
            .any(|edge| edge.from == node.id && is_flow_edge(edge.kind));
        let state = run.nodes.iter().find(|state| state.node_id == node.id)?;
        (!has_outgoing && state.status != NodeRunStatus::Skipped).then_some(state.status)
    });
    let statuses = active_sinks.collect::<Vec<_>>();
    run.status = if !statuses.is_empty()
        && statuses
            .iter()
            .all(|status| *status == NodeRunStatus::Succeeded)
    {
        RunStatus::Succeeded
    } else {
        RunStatus::Failed
    };
}

fn node_state(run: &RunDocument, id: &str) -> Result<NodeRunStatus, RunError> {
    run.nodes
        .iter()
        .find(|node| node.node_id == id)
        .map(|node| node.status)
        .ok_or_else(|| RunError::NodeNotFound(id.to_owned()))
}

fn edge_state(run: &RunDocument, id: &str) -> Result<EdgeRunStatus, RunError> {
    run.edges
        .iter()
        .find(|edge| edge.edge_id == id)
        .map(|edge| edge.status)
        .ok_or_else(|| RunError::EdgeNotFound(id.to_owned()))
}

fn set_edge(run: &mut RunDocument, id: &str, status: EdgeRunStatus) -> Result<(), RunError> {
    let edge = run
        .edges
        .iter_mut()
        .find(|edge| edge.edge_id == id)
        .ok_or_else(|| RunError::EdgeNotFound(id.to_owned()))?;
    edge.status = status;
    Ok(())
}
