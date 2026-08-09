use std::sync::Arc;

use cortex_domain::GraphDocument;
use cortex_sequences::{active_step_packet, instantiate_template, lint_sequence, templates};
use mcport::{ConcurrentMcpServer, ToolReply, json};
use serde::Deserialize;

use crate::CortexMcpState;

#[derive(Debug, Deserialize)]
struct SequenceListArgs {}

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
    let copy_state = Arc::clone(state);
    let step_state = Arc::clone(state);
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
                ToolReply::structured(template_list())
            },
        )
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
                    Ok(value) => ToolReply::structured(value),
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
                ToolReply::structured(lint_sequence(&arguments.graph))
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
                    Ok(packet) => ToolReply::structured(packet),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
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
}
