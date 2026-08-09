use super::{
    AdapterExportArgs, Arc, ConcurrentMcpServer, CortexMcpState, DEFAULT_GRAPH_ID,
    RefactorPreviewArgs, ToolReply, WeavatrixPrepareArgs, export_adapter, export_library_adapter,
    index_entry, json, preview_refactor_response, refactor_preview_schema,
};

#[allow(clippy::too_many_lines)]
pub(crate) fn register(
    server: ConcurrentMcpServer,
    state: &Arc<CortexMcpState>,
) -> ConcurrentMcpServer {
    let adapter_state = Arc::clone(state);
    let prepare_state = Arc::clone(state);
    let refactor_state = Arc::clone(state);
    server
        .typed_tool(
            "adapter_export",
            "Render vendor wiring (Claude Code, Codex, or Copilot) from one canonical graph: skill instructions plus MCP registration. Preview-only: returns file contents and never writes.",
            json!({
                "type": "object",
                "properties": {
                    "graphId": {"type": "string", "maxLength": 256},
                    "agent": {"type": "string", "enum": ["claude_code", "codex", "copilot"]},
                    "scope": {"type": "string", "enum": ["graph", "library"], "default": "graph"},
                    "launch": {
                        "type": "object",
                        "properties": {
                            "command": {"type": "string", "maxLength": 1024},
                            "args": {"type": "array", "items": {"type": "string", "maxLength": 1024}, "maxItems": 32}
                        },
                        "required": ["command", "args"],
                        "additionalProperties": false
                    }
                },
                "required": ["agent"],
                "additionalProperties": false
            }),
            move |context, arguments: AdapterExportArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let launch = arguments.launch.unwrap_or_default();
                if arguments.scope.as_deref() == Some("library") {
                    let graphs = match adapter_state.store.list() {
                        Ok(graphs) => graphs,
                        Err(error) => return ToolReply::error(error.to_string()),
                    };
                    // Only compiled workflows: a control-plane graph has no
                    // SKILL.md view and does not belong in a catalogue.
                    let skills: Vec<_> = graphs
                        .into_iter()
                        .filter(|graph| index_entry(graph).is_some())
                        .collect();
                    return match export_library_adapter(&skills, arguments.agent, &launch) {
                        Ok(bundle) => ToolReply::structured(bundle),
                        Err(error) => ToolReply::error(error.to_string()),
                    };
                }
                let id = arguments.graph_id.as_deref().unwrap_or(DEFAULT_GRAPH_ID);
                let graph = match adapter_state.store.get(id) {
                    Ok(Some(graph)) => graph,
                    Ok(None) => return ToolReply::error(format!("graph not found: {id}")),
                    Err(error) => return ToolReply::error(error.to_string()),
                };
                match export_adapter(&graph, arguments.agent, &launch) {
                    Ok(bundle) => ToolReply::structured(bundle),
                    Err(error) => ToolReply::error(error.to_string()),
                }
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
            "Validate and render an upstream-authored weavatrix.refactor-plan.v1 in native Rust. Breaking migration: pass {repository, plan}; operation/arguments are no longer accepted. Preview-only: no apply, confirmation token, or rollback authority exists.",
            refactor_preview_schema(),
            move |context, arguments: RefactorPreviewArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match preview_refactor_response(
                    &refactor_state.weavatrix,
                    &arguments.repository,
                    &arguments.plan,
                ) {
                    Ok(response) => ToolReply::structured(response),
                    Err(error) => ToolReply::error(error),
                }
            },
        )
}
