use std::collections::{HashMap, VecDeque};

use cortex_domain::{EdgeKind, GraphDocument};

use crate::{NodeOutcome, RunError};

pub(super) fn ensure_acyclic_flow(graph: &GraphDocument) -> Result<(), RunError> {
    let mut indegree = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), 0_usize))
        .collect::<HashMap<_, _>>();
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in graph.edges.iter().filter(|edge| is_flow_edge(edge.kind)) {
        *indegree.entry(edge.to.as_str()).or_default() += 1;
        outgoing
            .entry(edge.from.as_str())
            .or_default()
            .push(edge.to.as_str());
    }
    let mut queue = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect::<VecDeque<_>>();
    let mut visited = 0;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        for target in outgoing.get(node).into_iter().flatten() {
            if let Some(degree) = indegree.get_mut(target) {
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(target);
                }
            }
        }
    }
    if visited == graph.nodes.len() {
        Ok(())
    } else {
        Err(RunError::CyclicFlow)
    }
}

pub(super) fn incoming_flow(graph: &GraphDocument) -> HashMap<&str, Vec<&str>> {
    let mut incoming: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in graph.edges.iter().filter(|edge| is_flow_edge(edge.kind)) {
        incoming
            .entry(edge.to.as_str())
            .or_default()
            .push(edge.id.as_str());
    }
    incoming
}

pub(super) const fn is_flow_edge(kind: EdgeKind) -> bool {
    !matches!(
        kind,
        EdgeKind::Blocks | EdgeKind::Invalidates | EdgeKind::Supersedes
    )
}

pub(super) const fn edge_matches(kind: EdgeKind, outcome: NodeOutcome) -> bool {
    match outcome {
        NodeOutcome::Succeeded => matches!(
            kind,
            EdgeKind::Sequence
                | EdgeKind::Context
                | EdgeKind::Tool
                | EdgeKind::Success
                | EdgeKind::Approval
                | EdgeKind::Requires
        ),
        NodeOutcome::Failed => matches!(
            kind,
            EdgeKind::Failure | EdgeKind::Fallback | EdgeKind::Reject | EdgeKind::Escalates
        ),
    }
}
