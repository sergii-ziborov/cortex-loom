use std::collections::{HashMap, HashSet, VecDeque};

use cortex_domain::{EdgeKind, GraphDocument, GraphNode, NodeKind, RiskLevel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    UnreachableNode,
    MissingTerminal,
    ExecutableCycle,
    UnboundedRetry,
    GateWithoutFailureRoute,
    UnsafeLocalAuthority,
    BranchWithoutChoices,
    MissingCompletionCriteria,
    ExternalNodeReference,
    InvalidConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SequenceDiagnostic {
    pub code: DiagnosticCode,
    pub node_id: Option<String>,
    pub message: String,
    pub severity: DiagnosticSeverity,
}

#[must_use]
pub fn lint_sequence(graph: &GraphDocument) -> Vec<SequenceDiagnostic> {
    let mut diagnostics = Vec::new();
    let node_ids: HashSet<_> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    for edge in &graph.edges {
        for endpoint in [&edge.from, &edge.to] {
            if !node_ids.contains(endpoint.as_str()) {
                diagnostics.push(error(
                    DiagnosticCode::ExternalNodeReference,
                    None,
                    format!("edge {} references missing node {endpoint}", edge.id),
                ));
            }
        }
    }

    let executable: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| is_executable(node))
        .collect();
    let executable_ids: HashSet<_> = executable.iter().map(|node| node.id.as_str()).collect();
    if !executable
        .iter()
        .any(|node| node.kind == NodeKind::Terminal)
    {
        diagnostics.push(error(
            DiagnosticCode::MissingTerminal,
            None,
            "sequence has no terminal workflow step".to_owned(),
        ));
    }
    if let Some(start) = executable.iter().min_by_key(|node| order(node)) {
        let reachable = reachable(graph, &executable_ids, &start.id);
        for node in &executable {
            if !reachable.contains(node.id.as_str()) {
                diagnostics.push(error(
                    DiagnosticCode::UnreachableNode,
                    Some(&node.id),
                    format!("workflow step {} is unreachable", node.id),
                ));
            }
        }
    }
    if has_cycle(graph, &executable_ids) {
        diagnostics.push(error(
            DiagnosticCode::ExecutableCycle,
            None,
            "sequence contains an executable cycle outside a bounded retry controller".to_owned(),
        ));
    }

    for node in executable {
        lint_node(graph, node, &node_ids, &mut diagnostics);
    }
    diagnostics
}

fn lint_node(
    graph: &GraphDocument,
    node: &GraphNode,
    node_ids: &HashSet<&str>,
    diagnostics: &mut Vec<SequenceDiagnostic>,
) {
    if node.kind == NodeKind::Retry {
        let attempts = node
            .config
            .get("maxAttempts")
            .and_then(serde_json::Value::as_u64);
        let target = node
            .config
            .get("targetNodeId")
            .and_then(serde_json::Value::as_str);
        if !matches!(attempts, Some(1..=3)) || target.is_none_or(|value| !node_ids.contains(value))
        {
            diagnostics.push(error(
                DiagnosticCode::UnboundedRetry,
                Some(&node.id),
                "retry requires maxAttempts 1..=3 and an existing targetNodeId".to_owned(),
            ));
        }
    }
    if is_gate(node.kind)
        && !graph.edges.iter().any(|edge| {
            edge.from == node.id
                && matches!(
                    edge.kind,
                    EdgeKind::Failure | EdgeKind::Fallback | EdgeKind::Reject | EdgeKind::Escalates
                )
        })
    {
        diagnostics.push(error(
            DiagnosticCode::GateWithoutFailureRoute,
            Some(&node.id),
            "gate requires an explicit failure, reject, fallback, or escalation route".to_owned(),
        ));
    }
    if node.kind == NodeKind::LocalModel
        && node.execution.as_ref().is_some_and(|policy| {
            policy.allow_mutation
                || policy.risk >= RiskLevel::High
                || !policy.require_upstream_review
        })
    {
        diagnostics.push(error(
            DiagnosticCode::UnsafeLocalAuthority,
            Some(&node.id),
            "local-model steps must be advisory, below high risk, and upstream-reviewed".to_owned(),
        ));
    }
    if node.kind == NodeKind::Branch {
        let choices = graph
            .edges
            .iter()
            .filter(|edge| edge.from == node.id && edge.kind == EdgeKind::Conditional)
            .count();
        if choices < 2 {
            diagnostics.push(error(
                DiagnosticCode::BranchWithoutChoices,
                Some(&node.id),
                "branch requires at least two explicit conditional choices".to_owned(),
            ));
        }
    }
    match node.config.get("completionCriteria") {
        Some(serde_json::Value::Array(values))
            if !values.is_empty()
                && values
                    .iter()
                    .all(|value| value.as_str().is_some_and(|item| !item.trim().is_empty())) => {}
        None => diagnostics.push(error(
            DiagnosticCode::MissingCompletionCriteria,
            Some(&node.id),
            "workflow step requires completionCriteria".to_owned(),
        )),
        Some(_) => diagnostics.push(error(
            DiagnosticCode::InvalidConfig,
            Some(&node.id),
            "completionCriteria must be a non-empty string array".to_owned(),
        )),
    }
    for (key, minimum, maximum) in [("maxInputTokens", 1, 100_000), ("maxAttempts", 1, 3)] {
        if node.config.get(key).is_some_and(|value| {
            value
                .as_u64()
                .is_none_or(|number| number < minimum || number > maximum)
        }) {
            diagnostics.push(error(
                DiagnosticCode::InvalidConfig,
                Some(&node.id),
                format!("{key} must be an integer in {minimum}..={maximum}"),
            ));
        }
    }
}

fn reachable<'a>(
    graph: &'a GraphDocument,
    executable: &HashSet<&'a str>,
    start: &'a str,
) -> HashSet<&'a str> {
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &graph.edges {
        if executable.contains(edge.from.as_str())
            && executable.contains(edge.to.as_str())
            && is_transition(edge.kind)
        {
            outgoing.entry(&edge.from).or_default().push(&edge.to);
        }
    }
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(current) = queue.pop_front() {
        if seen.insert(current)
            && let Some(next) = outgoing.get(current)
        {
            queue.extend(next.iter().copied());
        }
    }
    seen
}

fn has_cycle<'a>(graph: &'a GraphDocument, executable: &HashSet<&'a str>) -> bool {
    let retry_ids: HashSet<_> = graph
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Retry)
        .map(|node| node.id.as_str())
        .collect();
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &graph.edges {
        if executable.contains(edge.from.as_str())
            && executable.contains(edge.to.as_str())
            && is_transition(edge.kind)
            && !(retry_ids.contains(edge.from.as_str()) && edge.kind == EdgeKind::Fallback)
        {
            outgoing.entry(&edge.from).or_default().push(&edge.to);
        }
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    executable
        .iter()
        .any(|node| visit_cycle(node, &outgoing, &mut visiting, &mut visited))
}

fn visit_cycle<'a>(
    node: &'a str,
    outgoing: &HashMap<&'a str, Vec<&'a str>>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> bool {
    if visited.contains(node) {
        return false;
    }
    if !visiting.insert(node) {
        return true;
    }
    if outgoing.get(node).is_some_and(|next| {
        next.iter()
            .any(|target| visit_cycle(target, outgoing, visiting, visited))
    }) {
        return true;
    }
    visiting.remove(node);
    visited.insert(node);
    false
}

fn is_executable(node: &GraphNode) -> bool {
    node.config.get("role").and_then(serde_json::Value::as_str) == Some("workflow_step")
}

fn order(node: &GraphNode) -> u64 {
    node.config
        .get("order")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX)
}

const fn is_gate(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::QualityGate
            | NodeKind::HumanGate
            | NodeKind::TestGate
            | NodeKind::ReviewGate
            | NodeKind::EvidenceGate
    )
}

const fn is_transition(kind: EdgeKind) -> bool {
    !matches!(
        kind,
        EdgeKind::Context
            | EdgeKind::Requires
            | EdgeKind::Blocks
            | EdgeKind::Invalidates
            | EdgeKind::Supersedes
    )
}

fn error(code: DiagnosticCode, node_id: Option<&str>, message: String) -> SequenceDiagnostic {
    SequenceDiagnostic {
        code,
        node_id: node_id.map(str::to_owned),
        message,
        severity: DiagnosticSeverity::Error,
    }
}
