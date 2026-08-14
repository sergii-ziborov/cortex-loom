use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cortex_adapters::{AgentKind, McpLaunch, export_adapter, export_library_adapter};
use cortex_context::{ContextRequest, compile_context};
use cortex_domain::{GraphDocument, default_control_plane};
use cortex_router::{RoutingDecision, RoutingRequest, route};
use cortex_shadow::{
    CompressionSnapshot, RoutingSnapshot, ShadowConfig, ShadowEvidence, ShadowHandle, ShadowTask,
};
use cortex_skills::{export_skill_markdown, import_skill_markdown, index_entry, render_index};
use cortex_store::{GraphStore, ShadowOperation, UsageOperation, UsageReport, UsageSample};
use cortex_weavatrix::{
    PlanHints, WeavatrixAdapter, WeavatrixConfig, assess_compiled, compile_evidence_bundle,
};
use mcport::{ConcurrentMcpServer, FlushPolicy, RuntimeConfig, ToolReply, TransportLimits, json};
use serde::Deserialize;
use serde_json::Value;

mod context_memory;
mod context_tools;
mod graph_skill_tools;
pub mod http;
mod llm_route;
mod plan_hints;
mod route_metric_tools;
mod run_tools;
mod semantic;
mod sequence_tools;
mod weavatrix_tools;

use llm_route::{LlmRouteConfig, LlmRouter};
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
    /// Present only under explicit `CORTEX_LLM=1` with a gated classification
    /// profile; may escalate above the lexical floor and fails closed to it.
    llm_router: Option<Arc<LlmRouter>>,
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
    /// Optional active skill graph. Its bounded frontmatter `PlanHints` guide
    /// gathering without coupling the planner to the skill compiler.
    skill_id: Option<String>,
    /// Plan the Weavatrix operations from the task instead of always asking
    /// the same four structural ones. Default.
    ///
    /// `false` selects the retired fixed set and is **deprecated**. It is
    /// dominated on both axes on every probe task measured: 36 039 tokens for
    /// 24/40 facts against 20 349 for 28/40. It remains accepted only so a
    /// caller pinned to its exact fragment ids does not break; no new caller
    /// should pass it.
    #[serde(default = "default_targeted")]
    targeted: bool,
}

const fn default_targeted() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillIndexArgs {
    format: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillReadArgs {
    id: String,
}

/// One catalogue row as JSON, for callers that would rather match on fields
/// than parse the Markdown rendering.
fn index_json(entry: &cortex_skills::SkillIndexEntry) -> serde_json::Value {
    serde_json::json!({
        "id": entry.id,
        "name": entry.name,
        "description": entry.description,
        "whenToUse": entry.when_to_use,
        "steps": entry.steps,
        "gates": entry.gates,
    })
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
    plan: Value,
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
    /// `library` wires the whole catalogue with deferred bodies; `graph`
    /// (the default) wires the single named graph as before.
    scope: Option<String>,
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
        plan_hints::seed_bundled_skills(&store)?;
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
        let llm_router = match LlmRouter::from_config(&LlmRouteConfig::from_env()) {
            Ok(router) => router.map(Arc::new),
            Err(error) => {
                eprintln!("cortex-mcp: local classifier disabled: {error}");
                None
            }
        };
        Ok(Self {
            store,
            weavatrix,
            shadow,
            semantic,
            llm_router,
        })
    }
}

/// Run the full server over stdio (the default transport).
pub fn serve(state: CortexMcpState) -> io::Result<()> {
    serve_with(state, ServerProfile::Full)
}

/// Run one profile's server over stdio.
pub fn serve_with(state: CortexMcpState, profile: ServerProfile) -> io::Result<()> {
    build_server_with(state, profile).serve(runtime_config())
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

/// Which slice of the tool surface a server exposes.
///
/// Every tool schema is loaded into the client's context for the whole
/// session, before a single call is made, so the surface is a standing cost.
/// Measured on this workspace: `Full` is 27 tools over two `tools/list` pages
/// and roughly 4 000 estimated tokens. A caller that only wants evidence pays
/// that for twenty-five tools it never calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerProfile {
    /// Graphs, skills, routing, runs, sequences, adapters and evidence.
    #[default]
    Full,
    /// Evidence compilation only. Routing, runs, graphs, skills and sequences
    /// are absent; a caller that needs them wants [`ServerProfile::Full`].
    Context,
}

impl ServerProfile {
    /// Parse a profile name, for a CLI flag or an environment variable.
    ///
    /// # Errors
    ///
    /// Returns the offending value when it names no known profile, so a
    /// misspelling fails loudly instead of silently serving everything.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" => Ok(Self::Full),
            "context" => Ok(Self::Context),
            other => Err(format!("unknown profile: {other} (full|context)")),
        }
    }
}

/// Build the full tool registry; transports share the same server value.
#[must_use]
pub fn build_server(state: CortexMcpState) -> ConcurrentMcpServer {
    build_server_with(state, ServerProfile::Full)
}

/// Build the tool registry for one profile.
#[must_use]
pub fn build_server_with(state: CortexMcpState, profile: ServerProfile) -> ConcurrentMcpServer {
    let state = Arc::new(state);

    let server = ConcurrentMcpServer::new("cortex-loom", env!("CARGO_PKG_VERSION"))
        .instructions(instructions(profile))
        .tool_page_size(16);
    let server = context_tools::register(server, &state);
    if profile == ServerProfile::Context {
        return server;
    }
    let server = graph_skill_tools::register(server, &state);
    let server = route_metric_tools::register(server, &state, route);
    let server = weavatrix_tools::register(server, &state);
    let server = run_tools::register(server, Arc::clone(&state));
    sequence_tools::register(server, &state)
}

const fn instructions(profile: ServerProfile) -> &'static str {
    match profile {
        ServerProfile::Full => {
            "Cortex Loom reduces repository context before Codex or Claude reasons about it. Use route_work first, then weavatrix_context_compile for revision-bound, budgeted evidence. Local-model results are advisory and must retain evidence IDs. High-risk or ambiguous work stays upstream. Refactor is preview-only: this server never applies a plan. Graphs are canonical in the local store; generated Markdown is only a view."
        }
        ServerProfile::Context => {
            "Cortex Loom reduces repository context before Codex or Claude reasons about it. Call weavatrix_context_compile for revision-bound, budgeted evidence with stable citation IDs; name the symbols, files, and constants you care about in `task`. A packet that reports requiresUpstream or an unmet sufficiency check is not a confident answer. This profile exposes evidence compilation only: routing, runs, graphs, skills, and sequences need the full profile."
        }
    }
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

fn refactor_preview_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "repository": {"type": "string"},
            "plan": {
                "type": "object",
                "description": "A complete weavatrix.refactor-plan.v1 object; native Rust parsing rejects unknown operation fields, unsafe paths, stale hashes, and oversized values.",
                "additionalProperties": true
            }
        },
        "required": ["repository", "plan"],
        "additionalProperties": false
    })
}

fn preview_refactor_response(
    adapter: &WeavatrixAdapter,
    repository: &std::path::Path,
    plan: &Value,
) -> Result<Value, String> {
    adapter
        .preview_refactor(repository, plan)
        .map(|preview| serde_json::json!({"mode": "preview", "preview": preview}))
        .map_err(|error| error.to_string())
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
#[path = "lib_tests.rs"]
mod tests;
