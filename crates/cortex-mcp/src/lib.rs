use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cortex_context::{ContextRequest, compile_context};
use cortex_domain::{GraphDocument, default_control_plane};
use cortex_router::{RoutingRequest, route};
use cortex_skills::{export_skill_markdown, import_skill_markdown};
use cortex_store::GraphStore;
use cortex_weavatrix::{
    RefactorOperation, WeavatrixAdapter, WeavatrixConfig, compile_evidence_bundle,
};
use mcport::{ConcurrentMcpServer, FlushPolicy, RuntimeConfig, ToolReply, TransportLimits, json};
use serde::Deserialize;
use serde_json::Value;

mod run_tools;

const DEFAULT_GRAPH_ID: &str = "default-control-plane";
const MAX_SKILL_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct CortexMcpState {
    store: GraphStore,
    weavatrix: WeavatrixAdapter,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphGetArgs {
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillCompileArgs {
    source: String,
    markdown: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillExportArgs {
    graph: GraphDocument,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphSaveArgs {
    graph: GraphDocument,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphListArgs {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeavatrixPrepareArgs {
    repository: PathBuf,
    task: String,
    symbol: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeavatrixContextArgs {
    repository: PathBuf,
    task: String,
    symbol: Option<String>,
    max_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefactorPreviewArgs {
    repository: PathBuf,
    operation: RefactorOperation,
    arguments: Value,
}

impl CortexMcpState {
    pub fn open(database: PathBuf) -> Result<Self, String> {
        if let Some(parent) = database.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let store = GraphStore::open(database).map_err(|error| error.to_string())?;
        store
            .seed_if_missing(&default_control_plane())
            .map_err(|error| error.to_string())?;
        let weavatrix =
            WeavatrixAdapter::new(WeavatrixConfig::discover().map_err(|error| error.to_string())?);
        Ok(Self { store, weavatrix })
    }
}

#[allow(clippy::too_many_lines)]
pub fn serve(state: CortexMcpState) -> io::Result<()> {
    let state = Arc::new(state);
    let graph_state = Arc::clone(&state);
    let graph_list_state = Arc::clone(&state);
    let graph_save_state = Arc::clone(&state);
    let prepare_state = Arc::clone(&state);
    let context_state = Arc::clone(&state);
    let refactor_state = Arc::clone(&state);

    let server = ConcurrentMcpServer::new("cortex-loom", env!("CARGO_PKG_VERSION"))
        .instructions(
            "Cortex Loom reduces repository context before Codex or Claude reasons about it. Use route_work first, then weavatrix_context_compile for revision-bound, budgeted evidence. Local-model results are advisory and must retain evidence IDs. High-risk or ambiguous work stays upstream. Refactor is preview-only: this server never applies a plan. Graphs are canonical in the local store; generated Markdown is only a view.",
        )
        .tool_page_size(16)
        .typed_tool(
            "context_compile",
            "Select bounded evidence deterministically and report omitted IDs and token savings.",
            json!({
                "type": "object",
                "properties": {
                    "items": {"type": "array", "maxItems": 4096},
                    "maxTokens": {"type": "integer", "minimum": 1}
                },
                "required": ["items", "maxTokens"],
                "additionalProperties": false
            }),
            move |context, arguments: ContextRequest| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match compile_context(&arguments) {
                    Ok(packet) => ToolReply::structured(packet),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "weavatrix_context_compile",
            "Build native Weavatrix evidence and compile it into one deterministic, budgeted context packet with stable citation IDs.",
            json!({
                "type": "object",
                "properties": {
                    "repository": {"type": "string", "maxLength": 4096},
                    "task": {"type": "string", "maxLength": 16384},
                    "symbol": {"type": "string", "maxLength": 4096},
                    "maxTokens": {"type": "integer", "minimum": 1, "maximum": 100_000}
                },
                "required": ["repository", "task", "maxTokens"],
                "additionalProperties": false
            }),
            move |context, arguments: WeavatrixContextArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let bundle = match context_state.weavatrix.prepare_context(
                    &arguments.repository,
                    &arguments.task,
                    arguments.symbol.as_deref(),
                ) {
                    Ok(bundle) => bundle,
                    Err(error) => return ToolReply::error(error.to_string()),
                };
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match compile_evidence_bundle(bundle, &arguments.task, arguments.max_tokens) {
                    Ok(packet) => ToolReply::structured(packet),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "graph_list",
            "List bounded metadata for canonical workflow graphs.",
            json!({
                "type": "object",
                "properties": {"limit": {"type": "integer", "minimum": 1, "maximum": 100}},
                "additionalProperties": false
            }),
            move |context, arguments: GraphListArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let limit = arguments.limit.unwrap_or(50).clamp(1, 100);
                match graph_list_state.store.list() {
                    Ok(graphs) => ToolReply::structured(
                        graphs
                            .iter()
                            .take(limit)
                            .map(graph_summary)
                            .collect::<Vec<_>>(),
                    ),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "graph_get",
            "Read one canonical editable workflow graph by id.",
            json!({
                "type": "object",
                "properties": {"id": {"type": "string"}},
                "additionalProperties": false
            }),
            move |context, arguments: GraphGetArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let id = arguments.id.as_deref().unwrap_or(DEFAULT_GRAPH_ID);
                match graph_state.store.get(id) {
                    Ok(Some(graph)) => ToolReply::structured(graph),
                    Ok(None) => ToolReply::error(format!("graph not found: {id}")),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "graph_save",
            "Create or revision-safely update one canonical workflow graph.",
            json!({
                "type": "object",
                "properties": {"graph": {"type": "object"}},
                "required": ["graph"],
                "additionalProperties": false
            }),
            move |context, arguments: GraphSaveArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match graph_save_state.store.save(&arguments.graph) {
                    Ok(graph) => ToolReply::structured(graph),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "skill_compile",
            "Compile readable SKILL.md Markdown into a typed graph without executing it.",
            json!({
                "type": "object",
                "properties": {
                    "source": {"type": "string", "maxLength": 1024},
                    "markdown": {"type": "string", "maxLength": MAX_SKILL_BYTES}
                },
                "required": ["source", "markdown"],
                "additionalProperties": false
            }),
            move |context, arguments: SkillCompileArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                if arguments.markdown.len() > MAX_SKILL_BYTES {
                    return ToolReply::error("skill Markdown exceeds the 2 MiB limit");
                }
                match import_skill_markdown(&arguments.source, &arguments.markdown) {
                    Ok(graph) => ToolReply::structured(graph),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "skill_export",
            "Render a cortex-skills graph as readable SKILL.md Markdown without executing it.",
            json!({
                "type": "object",
                "properties": {"graph": {"type": "object"}},
                "required": ["graph"],
                "additionalProperties": false
            }),
            move |context, arguments: SkillExportArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match export_skill_markdown(&arguments.graph) {
                    Ok(markdown) => ToolReply::structured(serde_json::json!({
                        "markdown": markdown
                    })),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "route_work",
            "Deterministically choose deterministic analysis, Weavatrix, bounded Ollama advice, or upstream execution. Never uses model self-confidence.",
            json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string"},
                    "evidence": {"type": "string", "enum": ["not_required", "verified", "missing", "contradictory"]},
                    "schemaValid": {"type": "boolean"},
                    "budget": {"type": "object"},
                    "mutation": {"type": "string", "enum": ["none", "approved", "approval_required"]},
                    "availability": {"type": "object"}
                },
                "required": ["task", "evidence", "schemaValid", "budget", "mutation", "availability"],
                "additionalProperties": false
            }),
            move |context, arguments: RoutingRequest| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                ToolReply::structured(route(&arguments))
            },
        )
        .typed_tool(
            "weavatrix_prepare",
            "Build or refresh the native Weavatrix graph and return bounded module, symbol, and verified-change planning evidence.",
            json!({
                "type": "object",
                "properties": {
                    "repository": {"type": "string"},
                    "task": {"type": "string"},
                    "symbol": {"type": "string"}
                },
                "required": ["repository", "task"],
                "additionalProperties": false
            }),
            move |context, arguments: WeavatrixPrepareArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match prepare_state.weavatrix.prepare_context(
                    &arguments.repository,
                    &arguments.task,
                    arguments.symbol.as_deref(),
                ) {
                    Ok(bundle) if context.is_cancelled() => {
                        let _ = bundle;
                        ToolReply::error("cancelled")
                    }
                    Ok(bundle) => ToolReply::structured(bundle),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "weavatrix_refactor_preview",
            "Request a bounded Weavatrix Refactor plan. Confirm tokens are stripped and apply_edit_plan is not exposed.",
            json!({
                "type": "object",
                "properties": {
                    "repository": {"type": "string"},
                    "operation": {"type": "string", "enum": ["rename_symbol", "rename_related_symbols", "move_file", "move_symbol", "change_signature", "edit_symbol"]},
                    "arguments": {"type": "object"}
                },
                "required": ["repository", "operation", "arguments"],
                "additionalProperties": false
            }),
            move |context, arguments: RefactorPreviewArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match refactor_state.weavatrix.preview_refactor(
                    &arguments.repository,
                    arguments.operation,
                    &arguments.arguments,
                ) {
                    Ok(plan) => ToolReply::structured(serde_json::json!({
                        "mode": "preview",
                        "plan": plan,
                        "applyAvailable": false
                    })),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        );
    let server = run_tools::register(server, Arc::clone(&state));
    server.serve(RuntimeConfig {
        transport: TransportLimits::new(4 * 1024 * 1024, 8 * 1024 * 1024),
        max_in_flight: 4,
        queue_depth: 16,
        output_queue_depth: 16,
        output_flush_policy: FlushPolicy::PerMessage,
        handler_deadline: Some(Duration::from_secs(120)),
    })
}

#[must_use]
pub fn graph_summary(graph: &GraphDocument) -> Value {
    serde_json::json!({
        "id": graph.id,
        "name": graph.name,
        "revision": graph.revision,
        "nodes": graph.nodes.len(),
        "edges": graph.edges.len()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_summary_is_bounded_metadata() {
        let summary = graph_summary(&default_control_plane());
        assert_eq!(summary.get("nodes").and_then(Value::as_u64), Some(8));
        assert!(summary.get("nodes").is_some());
    }
}
