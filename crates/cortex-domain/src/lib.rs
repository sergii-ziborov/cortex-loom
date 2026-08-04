#![doc = include_str!("../README.md")]

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

mod default_graph;

pub use default_graph::default_control_plane;

/// The wire format this crate reads and writes.
///
/// [`GraphDocument::validate`] accepts only this value, so a document that
/// declares a different schema is rejected rather than silently
/// misinterpreted.
pub const GRAPH_SCHEMA_VERSION: &str = "cortex-loom.graph.v1";
/// Upper bound on nodes in one document.
pub const MAX_GRAPH_NODES: usize = 4_096;
/// Upper bound on edges in one document.
pub const MAX_GRAPH_EDGES: usize = 16_384;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GraphDocument {
    pub schema_version: String,
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    #[serde(default)]
    pub description: String,
    pub position: Position,
    #[serde(default)]
    pub execution: Option<ExecutionPolicy>,
    #[serde(default)]
    pub provenance: Vec<Provenance>,
    #[serde(default)]
    /// Free-form typed configuration for the node kind, for example a retry
    /// controller's `targetNodeId` and `maxAttempts`.
    pub config: HashMap<String, blazingly_json::Value>,
}

/// What a node represents. The kind carries the semantics: gates block,
/// controllers branch or retry, and the rest do work.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Entry point that receives the request or task.
    Input,
    /// Work done by deterministic tooling, with no model involved.
    Deterministic,
    /// Repository graph and impact analysis.
    Weavatrix,
    /// A reusable methodology workflow, typically compiled from `SKILL.md`.
    Skill,
    /// A unit of work handed to an agent.
    AgentTask,
    /// A bounded local-model step; advisory output only.
    LocalModel,
    /// Gate on structure, provenance, risk, and budgets.
    QualityGate,
    /// Gate requiring an explicit human decision.
    HumanGate,
    /// Gate on a test run.
    TestGate,
    /// Gate requiring an explicit review decision.
    ReviewGate,
    /// Gate requiring cited evidence before proceeding.
    EvidenceGate,
    /// Controller that selects exactly one outgoing conditional edge.
    Branch,
    /// Controller that reopens a failed target for a bounded number of
    /// attempts; configured through `config` with `targetNodeId` and
    /// `maxAttempts`.
    Retry,
    /// Transfer of responsibility to another executor.
    Handoff,
    /// A terminal state that ends its path.
    Terminal,
    /// Work reserved for the strong upstream agent.
    UpstreamAgent,
    /// Exit point carrying the verified result.
    Output,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// How a node may be executed.
///
/// [`GraphDocument::validate`] enforces two authority rules on every policy,
/// not just structure: mutation authority is reserved for
/// [`ExecutionTarget::Upstream`] or [`ExecutionTarget::Human`], and so is any
/// target whose `risk` is [`RiskLevel::High`] or above. A graph that grants
/// mutation to a local model or an automated tool is rejected. This is a
/// deliberate safety default of this schema; if you need different rules,
/// leave `execution` as `None` and enforce your own policy above the graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPolicy {
    pub target: ExecutionTarget,
    pub risk: RiskLevel,
    pub max_input_tokens: u32,
    pub max_output_tokens: u32,
    pub require_evidence: bool,
    pub require_upstream_review: bool,
    pub allow_mutation: bool,
    #[serde(default)]
    pub model_profile: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTarget {
    Deterministic,
    Weavatrix,
    Ollama,
    Upstream,
    Human,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub source: String,
    pub locator: String,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub condition: Option<String>,
}

/// What an edge means. Executable kinds move a run forward; the rest express
/// relationships between nodes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Plain ordering: the target follows the source.
    Sequence,
    /// The source supplies context to the target.
    Context,
    /// The source invokes the target as a tool.
    Tool,
    /// Taken when the source succeeds.
    Success,
    /// Taken when the source fails.
    Failure,
    /// Taken only when explicitly selected by id; never inferred.
    Conditional,
    /// Recovery path taken when the source fails.
    Fallback,
    /// Taken when a gate approves.
    Approval,
    /// The target requires the source to have completed.
    Requires,
    /// Taken when a gate rejects.
    Reject,
    /// The source prevents the target from proceeding.
    Blocks,
    /// The source escalates to the target.
    Escalates,
    /// The source invalidates the target's result.
    Invalidates,
    /// The source replaces the target.
    Supersedes,
}

/// A structural validation failure.
///
/// Marked `#[non_exhaustive]` so new invariants can be reported without a
/// breaking release; match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GraphError {
    UnsupportedSchema(String),
    EmptyField(&'static str),
    EmptyNodeField { node: String, field: &'static str },
    EmptyEdgeField { edge: String, field: &'static str },
    TooManyNodes(usize),
    TooManyEdges(usize),
    DuplicateNode(String),
    DuplicateEdge(String),
    MissingEndpoint { edge: String, node: String },
    SelfEdge(String),
    InvalidPosition(String),
    InvalidExecution { node: String, message: &'static str },
}

impl Display for GraphError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchema(value) => {
                write!(formatter, "unsupported graph schema: {value}")
            }
            Self::EmptyField(field) => write!(formatter, "graph field must not be empty: {field}"),
            Self::EmptyNodeField { node, field } => {
                write!(formatter, "node {node} has an empty {field}")
            }
            Self::EmptyEdgeField { edge, field } => {
                write!(formatter, "edge {edge} has an empty {field}")
            }
            Self::TooManyNodes(count) => {
                write!(
                    formatter,
                    "graph has {count} nodes; limit is {MAX_GRAPH_NODES}"
                )
            }
            Self::TooManyEdges(count) => {
                write!(
                    formatter,
                    "graph has {count} edges; limit is {MAX_GRAPH_EDGES}"
                )
            }
            Self::DuplicateNode(id) => write!(formatter, "duplicate node id: {id}"),
            Self::DuplicateEdge(id) => write!(formatter, "duplicate edge id: {id}"),
            Self::MissingEndpoint { edge, node } => {
                write!(formatter, "edge {edge} references missing node {node}")
            }
            Self::SelfEdge(id) => write!(formatter, "self edge is not allowed: {id}"),
            Self::InvalidPosition(id) => write!(formatter, "node has a non-finite position: {id}"),
            Self::InvalidExecution { node, message } => {
                write!(
                    formatter,
                    "node {node} has an invalid execution policy: {message}"
                )
            }
        }
    }
}

impl std::error::Error for GraphError {}

impl GraphDocument {
    pub fn validate(&self) -> Result<(), GraphError> {
        if self.schema_version != GRAPH_SCHEMA_VERSION {
            return Err(GraphError::UnsupportedSchema(self.schema_version.clone()));
        }
        if self.id.trim().is_empty() {
            return Err(GraphError::EmptyField("id"));
        }
        if self.name.trim().is_empty() {
            return Err(GraphError::EmptyField("name"));
        }
        if self.nodes.len() > MAX_GRAPH_NODES {
            return Err(GraphError::TooManyNodes(self.nodes.len()));
        }
        if self.edges.len() > MAX_GRAPH_EDGES {
            return Err(GraphError::TooManyEdges(self.edges.len()));
        }

        let mut node_ids = HashSet::with_capacity(self.nodes.len());
        for node in &self.nodes {
            for (field, value) in [("id", node.id.as_str()), ("label", node.label.as_str())] {
                if value.trim().is_empty() {
                    return Err(GraphError::EmptyNodeField {
                        node: node.id.clone(),
                        field,
                    });
                }
            }
            if !node_ids.insert(node.id.as_str()) {
                return Err(GraphError::DuplicateNode(node.id.clone()));
            }
            if !node.position.x.is_finite() || !node.position.y.is_finite() {
                return Err(GraphError::InvalidPosition(node.id.clone()));
            }
            if let Some(policy) = &node.execution {
                validate_execution(&node.id, policy)?;
            }
        }

        let mut edge_ids = HashSet::with_capacity(self.edges.len());
        for edge in &self.edges {
            for (field, value) in [
                ("id", edge.id.as_str()),
                ("from", edge.from.as_str()),
                ("to", edge.to.as_str()),
            ] {
                if value.trim().is_empty() {
                    return Err(GraphError::EmptyEdgeField {
                        edge: edge.id.clone(),
                        field,
                    });
                }
            }
            if !edge_ids.insert(edge.id.as_str()) {
                return Err(GraphError::DuplicateEdge(edge.id.clone()));
            }

            if edge.from == edge.to {
                return Err(GraphError::SelfEdge(edge.id.clone()));
            }
            for endpoint in [&edge.from, &edge.to] {
                if !node_ids.contains(endpoint.as_str()) {
                    return Err(GraphError::MissingEndpoint {
                        edge: edge.id.clone(),
                        node: endpoint.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn reachable_from(&self, start: &str) -> HashSet<String> {
        let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &self.edges {
            outgoing.entry(&edge.from).or_default().push(&edge.to);
        }
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([start]);
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.to_owned()) {
                continue;
            }
            if let Some(targets) = outgoing.get(current) {
                queue.extend(targets.iter().copied());
            }
        }
        seen
    }
}

fn validate_execution(node: &str, policy: &ExecutionPolicy) -> Result<(), GraphError> {
    let invalid = |message| GraphError::InvalidExecution {
        node: node.to_owned(),
        message,
    };
    if policy.max_input_tokens == 0 || policy.max_output_tokens == 0 {
        return Err(invalid("token budgets must be greater than zero"));
    }
    if policy
        .model_profile
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(invalid("model profile must not be empty"));
    }
    if policy.allow_mutation
        && !matches!(
            policy.target,
            ExecutionTarget::Upstream | ExecutionTarget::Human
        )
    {
        return Err(invalid(
            "only upstream or human targets may receive mutation authority",
        ));
    }
    if policy.risk >= RiskLevel::High
        && !matches!(
            policy.target,
            ExecutionTarget::Upstream | ExecutionTarget::Human
        )
    {
        return Err(invalid(
            "high-risk work must target an upstream agent or human",
        ));
    }
    if policy.target == ExecutionTarget::Ollama
        && (!policy.require_upstream_review || policy.allow_mutation)
    {
        return Err(invalid(
            "Ollama work must be advisory and require upstream review",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
