use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cortex_adapters::{AgentKind, McpLaunch, export_adapter};
use cortex_context::{ContextRequest, compile_context};
use cortex_domain::{GraphDocument, default_control_plane};
use cortex_router::{RoutingRequest, route};
use cortex_shadow::{
    CompressionSnapshot, RoutingSnapshot, ShadowConfig, ShadowEvidence, ShadowHandle, ShadowTask,
};
use cortex_skills::{export_skill_markdown, import_skill_markdown};
use cortex_store::{GraphStore, ShadowOperation, UsageOperation, UsageReport, UsageSample};
use cortex_weavatrix::{
    RefactorOperation, WeavatrixAdapter, WeavatrixConfig, compile_evidence_bundle,
};
use mcport::{ConcurrentMcpServer, FlushPolicy, RuntimeConfig, ToolReply, TransportLimits, json};
use serde::Deserialize;
use serde_json::Value;

pub mod http;
mod run_tools;
mod semantic;

use semantic::{SemanticConfig, SemanticScorer};

const DEFAULT_GRAPH_ID: &str = "default-control-plane";
const MAX_SKILL_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct CortexMcpState {
    store: GraphStore,
    weavatrix: WeavatrixAdapter,
    /// Present only under explicit `CORTEX_SHADOW=1` configuration; observes
    /// deterministic outcomes without any workflow influence.
    shadow: Option<Arc<ShadowHandle>>,
    /// Present only under explicit `CORTEX_SEMANTIC=1` configuration with a
    /// gated model; reorders evidence within priority bands and falls back
    /// to deterministic order on any failure.
    semantic: Option<Arc<SemanticScorer>>,
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
    /// Optional run attribution for quality-equivalent token accounting.
    run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteWorkArgs {
    #[serde(flatten)]
    request: RoutingRequest,
    run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefactorPreviewArgs {
    repository: PathBuf,
    operation: RefactorOperation,
    arguments: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShadowMetricsArgs {
    operation: Option<String>,
    model: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageReadArgs {
    operation: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageReportArgs {
    run_id: Option<String>,
    agent: String,
    input_tokens: u64,
    output_tokens: u64,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdapterExportArgs {
    graph_id: Option<String>,
    agent: AgentKind,
    launch: Option<McpLaunch>,
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
        let shadow = match cortex_shadow::spawn(ShadowConfig::from_env(), store.shadow()) {
            Ok(handle) => handle.map(Arc::new),
            Err(error) => {
                // A broken shadow configuration must never block the host.
                eprintln!("cortex-mcp: shadow mode disabled: {error}");
                None
            }
        };
        let semantic = match SemanticScorer::from_config(SemanticConfig::from_env()) {
            Ok(scorer) => scorer.map(Arc::new),
            Err(error) => {
                // A broken semantic configuration must never block the host.
                eprintln!("cortex-mcp: semantic ordering disabled: {error}");
                None
            }
        };
        Ok(Self {
            store,
            weavatrix,
            shadow,
            semantic,
        })
    }
}

/// Run the server over stdio (the default transport).
pub fn serve(state: CortexMcpState) -> io::Result<()> {
    build_server(state).serve(runtime_config())
}

/// The shared runtime bounds used by every transport.
#[must_use]
pub fn runtime_config() -> RuntimeConfig {
    RuntimeConfig {
        transport: TransportLimits::new(4 * 1024 * 1024, 8 * 1024 * 1024),
        max_in_flight: 4,
        queue_depth: 16,
        output_queue_depth: 16,
        output_flush_policy: FlushPolicy::PerMessage,
        handler_deadline: Some(Duration::from_secs(120)),
    }
}

/// Build the tool registry once; transports share the same server value.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn build_server(state: CortexMcpState) -> ConcurrentMcpServer {
    let state = Arc::new(state);
    let graph_state = Arc::clone(&state);
    let graph_list_state = Arc::clone(&state);
    let graph_save_state = Arc::clone(&state);
    let prepare_state = Arc::clone(&state);
    let context_state = Arc::clone(&state);
    let refactor_state = Arc::clone(&state);
    let route_state = Arc::clone(&state);
    let shadow_state = Arc::clone(&state);
    let adapter_state = Arc::clone(&state);
    let usage_state = Arc::clone(&state);
    let report_state = Arc::clone(&state);

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
                    "maxTokens": {"type": "integer", "minimum": 1, "maximum": 100_000},
                    "runId": {"type": "string", "maxLength": 256}
                },
                "required": ["repository", "task", "maxTokens"],
                "additionalProperties": false
            }),
            move |context, arguments: WeavatrixContextArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let mut bundle = match context_state.weavatrix.prepare_context(
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
                // Copied only when shadow observation is active; the shadow
                // runner never touches the deterministic reply below.
                let shadow_evidence = context_state.shadow.as_ref().map(|_| {
                    bundle
                        .evidence
                        .iter()
                        .map(|fragment| ShadowEvidence {
                            id: fragment.id.clone(),
                            source: fragment.source.clone(),
                            content: fragment.content.clone(),
                        })
                        .collect::<Vec<_>>()
                });
                let started = std::time::Instant::now();
                // Gated semantic ordering: on any failure the packet keeps
                // the deterministic order and records why.
                let mut semantic_note = None;
                let relevance = context_state.semantic.as_ref().and_then(|scorer| {
                    let fragments: Vec<(String, String)> = bundle
                        .evidence
                        .iter()
                        .map(|fragment| (fragment.id.clone(), fragment.content.clone()))
                        .collect();
                    match scorer.score(&arguments.task, &fragments) {
                        Ok(scores) => {
                            semantic_note = Some(scorer.provenance());
                            Some(scores)
                        }
                        Err(error) => {
                            bundle.warnings.push(format!(
                                "semantic ordering unavailable: {error}; deterministic order used"
                            ));
                            None
                        }
                    }
                });
                match compile_evidence_bundle(
                    bundle,
                    &arguments.task,
                    arguments.max_tokens,
                    relevance.as_ref(),
                ) {
                    Ok(mut packet) => {
                        packet.semantic_ranking = semantic_note;
                        record_usage(
                            &context_state.store,
                            &UsageSample {
                                operation: UsageOperation::ContextCompile,
                                run_id: arguments.run_id.clone(),
                                target: None,
                                model_tier: None,
                                task_class: None,
                                budget_tokens: Some(arguments.max_tokens),
                                raw_tokens: Some(packet.context.raw_estimated_tokens),
                                selected_tokens: Some(packet.context.selected_estimated_tokens),
                                omitted_tokens: Some(packet.context.omitted_estimated_tokens),
                                requires_upstream: Some(packet.context.requires_upstream),
                                latency_ms: Some(
                                    u64::try_from(started.elapsed().as_millis())
                                        .unwrap_or(u64::MAX),
                                ),
                            },
                        );
                        if let (Some(shadow), Some(evidence)) =
                            (&context_state.shadow, shadow_evidence)
                        {
                            shadow.observe(ShadowTask::ContextCompression {
                                task: arguments.task.clone(),
                                evidence,
                                deterministic: CompressionSnapshot {
                                    included_ids: packet.context.included_ids.clone(),
                                    omitted_ids: packet.context.omitted_ids.clone(),
                                    selected_estimated_tokens: packet
                                        .context
                                        .selected_estimated_tokens,
                                    requires_upstream: packet.context.requires_upstream,
                                },
                            });
                        }
                        ToolReply::structured(packet)
                    }
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
                    "availability": {"type": "object"},
                    "runId": {"type": "string", "maxLength": 256}
                },
                "required": ["task", "evidence", "schemaValid", "budget", "mutation", "availability"],
                "additionalProperties": false
            }),
            move |context, arguments: RouteWorkArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let decision = route(&arguments.request);
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
                        latency_ms: None,
                    },
                );
                ToolReply::structured(decision)
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
                ToolReply::structured(serde_json::json!({
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
                ToolReply::structured(serde_json::json!({
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
                    Ok(id) => ToolReply::structured(serde_json::json!({"recorded": true, "id": id})),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "adapter_export",
            "Render vendor wiring (Claude Code, Codex, or Copilot) from one canonical graph: skill instructions plus MCP registration. Preview-only: returns file contents and never writes.",
            json!({
                "type": "object",
                "properties": {
                    "graphId": {"type": "string", "maxLength": 256},
                    "agent": {"type": "string", "enum": ["claude_code", "codex", "copilot"]},
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
                let id = arguments.graph_id.as_deref().unwrap_or(DEFAULT_GRAPH_ID);
                let graph = match adapter_state.store.get(id) {
                    Ok(Some(graph)) => graph,
                    Ok(None) => return ToolReply::error(format!("graph not found: {id}")),
                    Err(error) => return ToolReply::error(error.to_string()),
                };
                let launch = arguments.launch.unwrap_or_default();
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
    run_tools::register(server, Arc::clone(&state))
}

/// Telemetry writes never fail the tool: the deterministic reply is the
/// product, the ledger is measurement.
fn record_usage(store: &GraphStore, sample: &UsageSample) {
    if let Err(error) = store.usage().insert(sample) {
        eprintln!("cortex-mcp: usage telemetry insert failed: {error}");
    }
}

fn to_snake<T: serde::Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
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
