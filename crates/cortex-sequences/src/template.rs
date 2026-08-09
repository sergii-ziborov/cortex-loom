use std::fmt::{Display, Formatter};

use cortex_domain::{EdgeKind, GraphDocument, GraphEdge, NodeKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::SequenceError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct TemplateVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl TemplateVersion {
    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl Display for TemplateVersion {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivationHints {
    pub task_classes: &'static [&'static str],
    pub intents: &'static [&'static str],
    pub risks: &'static [&'static str],
    pub mutation: bool,
    pub evidence_classes: &'static [&'static str],
    pub lexical_cues: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub struct SequenceTemplate {
    pub id: &'static str,
    pub version: TemplateVersion,
    pub title: &'static str,
    pub description: &'static str,
    pub markdown: &'static str,
    pub changelog: &'static str,
    pub activation: ActivationHints,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateRef {
    pub id: String,
    pub version: TemplateVersion,
    pub fingerprint: String,
}

#[must_use]
pub const fn templates() -> &'static [SequenceTemplate] {
    &crate::catalog::TEMPLATES
}

/// Create a detached, editable graph from an immutable built-in template.
///
/// # Errors
///
/// Returns [`SequenceError`] when the template is unknown, the requested
/// identity is empty, or the bundled Markdown cannot compile and validate.
pub fn instantiate_template(
    template_id: &str,
    graph_id: &str,
    name: &str,
) -> Result<GraphDocument, SequenceError> {
    if graph_id.trim().is_empty() || name.trim().is_empty() {
        return Err(SequenceError::InvalidCopy(
            "sequence graph id and name must not be empty".to_owned(),
        ));
    }
    let template = templates()
        .iter()
        .find(|candidate| candidate.id == template_id)
        .ok_or_else(|| SequenceError::UnknownTemplate(template_id.to_owned()))?;
    let source = format!("cortex-sequences/templates/{}.md", template.id);
    let mut graph = cortex_skills::import_skill_markdown(&source, template.markdown)?;
    let canonical = cortex_skills::export_skill_markdown(&graph)?;
    let fingerprint = format!("{:x}", Sha256::digest(canonical.as_bytes()));
    enrich_template_graph(&mut graph);
    graph_id.trim().clone_into(&mut graph.id);
    name.trim().clone_into(&mut graph.name);
    graph.revision = 0;
    graph
        .metadata
        .insert("sequence.templateId".to_owned(), template.id.to_owned());
    graph.metadata.insert(
        "sequence.templateVersion".to_owned(),
        template.version.to_string(),
    );
    graph
        .metadata
        .insert("sequence.templateFingerprint".to_owned(), fingerprint);
    graph
        .metadata
        .insert("sequence.editable".to_owned(), "true".to_owned());
    graph
        .validate()
        .map_err(|error| SequenceError::InvalidCopy(error.to_string()))?;
    Ok(graph)
}

fn enrich_template_graph(graph: &mut GraphDocument) {
    let mut workflow: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| {
            node.config.get("role").and_then(serde_json::Value::as_str) == Some("workflow_step")
        })
        .map(|node| {
            (
                node.id.clone(),
                node.kind,
                node.config
                    .get("order")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(u64::MAX),
            )
        })
        .collect();
    workflow.sort_by_key(|(_, _, order)| *order);

    for node in graph.nodes.iter_mut().filter(|node| {
        node.config.get("role").and_then(serde_json::Value::as_str) == Some("workflow_step")
    }) {
        node.config.insert(
            "instruction".to_owned(),
            serde_json::Value::String(node.label.clone()),
        );
        node.config.insert(
            "completionCriteria".to_owned(),
            serde_json::json!([format!("Completed: {}", node.label)]),
        );
        node.config.insert(
            "requiredEvidence".to_owned(),
            serde_json::json!(required_evidence(node.kind)),
        );
        node.config.insert(
            "maxInputTokens".to_owned(),
            serde_json::Value::from(input_budget(node.kind)),
        );
        node.config
            .insert("maxAttempts".to_owned(), serde_json::Value::from(1));
        node.config.insert(
            "executor".to_owned(),
            serde_json::Value::String(executor(node.kind).to_owned()),
        );
    }

    let mut extra_edges = Vec::new();
    for (index, (id, kind, _)) in workflow.iter().enumerate() {
        if *kind == NodeKind::Retry
            && let Some((target, _, _)) = index.checked_sub(1).and_then(|item| workflow.get(item))
        {
            if let Some(node) = graph.nodes.iter_mut().find(|node| node.id == *id) {
                node.config.insert(
                    "targetNodeId".to_owned(),
                    serde_json::Value::String(target.clone()),
                );
            }
            extra_edges.push(GraphEdge {
                id: format!("sequence-retry-{id}"),
                from: id.clone(),
                to: target.clone(),
                kind: EdgeKind::Fallback,
                label: "bounded retry".to_owned(),
                condition: None,
            });
        }
        if is_gate(*kind)
            && let Some((target, _, _)) = workflow[index + 1..].iter().find(|(_, candidate, _)| {
                matches!(candidate, NodeKind::UpstreamAgent | NodeKind::Handoff)
            })
        {
            extra_edges.push(GraphEdge {
                id: format!("sequence-escalate-{id}"),
                from: id.clone(),
                to: target.clone(),
                kind: EdgeKind::Escalates,
                label: "insufficient or rejected".to_owned(),
                condition: None,
            });
        }
    }
    graph.edges.extend(extra_edges);
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

const fn executor(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Weavatrix => "weavatrix",
        NodeKind::AgentTask | NodeKind::UpstreamAgent | NodeKind::Handoff => "upstream",
        NodeKind::HumanGate => "human",
        NodeKind::LocalModel => "micro_extract",
        _ => "deterministic",
    }
}

const fn input_budget(kind: NodeKind) -> u32 {
    match kind {
        NodeKind::Weavatrix => 4_000,
        NodeKind::AgentTask | NodeKind::UpstreamAgent => 8_000,
        NodeKind::LocalModel => 1_500,
        _ => 1_000,
    }
}

const fn required_evidence(kind: NodeKind) -> &'static [&'static str] {
    match kind {
        NodeKind::EvidenceGate => &["current-attempt evidence"],
        NodeKind::TestGate => &["test output"],
        NodeKind::ReviewGate => &["review findings", "diff"],
        NodeKind::QualityGate => &["policy result"],
        NodeKind::UpstreamAgent | NodeKind::AgentTask => &["cited repository evidence"],
        _ => &[],
    }
}
