use std::sync::Arc;

use cortex_domain::GraphDocument;
use cortex_sequences::{
    SequenceCandidate, active_step_packet, candidate_templates, instantiate_template,
    lint_sequence, templates,
};
use mcport::{ConcurrentMcpServer, ToolReply, json};
use serde::Deserialize;

use crate::CortexMcpState;

#[derive(Debug, Deserialize)]
struct SequenceListArgs {}

#[derive(Debug, Deserialize)]
struct SequenceRecommendArgs {
    task: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SequenceCopyArgs {
    template_id: String,
    graph_id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SequenceLintArgs {
    graph: GraphDocument,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SequenceStepArgs {
    graph_id: String,
    node_id: String,
    #[serde(default)]
    evidence_ids: Vec<String>,
}

pub(super) fn register(
    server: ConcurrentMcpServer,
    state: &Arc<CortexMcpState>,
) -> ConcurrentMcpServer {
    let server = register_discovery(server, state);
    register_editing(server, state)
}

fn register_discovery(
    server: ConcurrentMcpServer,
    state: &Arc<CortexMcpState>,
) -> ConcurrentMcpServer {
    let recommend_state = Arc::clone(state);
    server
        .typed_tool(
            "sequence_list",
            "List the seven immutable Cortex sequence templates and their deterministic activation hints. Copy one before editing it.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            move |context, _: SequenceListArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                ToolReply::text(template_list())
            },
        )
        .typed_tool(
            "sequence_recommend",
            "Recommend at most three Cortex sequence templates using deterministic declared hints. An optional gated embedding may only reorder those candidates; this never changes executor or mutation authority.",
            json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string", "minLength": 1, "maxLength": 16384}
                },
                "required": ["task"],
                "additionalProperties": false
            }),
            move |context, arguments: SequenceRecommendArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                if arguments.task.trim().is_empty() {
                    return ToolReply::error("task must not be empty");
                }
                ToolReply::text(recommendations(&recommend_state, &arguments.task))
            },
        )
}

fn register_editing(
    server: ConcurrentMcpServer,
    state: &Arc<CortexMcpState>,
) -> ConcurrentMcpServer {
    let copy_state = Arc::clone(state);
    let step_state = Arc::clone(state);
    server
        .typed_tool(
            "sequence_copy",
            "Create one editable graph from an immutable Cortex sequence template. An existing graph is returned unchanged, so this never overwrites user edits.",
            json!({
                "type": "object",
                "properties": {
                    "templateId": {"type": "string", "minLength": 1, "maxLength": 128},
                    "graphId": {"type": "string", "minLength": 1, "maxLength": 256},
                    "name": {"type": "string", "minLength": 1, "maxLength": 512}
                },
                "required": ["templateId", "graphId", "name"],
                "additionalProperties": false
            }),
            move |context, arguments: SequenceCopyArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match copy_sequence(&copy_state, &arguments) {
                    Ok(value) => ToolReply::text(value),
                    Err(error) => ToolReply::error(error),
                }
            },
        )
        .typed_tool(
            "sequence_lint",
            "Validate a draft sequence for reachability, bounded retries, evidence gates, local-model authority, completion criteria, and terminal paths. Saving a draft remains separate from running it.",
            json!({
                "type": "object",
                "properties": {
                    "graph": graph_schema()
                },
                "required": ["graph"],
                "additionalProperties": false
            }),
            move |context, arguments: SequenceLintArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                ToolReply::text(lint_sequence(&arguments.graph))
            },
        )
        .typed_tool(
            "sequence_step_read",
            "Compile only the selected active step from a saved editable sequence into a bounded packet. Inactive step instructions are not returned.",
            json!({
                "type": "object",
                "properties": {
                    "graphId": {"type": "string", "minLength": 1, "maxLength": 256},
                    "nodeId": {"type": "string", "minLength": 1, "maxLength": 256},
                    "evidenceIds": {
                        "type": "array",
                        "maxItems": 64,
                        "items": {"type": "string", "minLength": 1, "maxLength": 256}
                    }
                },
                "required": ["graphId", "nodeId"],
                "additionalProperties": false
            }),
            move |context, arguments: SequenceStepArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let graph = match step_state.store.get(&arguments.graph_id) {
                    Ok(Some(graph)) => graph,
                    Ok(None) => {
                        return ToolReply::error(format!(
                            "graph not found: {}",
                            arguments.graph_id
                        ));
                    }
                    Err(error) => return ToolReply::error(error.to_string()),
                };
                match active_step_packet(&graph, &arguments.node_id, &arguments.evidence_ids) {
                    Ok(packet) => ToolReply::text(packet),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
}

fn recommendations(state: &CortexMcpState, task: &str) -> serde_json::Value {
    let mut candidates = candidate_templates(task);
    let mut semantic_ranking = None;
    let mut warnings = Vec::new();
    if let Some(scorer) = &state.semantic {
        let texts: Vec<(String, String)> = candidates
            .iter()
            .map(|candidate| {
                let description = templates()
                    .iter()
                    .find(|template| template.id == candidate.template_id)
                    .map_or("", |template| template.description);
                (
                    candidate.template_id.clone(),
                    format!("{description} {}", candidate.matched_hints.join(" ")),
                )
            })
            .collect();
        let fragments: Vec<cortex_context::ranking::EvidenceLink<'_>> = texts
            .iter()
            .map(|(id, content)| cortex_context::ranking::EvidenceLink {
                id,
                source: id,
                content,
            })
            .collect();
        match scorer.score(task, &fragments, None) {
            Ok(scores) => {
                rerank_candidates(&mut candidates, &scores);
                semantic_ranking = Some(scorer.provenance());
            }
            Err(error) => warnings.push(format!(
                "semantic candidate ordering unavailable: {error}; deterministic order used"
            )),
        }
    }
    serde_json::json!({
        "candidates": candidates,
        "semanticRanking": semantic_ranking,
        "warnings": warnings,
        "authority": "recommendation_only",
        "maxCandidates": 3,
    })
}

fn rerank_candidates(
    candidates: &mut [SequenceCandidate],
    scores: &std::collections::HashMap<String, f64>,
) {
    candidates.sort_by(|left, right| {
        scores
            .get(&right.template_id)
            .copied()
            .unwrap_or_default()
            .total_cmp(&scores.get(&left.template_id).copied().unwrap_or_default())
            .then_with(|| right.deterministic_score.cmp(&left.deterministic_score))
            .then_with(|| left.template_id.cmp(&right.template_id))
    });
}

fn template_list() -> Vec<serde_json::Value> {
    templates()
        .iter()
        .map(|template| {
            serde_json::json!({
                "id": template.id,
                "version": template.version,
                "title": template.title,
                "description": template.description,
                "changelog": template.changelog,
                "activation": template.activation,
            })
        })
        .collect()
}

fn copy_sequence(
    state: &CortexMcpState,
    arguments: &SequenceCopyArgs,
) -> Result<serde_json::Value, String> {
    let graph = instantiate_template(&arguments.template_id, &arguments.graph_id, &arguments.name)
        .map_err(|error| error.to_string())?;
    let created = state
        .store
        .get(&graph.id)
        .map_err(|error| error.to_string())?
        .is_none();
    let graph = state
        .store
        .seed_if_missing(&graph)
        .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({"created": created, "graph": graph}))
}

fn graph_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "schemaVersion": {"type": "string"},
            "id": {"type": "string"},
            "name": {"type": "string"},
            "revision": {"type": "integer", "minimum": 0},
            "nodes": {"type": "array", "items": {"type": "object"}},
            "edges": {"type": "array", "items": {"type": "object"}},
            "metadata": {"type": "object", "additionalProperties": {"type": "string"}}
        },
        "required": ["schemaVersion", "id", "name", "revision", "nodes", "edges"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn list_is_bounded_to_the_seven_cortex_templates() {
        let list = template_list();
        assert_eq!(list.len(), 7);
        assert_eq!(list[0]["id"], "discover-and-plan");
        assert!(list.iter().all(|item| item.get("activation").is_some()));
    }

    #[test]
    fn lint_schema_exposes_the_graph_shape() {
        let schema = graph_schema();
        assert_eq!(schema["properties"]["nodes"]["type"], "array");
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn semantic_order_cannot_change_the_deterministic_candidate_set() {
        let mut candidates = candidate_templates("plan and review an API change");
        let before: std::collections::BTreeSet<_> = candidates
            .iter()
            .map(|candidate| candidate.template_id.clone())
            .collect();
        let scores = HashMap::from([
            ("review-and-correct".to_owned(), 0.9),
            ("discover-and-plan".to_owned(), 0.1),
            ("invented-fourth".to_owned(), 1.0),
        ]);
        rerank_candidates(&mut candidates, &scores);
        let after: std::collections::BTreeSet<_> = candidates
            .iter()
            .map(|candidate| candidate.template_id.clone())
            .collect();
        assert_eq!(after, before);
        assert!(candidates.len() <= 3);
        assert_eq!(candidates[0].template_id, "review-and-correct");
    }
}
