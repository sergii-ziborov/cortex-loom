use super::{
    Arc, ConcurrentMcpServer, CortexMcpState, DEFAULT_GRAPH_ID, GraphGetArgs, GraphListArgs,
    GraphSaveArgs, MAX_SKILL_BYTES, SkillCompileArgs, SkillExportArgs, SkillIndexArgs,
    SkillReadArgs, ToolReply, export_skill_markdown, graph_summary, import_skill_markdown,
    index_entry, index_json, json, render_index,
};

#[allow(clippy::too_many_lines)]
pub(crate) fn register(
    server: ConcurrentMcpServer,
    state: &Arc<CortexMcpState>,
) -> ConcurrentMcpServer {
    let graph_state = Arc::clone(state);
    let graph_list_state = Arc::clone(state);
    let graph_save_state = Arc::clone(state);
    let index_state = Arc::clone(state);
    let read_state = Arc::clone(state);
    server
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
                "properties": {"graph": {"type": "object", "description": "A whole cortex-loom.graph.v1 document. Read one with graph_get and send it back edited; the nested node and edge shapes are documented there.", "properties": {"schemaVersion": {"type": "string"}, "id": {"type": "string"}, "name": {"type": "string"}, "revision": {"type": "integer", "minimum": 0}, "nodes": {"type": "array", "items": {"type": "object"}}, "edges": {"type": "array", "items": {"type": "object"}}, "metadata": {"type": "object", "additionalProperties": {"type": "string"}}}, "required": ["schemaVersion", "id", "name", "revision", "nodes", "edges"]}},
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
                "properties": {"graph": {"type": "object", "description": "A whole cortex-loom.graph.v1 document. Read one with graph_get and send it back edited; the nested node and edge shapes are documented there.", "properties": {"schemaVersion": {"type": "string"}, "id": {"type": "string"}, "name": {"type": "string"}, "revision": {"type": "integer", "minimum": 0}, "nodes": {"type": "array", "items": {"type": "object"}}, "edges": {"type": "array", "items": {"type": "object"}}, "metadata": {"type": "object", "additionalProperties": {"type": "string"}}}, "required": ["schemaVersion", "id", "name", "revision", "nodes", "edges"]}},
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
            "skill_index",
            "List every stored workflow as one line each: id, when it applies, and its size. Keep this loaded; fetch a workflow body with `skill_read` only once a task matches one.",
            json!({
                "type": "object",
                "properties": {"format": {"enum": ["markdown", "structured"]}},
                "additionalProperties": false
            }),
            move |context, arguments: SkillIndexArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match index_state.store.list() {
                    Ok(graphs) => {
                        let entries: Vec<_> = graphs.iter().filter_map(index_entry).collect();
                        if arguments.format.as_deref() == Some("structured") {
                            return ToolReply::structured(
                                entries.iter().map(index_json).collect::<Vec<_>>(),
                            );
                        }
                        ToolReply::structured(serde_json::json!({
                            "markdown": render_index(&entries),
                            "count": entries.len(),
                        }))
                    }
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "skill_read",
            "Fetch one workflow's SKILL.md by id, after `skill_index` showed it applies. Read one, not several: an unread workflow costs nothing and a loaded one costs every turn.",
            json!({
                "type": "object",
                "properties": {"id": {"type": "string", "maxLength": 256}},
                "required": ["id"],
                "additionalProperties": false
            }),
            move |context, arguments: SkillReadArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match read_state.store.get(&arguments.id) {
                    Ok(Some(graph)) => match export_skill_markdown(&graph) {
                        Ok(markdown) => ToolReply::structured(serde_json::json!({
                            "id": graph.id,
                            "name": graph.name,
                            "markdown": markdown,
                        })),
                        Err(error) => ToolReply::error(error.to_string()),
                    },
                    Ok(None) => ToolReply::error(format!("workflow not found: {}", arguments.id)),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
}
