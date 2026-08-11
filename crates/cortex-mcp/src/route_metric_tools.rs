use super::{
    Arc, ConcurrentMcpServer, CortexMcpState, RouteWorkArgs, RoutingSnapshot, ShadowHandle,
    ShadowMetricsArgs, ShadowOperation, ShadowTask, ToolReply, UsageOperation, UsageReadArgs,
    UsageReport, UsageReportArgs, UsageSample, json, llm_route, record_usage, to_snake,
};
use super::{RoutingDecision, RoutingRequest};

#[allow(clippy::too_many_lines)]
pub(crate) fn register(
    server: ConcurrentMcpServer,
    state: &Arc<CortexMcpState>,
    lexical_route: fn(&RoutingRequest) -> RoutingDecision,
) -> ConcurrentMcpServer {
    let route_state = Arc::clone(state);
    let shadow_state = Arc::clone(state);
    let usage_state = Arc::clone(state);
    let report_state = Arc::clone(state);
    server
        .typed_tool(
            "route_work",
            "Choose deterministic analysis, Weavatrix, bounded Ollama advice, or upstream execution. Lexical rules are the floor; when CORTEX_LLM=1 and a gated classifier is up, the model may only escalate. Failures and under-calls keep the lexical decision.",
            json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string"},
                    "evidence": {"type": "string", "enum": ["not_required", "verified", "missing", "contradictory"]},
                    "schemaValid": {"type": "boolean"},
                    // Declared in full. A bare {"type": "object"} advertised a
                    // contract this tool does not accept: deserialization
                    // requires every field below, so a caller had to discover
                    // `weavatrix` by being rejected for it. Guess-and-retry is
                    // exactly the token waste this server exists to remove.
                    "budget": {
                        "type": "object",
                        "properties": {
                            "estimatedInputTokens": {"type": "integer", "minimum": 0},
                            "estimatedOutputTokens": {"type": "integer", "minimum": 0},
                            "maxInputTokens": {"type": "integer", "minimum": 1},
                            "maxOutputTokens": {"type": "integer", "minimum": 1}
                        },
                        "required": [
                            "estimatedInputTokens",
                            "estimatedOutputTokens",
                            "maxInputTokens",
                            "maxOutputTokens"
                        ],
                        "additionalProperties": false
                    },
                    "mutation": {"type": "string", "enum": ["none", "approved", "approval_required"]},
                    "availability": {
                        "type": "object",
                        "properties": {
                            "weavatrix": {"type": "boolean"},
                            "ollama": {"type": "boolean"}
                        },
                        "required": ["weavatrix", "ollama"],
                        "additionalProperties": false
                    },
                    "runId": {"type": "string", "maxLength": 256}
                },
                "required": ["task", "evidence", "schemaValid", "budget", "mutation", "availability"],
                "additionalProperties": false
            }),
            move |context, arguments: RouteWorkArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let routed = route_state.llm_router.as_ref().map_or_else(
                    || llm_route::RoutedWork {
                        decision: lexical_route(&arguments.request),
                        latency_ms: None,
                        classifier_profile: None,
                    },
                    |router| router.decide(&arguments.request),
                );
                let decision = &routed.decision;
                if let Some(shadow) = &route_state.shadow {
                    shadow.observe(ShadowTask::RouteClassification {
                        task: arguments.request.task.clone(),
                        deterministic: RoutingSnapshot {
                            tier: decision.model_tier,
                            class: decision.class,
                            risk: decision.risk,
                        },
                    });
                }
                record_usage(
                    &route_state.store,
                    &UsageSample {
                        operation: UsageOperation::RouteWork,
                        run_id: arguments.run_id.clone(),
                        target: to_snake(&decision.target),
                        model_tier: to_snake(&decision.model_tier),
                        task_class: to_snake(&decision.class),
                        budget_tokens: None,
                        raw_tokens: None,
                        selected_tokens: None,
                        omitted_tokens: None,
                        requires_upstream: None,
                        latency_ms: routed.latency_ms,
                    },
                );
                ToolReply::text(decision)
            },
        )
        .typed_tool(
            "shadow_metrics_read",
            "Read append-only shadow observation aggregates and recent samples. Shadow output never influences routing or compilation.",
            json!({
                "type": "object",
                "properties": {
                    "operation": {"type": "string", "enum": ["route_classification", "context_compression"]},
                    "model": {"type": "string", "maxLength": 256},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100}
                },
                "additionalProperties": false
            }),
            move |context, arguments: ShadowMetricsArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let operation = match arguments.operation.as_deref() {
                    None => None,
                    Some(value) => match ShadowOperation::parse(value) {
                        Some(operation) => Some(operation),
                        None => {
                            return ToolReply::error(format!("unknown operation: {value}"));
                        }
                    },
                };
                let shadow_store = shadow_state.store.shadow();
                let aggregates =
                    match shadow_store.aggregate(operation, arguments.model.as_deref()) {
                        Ok(aggregates) => aggregates,
                        Err(error) => return ToolReply::error(error.to_string()),
                    };
                let samples = match arguments.limit {
                    None => Vec::new(),
                    Some(limit) => match shadow_store.list(
                        operation,
                        arguments.model.as_deref(),
                        limit.clamp(1, 100),
                    ) {
                        Ok(samples) => samples,
                        Err(error) => return ToolReply::error(error.to_string()),
                    },
                };
                let handle = shadow_state.shadow.as_deref();
                ToolReply::text(serde_json::json!({
                    "enabled": handle.is_some(),
                    "smallModel": handle.and_then(ShadowHandle::small_model),
                    "mediumModel": handle.and_then(ShadowHandle::medium_model),
                    "droppedSamples": handle.map_or(0, ShadowHandle::dropped),
                    "oversizeSkipped": handle.map_or(0, ShadowHandle::oversize_skipped),
                    "aggregates": aggregates,
                    "samples": samples,
                }))
            },
        )
        .typed_tool(
            "usage_read",
            "Read the append-only token-accounting ledger: routing decisions and context-compilation savings. Bounded and read-only.",
            json!({
                "type": "object",
                "properties": {
                    "operation": {"type": "string", "enum": ["route_work", "context_compile"]},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100}
                },
                "additionalProperties": false
            }),
            move |context, arguments: UsageReadArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let operation = match arguments.operation.as_deref() {
                    None => None,
                    Some(value) => match UsageOperation::parse(value) {
                        Some(operation) => Some(operation),
                        None => return ToolReply::error(format!("unknown operation: {value}")),
                    },
                };
                let usage = usage_state.store.usage();
                let summary = match usage.summary() {
                    Ok(summary) => summary,
                    Err(error) => return ToolReply::error(error.to_string()),
                };
                let quality = match usage.quality_summary() {
                    Ok(quality) => quality,
                    Err(error) => return ToolReply::error(error.to_string()),
                };
                let samples = match arguments.limit {
                    None => Vec::new(),
                    Some(limit) => match usage.list(operation, limit.clamp(1, 100)) {
                        Ok(samples) => samples,
                        Err(error) => return ToolReply::error(error.to_string()),
                    },
                };
                ToolReply::text(serde_json::json!({
                    "summary": summary,
                    "quality": quality,
                    "samples": samples,
                }))
            },
        )
        .typed_tool(
            "usage_report",
            "Self-report upstream token consumption into the append-only ledger, optionally attributed to a run. Closes the token balance; it is honest self-reporting, not verification.",
            json!({
                "type": "object",
                "properties": {
                    "runId": {"type": "string", "maxLength": 256},
                    "agent": {"type": "string", "maxLength": 256},
                    "inputTokens": {"type": "integer", "minimum": 0, "maximum": 100_000_000},
                    "outputTokens": {"type": "integer", "minimum": 0, "maximum": 100_000_000},
                    "note": {"type": "string", "maxLength": 2048}
                },
                "required": ["agent", "inputTokens", "outputTokens"],
                "additionalProperties": false
            }),
            move |context, arguments: UsageReportArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                if arguments.agent.trim().is_empty() {
                    return ToolReply::error("agent must not be empty");
                }
                let report = UsageReport {
                    run_id: arguments.run_id,
                    agent: arguments.agent,
                    input_tokens: arguments.input_tokens,
                    output_tokens: arguments.output_tokens,
                    note: arguments.note,
                };
                match report_state.store.usage().insert_report(&report) {
                    Ok(id) => ToolReply::text(serde_json::json!({"recorded": true, "id": id})),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
}
