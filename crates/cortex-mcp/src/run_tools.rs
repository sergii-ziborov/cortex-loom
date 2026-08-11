use std::sync::Arc;

use cortex_run::{NodeRunStatus, RunCommand, RunDocument};
use mcport::{ConcurrentMcpServer, ToolReply, json};
use serde::Deserialize;

use crate::CortexMcpState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunCreateArgs {
    id: String,
    graph_id: String,
    graph_revision: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunGetArgs {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunListArgs {
    graph_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunApplyArgs {
    id: String,
    command: RunCommand,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunEventsArgs {
    id: String,
    after: Option<u64>,
    limit: Option<usize>,
}

#[allow(clippy::too_many_lines)]
pub(super) fn register(
    server: ConcurrentMcpServer,
    state: Arc<CortexMcpState>,
) -> ConcurrentMcpServer {
    let create_state = Arc::clone(&state);
    let get_state = Arc::clone(&state);
    let list_state = Arc::clone(&state);
    let apply_state = Arc::clone(&state);
    let events_state = Arc::clone(&state);
    server
        .typed_tool(
            "run_create",
            "Create a durable executable run from an exact saved graph revision.",
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "minLength": 1, "maxLength": 256},
                    "graphId": {"type": "string", "minLength": 1},
                    "graphRevision": {"type": "integer", "minimum": 0}
                },
                "required": ["id", "graphId", "graphRevision"],
                "additionalProperties": false
            }),
            move |context, arguments: RunCreateArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let graph = match create_state.store.get(&arguments.graph_id) {
                    Ok(Some(graph)) => graph,
                    Ok(None) => {
                        return ToolReply::error(format!(
                            "graph not found: {}",
                            arguments.graph_id
                        ));
                    }
                    Err(error) => return ToolReply::error(error.to_string()),
                };
                if graph.revision != arguments.graph_revision {
                    return ToolReply::error(format!(
                        "graph {} revision conflict: expected {}, current {}",
                        graph.id, arguments.graph_revision, graph.revision
                    ));
                }
                match create_state.store.runs().create(&arguments.id, &graph) {
                    Ok(run) => ToolReply::text(run),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "run_get",
            "Read one durable run together with its immutable graph snapshot.",
            json!({
                "type": "object",
                "properties": {"id": {"type": "string", "minLength": 1}},
                "required": ["id"],
                "additionalProperties": false
            }),
            move |context, arguments: RunGetArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match get_state.store.runs().get(&arguments.id) {
                    Ok(Some(run)) => match get_state.store.runs().get_graph(&arguments.id) {
                        Ok(Some(graph)) => ToolReply::text(serde_json::json!({
                            "run": run,
                            "graph": graph
                        })),
                        Ok(None) => {
                            ToolReply::error(format!("run not found: {}", arguments.id))
                        }
                        Err(error) => ToolReply::error(error.to_string()),
                    },
                    Ok(None) => ToolReply::error(format!("run not found: {}", arguments.id)),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "run_list",
            "List bounded run summaries, optionally filtered by graph id.",
            json!({
                "type": "object",
                "properties": {
                    "graphId": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100}
                },
                "additionalProperties": false
            }),
            move |context, arguments: RunListArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match list_state.store.runs().list(
                    arguments.graph_id.as_deref(),
                    arguments.limit.unwrap_or(50),
                ) {
                    Ok(runs) => ToolReply::text(
                        runs.iter().map(run_summary).collect::<Vec<_>>(),
                    ),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "run_apply",
            "Apply one revision-checked start, evidence submission, completion, human decision, bounded retry, or cancellation. This records state only; external execution authority remains with the selected agent or human.",
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "minLength": 1},
                    "command": {
                        "oneOf": [
                            {
                                "type": "object",
                                "properties": {
                                    "action": {"const": "start_node"},
                                    "expectedRevision": {"type": "integer", "minimum": 1},
                                    "nodeId": {"type": "string", "minLength": 1}
                                },
                                "required": ["action", "expectedRevision", "nodeId"],
                                "additionalProperties": false
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "action": {"const": "submit_evidence"},
                                    "expectedRevision": {"type": "integer", "minimum": 1},
                                    "nodeId": {"type": "string", "minLength": 1},
                                    "evidenceId": {"type": "string", "minLength": 1, "maxLength": 1024},
                                    "submittedBy": {"type": "string", "minLength": 1, "maxLength": 2048},
                                    "source": {"type": "string", "minLength": 1, "maxLength": 2048},
                                    "locator": {"type": "string", "minLength": 1, "maxLength": 2048},
                                    "digest": {"type": ["string", "null"], "maxLength": 2048},
                                    "summary": {"type": "string", "minLength": 1, "maxLength": 8192}
                                },
                                "required": [
                                    "action", "expectedRevision", "nodeId", "evidenceId",
                                    "submittedBy", "source", "locator", "summary"
                                ],
                                "additionalProperties": false
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "action": {"const": "complete_node"},
                                    "expectedRevision": {"type": "integer", "minimum": 1},
                                    "nodeId": {"type": "string", "minLength": 1},
                                    "outcome": {"type": "string", "enum": ["succeeded", "failed"]},
                                    "selectedEdgeIds": {
                                        "type": "array",
                                        "items": {"type": "string", "minLength": 1},
                                        "maxItems": 4096
                                    },
                                    "evidenceIds": {
                                        "type": "array",
                                        "items": {"type": "string", "minLength": 1, "maxLength": 1024},
                                        "maxItems": 256
                                    },
                                    "detail": {"type": ["string", "null"], "maxLength": 16384}
                                },
                                "required": ["action", "expectedRevision", "nodeId", "outcome"],
                                "additionalProperties": false
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "action": {"const": "decide_human_gate"},
                                    "expectedRevision": {"type": "integer", "minimum": 1},
                                    "nodeId": {"type": "string", "minLength": 1},
                                    "decision": {"type": "string", "enum": ["approved", "rejected"]},
                                    "actor": {"type": "string", "minLength": 1, "maxLength": 16384},
                                    "reason": {"type": "string", "minLength": 1, "maxLength": 16384},
                                    "selectedEdgeIds": {
                                        "type": "array",
                                        "items": {"type": "string", "minLength": 1},
                                        "maxItems": 4096
                                    },
                                    "evidenceIds": {
                                        "type": "array",
                                        "items": {"type": "string", "minLength": 1, "maxLength": 1024},
                                        "maxItems": 256
                                    }
                                },
                                "required": [
                                    "action", "expectedRevision", "nodeId", "decision",
                                    "actor", "reason"
                                ],
                                "additionalProperties": false
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "action": {"const": "trigger_retry"},
                                    "expectedRevision": {"type": "integer", "minimum": 1},
                                    "retryNodeId": {"type": "string", "minLength": 1},
                                    "reason": {"type": "string", "minLength": 1, "maxLength": 16384}
                                },
                                "required": ["action", "expectedRevision", "retryNodeId", "reason"],
                                "additionalProperties": false
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "action": {"const": "cancel"},
                                    "expectedRevision": {"type": "integer", "minimum": 1},
                                    "reason": {"type": "string", "minLength": 1, "maxLength": 16384}
                                },
                                "required": ["action", "expectedRevision", "reason"],
                                "additionalProperties": false
                            }
                        ]
                    }
                },
                "required": ["id", "command"],
                "additionalProperties": false
            }),
            move |context, arguments: RunApplyArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match apply_state
                    .store
                    .runs()
                    .apply(&arguments.id, &arguments.command)
                {
                    Ok(run) => ToolReply::text(run),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "run_events",
            "Read bounded append-only transition events for one run.",
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "minLength": 1},
                    "after": {"type": "integer", "minimum": 0},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500}
                },
                "required": ["id"],
                "additionalProperties": false
            }),
            move |context, arguments: RunEventsArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match events_state.store.runs().events(
                    &arguments.id,
                    arguments.after.unwrap_or(0),
                    arguments.limit.unwrap_or(100),
                ) {
                    Ok(events) => ToolReply::text(events),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "run_replay_verify",
            "Reconstruct a run from its immutable graph snapshot and ordered command events, then compare it with the durable snapshot without changing either.",
            json!({
                "type": "object",
                "properties": {"id": {"type": "string", "minLength": 1}},
                "required": ["id"],
                "additionalProperties": false
            }),
            move |context, arguments: RunGetArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match state.store.runs().verify_replay(&arguments.id) {
                    Ok(verification) => ToolReply::text(verification),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
}

fn run_summary(run: &RunDocument) -> serde_json::Value {
    serde_json::json!({
        "id": run.id,
        "graphId": run.graph_id,
        "graphRevision": run.graph_revision,
        "revision": run.revision,
        "status": run.status,
        "updatedAt": run.updated_at,
        "ready": run.nodes.iter().filter(|node| node.status == NodeRunStatus::Ready).count(),
        "running": run.nodes.iter().filter(|node| node.status == NodeRunStatus::Running).count()
    })
}
