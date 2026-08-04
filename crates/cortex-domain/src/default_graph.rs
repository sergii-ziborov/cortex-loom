use std::collections::HashMap;

use crate::{
    EdgeKind, ExecutionPolicy, ExecutionTarget, GRAPH_SCHEMA_VERSION, GraphDocument, GraphEdge,
    GraphNode, NodeKind, Position, RiskLevel,
};

/// One valid example graph: a request fans out to deterministic and
/// repository analysis, both feed a gate, and the gate hands off to the
/// upstream agent.
///
/// It exists so tests and first-run experiences have a graph that passes
/// [`GraphDocument::validate`]. It is an example, not a recommended
/// topology â€” build your own.
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
            "The task as stated, before any tool has looked at the repository.",
            40.0,
            210.0,
        ),
        node(
            "scan",
            NodeKind::Deterministic,
            "Deterministic scan",
            "Parsers and repository tools reduce the search space. No model runs here, so the result is reproducible.",
            250.0,
            80.0,
        )
        .with_execution(policy(
            ExecutionTarget::Deterministic,
            RiskLevel::Low,
            false,
            false,
        )),
        node(
            "weavatrix",
            NodeKind::Weavatrix,
            "Weavatrix evidence",
            "Revision-bound graph, impact, and symbol evidence, returned as individually citable fragments.",
            250.0,
            260.0,
        )
        .with_execution(policy(ExecutionTarget::Weavatrix, RiskLevel::Low, false, false)),
        node(
            "skill",
            NodeKind::Skill,
            "Skill workflow",
            "The methodology that applies to this task, compiled from a readable SKILL.md.",
            500.0,
            80.0,
        ),
        node(
            "local",
            NodeKind::LocalModel,
            "Local bounded draft",
            "A small local model drafts within a token budget. Advisory only: it cites supplied evidence ids and may not mutate anything.",
            500.0,
            260.0,
        )
        .with_execution(policy(ExecutionTarget::Ollama, RiskLevel::Low, false, false)),
        node(
            "gate",
            NodeKind::QualityGate,
            "Evidence + risk gate",
            "Checks structure, provenance, risk, and budgets. Missing or contradictory evidence escalates instead of passing.",
            760.0,
            170.0,
        ),
        node(
            "upstream",
            NodeKind::UpstreamAgent,
            "Upstream coding agent",
            "The strong agent receives the compact cited packet and owns every ambiguous, high-risk, or mutating decision.",
            1010.0,
            170.0,
        )
        .with_execution(policy(ExecutionTarget::Upstream, RiskLevel::Medium, true, true)),
        node(
            "result",
            NodeKind::Output,
            "Verified result",
            "The accepted outcome, reached only through the gate and the upstream agent.",
            1250.0,
            170.0,
        ),
    ]
}

/// Only `Upstream` and `Human` targets may hold mutation authority, and the
/// same applies at `High` risk or above â€” `GraphDocument::validate` rejects
/// anything else.
///
/// `require_evidence` is set only where citations are the point. The nodes
/// that *produce* evidence, and the advisory local draft, are not asked to
/// cite it; the upstream agent, which produces the actual change, is.
fn policy(
    target: ExecutionTarget,
    risk: RiskLevel,
    allow_mutation: bool,
    require_evidence: bool,
) -> ExecutionPolicy {
    ExecutionPolicy {
        target,
        risk,
        max_input_tokens: 4_000,
        max_output_tokens: 1_024,
        require_evidence,
        require_upstream_review: !allow_mutation,
        allow_mutation,
        model_profile: None,
    }
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

fn node(id: &str, kind: NodeKind, label: &str, description: &str, x: f64, y: f64) -> GraphNode {
    GraphNode {
        id: id.to_owned(),
        kind,
        label: label.to_owned(),
        description: description.to_owned(),
        position: Position { x, y },
        execution: None,
        provenance: Vec::new(),
        config: HashMap::new(),
    }
}

trait WithExecution {
    fn with_execution(self, policy: ExecutionPolicy) -> Self;
}

impl WithExecution for GraphNode {
    fn with_execution(mut self, policy: ExecutionPolicy) -> Self {
        self.execution = Some(policy);
        self
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
