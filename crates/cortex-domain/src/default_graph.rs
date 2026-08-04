use std::collections::HashMap;

use crate::{
    EdgeKind, GRAPH_SCHEMA_VERSION, GraphDocument, GraphEdge, GraphNode, NodeKind, Position,
};

/// One valid example graph: a request fans out to deterministic and
/// repository analysis, both feed a gate, and the gate hands off to the
/// upstream agent.
///
/// It exists so tests and first-run experiences have a graph that passes
/// [`GraphDocument::validate`]. It is an example, not a recommended
/// topology — build your own.
#[must_use]
pub fn default_control_plane() -> GraphDocument {
    GraphDocument {
        schema_version: GRAPH_SCHEMA_VERSION.to_owned(),
        id: "default-control-plane".to_owned(),
        name: "Cortex Loom control plane".to_owned(),
        revision: 1,
        nodes: default_nodes(),
        edges: default_edges(),
        metadata: HashMap::from([
            ("source".to_owned(), "cortex-domain default".to_owned()),
            (
                "localModelPolicy".to_owned(),
                "bounded-and-reviewable".to_owned(),
            ),
        ]),
    }
}

fn default_nodes() -> Vec<GraphNode> {
    vec![
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
            "Upstream coding agent",
            1010.0,
            170.0,
        ),
        node("result", NodeKind::Output, "Verified result", 1250.0, 170.0),
    ]
}

fn default_edges() -> Vec<GraphEdge> {
    vec![
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
    ]
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
