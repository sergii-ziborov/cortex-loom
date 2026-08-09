use std::collections::HashSet;

use cortex_domain::{EdgeKind, GraphDocument};
use serde::{Deserialize, Serialize};

use crate::{DiagnosticSeverity, SequenceError, lint_sequence};

const MAX_EVIDENCE_IDS: usize = 64;
const MAX_EVIDENCE_ID_BYTES: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActiveStepPacket {
    pub graph_id: String,
    pub graph_revision: u64,
    pub node_id: String,
    pub instruction: String,
    pub required_evidence: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub completion_criteria: Vec<String>,
    pub max_input_tokens: u32,
    pub max_attempts: u32,
    pub executor: String,
    pub success_edges: Vec<String>,
    pub recovery_edges: Vec<String>,
    pub escalation_edges: Vec<String>,
}

/// Compile only one active workflow step into a bounded methodology packet.
///
/// # Errors
///
/// Returns [`SequenceError`] when the graph fails sequence lint, the node is
/// absent, its typed configuration is invalid, or evidence IDs exceed bounds.
pub fn active_step_packet(
    graph: &GraphDocument,
    node_id: &str,
    evidence_ids: &[String],
) -> Result<ActiveStepPacket, SequenceError> {
    if let Some(diagnostic) = lint_sequence(graph)
        .into_iter()
        .find(|item| item.severity == DiagnosticSeverity::Error)
    {
        return Err(SequenceError::InvalidSequence(diagnostic.message));
    }
    let node = graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| SequenceError::NodeNotFound(node_id.to_owned()))?;
    let evidence_ids = bounded_evidence_ids(evidence_ids)?;
    let instruction =
        optional_string(node.config.get("instruction"))?.unwrap_or_else(|| node.label.clone());
    let required_evidence = string_array(node.config.get("requiredEvidence"), true)?;
    let completion_criteria = string_array(node.config.get("completionCriteria"), false)?;
    let max_input_tokens = bounded_u32(node.config.get("maxInputTokens"), 1, 100_000, 1_000)?;
    let max_attempts = bounded_u32(node.config.get("maxAttempts"), 1, 3, 1)?;
    let executor =
        optional_string(node.config.get("executor"))?.unwrap_or_else(|| "upstream".to_owned());
    let mut success_edges = Vec::new();
    let mut recovery_edges = Vec::new();
    let mut escalation_edges = Vec::new();
    for edge in graph.edges.iter().filter(|edge| edge.from == node.id) {
        match edge.kind {
            EdgeKind::Sequence | EdgeKind::Success | EdgeKind::Approval | EdgeKind::Conditional => {
                success_edges.push(edge.to.clone());
            }
            EdgeKind::Failure | EdgeKind::Fallback | EdgeKind::Reject => {
                recovery_edges.push(edge.to.clone());
            }
            EdgeKind::Escalates => escalation_edges.push(edge.to.clone()),
            EdgeKind::Context
            | EdgeKind::Tool
            | EdgeKind::Requires
            | EdgeKind::Blocks
            | EdgeKind::Invalidates
            | EdgeKind::Supersedes => {}
        }
    }
    sort_unique(&mut success_edges);
    sort_unique(&mut recovery_edges);
    sort_unique(&mut escalation_edges);
    Ok(ActiveStepPacket {
        graph_id: graph.id.clone(),
        graph_revision: graph.revision,
        node_id: node.id.clone(),
        instruction,
        required_evidence,
        evidence_ids,
        completion_criteria,
        max_input_tokens,
        max_attempts,
        executor,
        success_edges,
        recovery_edges,
        escalation_edges,
    })
}

fn bounded_evidence_ids(values: &[String]) -> Result<Vec<String>, SequenceError> {
    if values.len() > MAX_EVIDENCE_IDS {
        return Err(SequenceError::InvalidSequence(format!(
            "active step accepts at most {MAX_EVIDENCE_IDS} evidence IDs"
        )));
    }
    let mut seen = HashSet::new();
    let mut result = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if value.is_empty() || value.len() > MAX_EVIDENCE_ID_BYTES {
            return Err(SequenceError::InvalidSequence(
                "evidence IDs must be non-empty and at most 256 bytes".to_owned(),
            ));
        }
        if seen.insert(value) {
            result.push(value.to_owned());
        }
    }
    Ok(result)
}

fn optional_string(value: Option<&serde_json::Value>) -> Result<Option<String>, SequenceError> {
    match value {
        None => Ok(None),
        Some(serde_json::Value::String(value)) if !value.trim().is_empty() => {
            Ok(Some(value.trim().to_owned()))
        }
        Some(_) => Err(SequenceError::InvalidSequence(
            "expected a non-empty string configuration value".to_owned(),
        )),
    }
}

fn string_array(
    value: Option<&serde_json::Value>,
    allow_empty: bool,
) -> Result<Vec<String>, SequenceError> {
    let Some(serde_json::Value::Array(values)) = value else {
        return Err(SequenceError::InvalidSequence(
            "expected a string-array configuration value".to_owned(),
        ));
    };
    let result: Option<Vec<_>> = values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
        })
        .collect();
    let result = result.ok_or_else(|| {
        SequenceError::InvalidSequence("configuration arrays must contain strings".to_owned())
    })?;
    if !allow_empty && result.is_empty() {
        return Err(SequenceError::InvalidSequence(
            "completion criteria must not be empty".to_owned(),
        ));
    }
    Ok(result)
}

fn bounded_u32(
    value: Option<&serde_json::Value>,
    minimum: u32,
    maximum: u32,
    default: u32,
) -> Result<u32, SequenceError> {
    let Some(value) = value else {
        return Ok(default);
    };
    let number = value
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .filter(|number| (*number >= minimum) && (*number <= maximum))
        .ok_or_else(|| {
            SequenceError::InvalidSequence(format!(
                "numeric configuration must be in {minimum}..={maximum}"
            ))
        })?;
    Ok(number)
}

fn sort_unique(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}
