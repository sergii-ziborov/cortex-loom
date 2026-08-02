use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

pub const GRAPH_SCHEMA_VERSION: &str = "cortex-loom.graph.v1";

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
    LocalModel,
    QualityGate,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    UnsupportedSchema(String),
    EmptyField(&'static str),
    DuplicateNode(String),
    DuplicateEdge(String),
    MissingEndpoint { edge: String, node: String },
    SelfEdge(String),
    InvalidPosition(String),
}

impl Display for GraphError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchema(value) => {
                write!(formatter, "unsupported graph schema: {value}")
            }
            Self::EmptyField(field) => write!(formatter, "graph field must not be empty: {field}"),
            Self::DuplicateNode(id) => write!(formatter, "duplicate node id: {id}"),
            Self::DuplicateEdge(id) => write!(formatter, "duplicate edge id: {id}"),
            Self::MissingEndpoint { edge, node } => {
                write!(formatter, "edge {edge} references missing node {node}")
            }
            Self::SelfEdge(id) => write!(formatter, "self edge is not allowed: {id}"),
            Self::InvalidPosition(id) => write!(formatter, "node has a non-finite position: {id}"),
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

        let mut node_ids = HashSet::with_capacity(self.nodes.len());
        for node in &self.nodes {
            if !node_ids.insert(node.id.as_str()) {
                return Err(GraphError::DuplicateNode(node.id.clone()));
            }
            if !node.position.x.is_finite() || !node.position.y.is_finite() {
                return Err(GraphError::InvalidPosition(node.id.clone()));
            }
        }

        let mut edge_ids = HashSet::with_capacity(self.edges.len());
        for edge in &self.edges {
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

#[must_use]
pub fn default_control_plane() -> GraphDocument {
    let nodes = vec![
        node(
            "request",
            NodeKind::Input,
            "Engineering request",
            40.0,
            210.0,
        ),
        node(
            "scan",
            NodeKind::Deterministic,
            "Deterministic scan",
            250.0,
            80.0,
        ),
        node(
            "weavatrix",
            NodeKind::Weavatrix,
            "Weavatrix evidence",
            250.0,
            260.0,
        ),
        node("skill", NodeKind::Skill, "Skill workflow", 500.0, 80.0),
        node(
            "local",
            NodeKind::LocalModel,
            "Local bounded draft",
            500.0,
            260.0,
        ),
        node(
            "gate",
            NodeKind::QualityGate,
            "Evidence + risk gate",
            760.0,
            170.0,
        ),
        node(
            "upstream",
            NodeKind::UpstreamAgent,
            "Codex / Claude",
            1010.0,
            170.0,
        ),
        node("result", NodeKind::Output, "Verified result", 1250.0, 170.0),
    ];
    let edges = vec![
        edge("e1", "request", "scan", EdgeKind::Tool, "facts"),
        edge("e2", "request", "weavatrix", EdgeKind::Tool, "repo graph"),
        edge("e3", "scan", "skill", EdgeKind::Context, "structured input"),
        edge(
            "e4",
            "weavatrix",
            "local",
            EdgeKind::Context,
            "bounded evidence",
        ),
        edge("e5", "skill", "gate", EdgeKind::Context, "workflow rules"),
        edge(
            "e6",
            "local",
            "gate",
            EdgeKind::Success,
            "draft + citations",
        ),
        edge(
            "e7",
            "local",
            "upstream",
            EdgeKind::Fallback,
            "invalid or uncertain",
        ),
        edge(
            "e8",
            "gate",
            "upstream",
            EdgeKind::Approval,
            "decision context",
        ),
        edge(
            "e9",
            "upstream",
            "result",
            EdgeKind::Success,
            "implementation",
        ),
    ];
    GraphDocument {
        schema_version: GRAPH_SCHEMA_VERSION.to_owned(),
        id: "default-control-plane".to_owned(),
        name: "Cortex Loom control plane".to_owned(),
        revision: 1,
        nodes,
        edges,
        metadata: HashMap::from([
            (
                "source".to_owned(),
                "AI Dev System graph editor extraction".to_owned(),
            ),
            (
                "localModelPolicy".to_owned(),
                "bounded-and-reviewable".to_owned(),
            ),
        ]),
    }
}

fn node(id: &str, kind: NodeKind, label: &str, x: f64, y: f64) -> GraphNode {
    GraphNode {
        id: id.to_owned(),
        kind,
        label: label.to_owned(),
        description: String::new(),
        position: Position { x, y },
        execution: None,
        provenance: Vec::new(),
        config: HashMap::new(),
    }
}

fn edge(id: &str, from: &str, to: &str, kind: EdgeKind, label: &str) -> GraphEdge {
    GraphEdge {
        id: id.to_owned(),
        from: from.to_owned(),
        to: to.to_owned(),
        kind,
        label: label.to_owned(),
        condition: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_graph_is_valid_and_connected() {
        let graph = default_control_plane();
        graph.validate().expect("default graph must remain valid");
        assert_eq!(graph.reachable_from("request").len(), graph.nodes.len());
    }

    #[test]
    fn rejects_an_edge_to_a_missing_node() {
        let mut graph = default_control_plane();
        graph.edges[0].to = "missing".to_owned();
        assert!(matches!(
            graph.validate(),
            Err(GraphError::MissingEndpoint { node, .. }) if node == "missing"
        ));
    }
}
