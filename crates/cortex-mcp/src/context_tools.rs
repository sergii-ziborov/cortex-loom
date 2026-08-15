use super::{
    Arc, ConcurrentMcpServer, ContextRequest, CortexMcpState, ToolReply, WeavatrixContextArgs,
    compile_context, json,
};
use crate::compile_session::{CompileArgs, compile_weavatrix};

pub(crate) fn register(
    server: ConcurrentMcpServer,
    state: &Arc<CortexMcpState>,
) -> ConcurrentMcpServer {
    let context_state = Arc::clone(state);
    server
        .typed_tool(
            "context_compile",
            "Select bounded evidence deterministically and report omitted IDs and token savings.",
            json!({
                "type": "object",
                "properties": {
                    "items": {
                    "type": "array",
                    "maxItems": 4096,
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string", "description": "Stable citation ID; keep it in derived output."},
                            "source": {"type": "string", "description": "Where the evidence came from, e.g. src/lib.rs:120."},
                            "content": {"type": "string"},
                            "priority": {"type": "string", "enum": ["critical", "high", "normal", "low"]},
                            "state": {"type": "string", "enum": ["verified", "unverified", "contradictory"]},
                            "relevance": {"type": "number", "description": "Optional score; reorders only within a priority band."}
                        },
                        "required": ["id", "source", "content", "priority", "state"],
                        "additionalProperties": false
                    }
                },
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
                    Ok(packet) => ToolReply::text(packet),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "weavatrix_context_compile",
            "Plan Weavatrix operations from the task, then compile a cortex-source packet: complete named definitions, bounded read_source on search hits and callee files, one sufficiency retry, then a budgeted compile with stable citation IDs. Name the symbols, files, and constants you care about in `task`. Leave `targeted` unset: `targeted=false` is a retired fixed operation set that is worse on tokens and recall.",
            json!({
                "type": "object",
                "properties": {
                    "repository": {"type": "string", "maxLength": 4096},
                    "task": {"type": "string", "maxLength": 16384},
                    "symbol": {"type": "string", "maxLength": 4096},
                    "maxTokens": {"type": "integer", "minimum": 1, "maximum": 100_000},
                    "runId": {
                        "type": "string",
                        "maxLength": 256,
                        "description": "Completed prior run to attribute usage and load high-signal memory from. Absent or Running loads no prior memory."
                    },
                    "skillId": {"type": "string", "maxLength": 256, "description": "Optional active skill graph whose context-intent, source-followup, and skip-change-plan frontmatter guide evidence gathering."},
                    "targeted": {"type": "boolean", "default": true}
                },
                "required": ["repository", "task", "maxTokens"],
                "additionalProperties": false
            }),
            move |context, arguments: WeavatrixContextArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match compile_weavatrix(
                    &context_state,
                    &CompileArgs {
                        repository: arguments.repository,
                        task: arguments.task,
                        symbol: arguments.symbol,
                        max_tokens: arguments.max_tokens,
                        run_id: arguments.run_id,
                        skill_id: arguments.skill_id,
                        targeted: arguments.targeted,
                        hints: None,
                    },
                ) {
                    Ok(packet) => ToolReply::text(packet),
                    Err(error) => ToolReply::error(error),
                }
            },
        )
}
