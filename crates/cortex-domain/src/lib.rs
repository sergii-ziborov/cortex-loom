use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

mod default_graph;

pub use default_graph::default_control_plane;

pub const GRAPH_SCHEMA_VERSION: &str = "cortex-loom.graph.v1";
pub const MAX_GRAPH_NODES: usize = 4_096;
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
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Input,
    Deterministic,
    Weavatrix,
    Skill,
    AgentTask,
    LocalModel,
    QualityGate,
    HumanGate,
    TestGate,
    ReviewGate,
    EvidenceGate,
    Branch,
    Retry,
    Handoff,
    Terminal,
    UpstreamAgent,
    Output,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Sequence,
    Context,
    Tool,
    Success,
    Failure,
    Conditional,
    Fallback,
    Approval,
    Requires,
    Reject,
    Blocks,
    Escalates,
    Invalidates,
    Supersedes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
