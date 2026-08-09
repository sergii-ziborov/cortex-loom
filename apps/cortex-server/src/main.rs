use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use cortex_adapters::{AdapterBundle, AgentKind, McpLaunch, export_adapter};
use cortex_domain::{GraphDocument, default_control_plane};
use cortex_skills::{export_skill_markdown, import_skill_markdown};
use cortex_store::{
    GraphStore, QualitySummary, ShadowAggregate, ShadowOperation, ShadowSampleRow, StoreError,
    UsageOperation, UsageReport, UsageReportRow, UsageSampleRow, UsageSummary,
};
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::services::ServeDir;

mod docs;
mod graphs;
mod library;
mod runs;
mod sequences;

const DEFAULT_ADDRESS: &str = "127.0.0.1:43817";

/// Production UI assets baked into the binary for single-file distribution.
/// Empty when the UI was not built before compilation; the server then serves
/// from disk exactly as before.
static EMBEDDED_UI: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../ui/dist");

#[derive(Clone)]
pub(crate) struct AppState {
    store: GraphStore,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompileSkillRequest {
    #[serde(alias = "name")]
    source: String,
    markdown: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportSkillResponse {
    markdown: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    ok: bool,
    version: &'static str,
    graph_count: usize,
}

#[derive(Debug)]
pub(crate) enum ApiError {
    NotFound(String),
    BadRequest(String),
    Conflict { message: String, current: u64 },
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::NotFound(message) => (StatusCode::NOT_FOUND, json!({"error": message})),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, json!({"error": message})),
            Self::Conflict { message, current } => (
                StatusCode::CONFLICT,
                json!({"error": message, "currentRevision": current}),
            ),
            Self::Internal(message) => {
                (StatusCode::INTERNAL_SERVER_ERROR, json!({"error": message}))
            }
        };
        (status, Json(body)).into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(value: StoreError) -> Self {
        match value {
            StoreError::Conflict { current, .. } => Self::Conflict {
                message: value.to_string(),
                current,
            },
            StoreError::Run(cortex_run::RunError::RevisionConflict { current, .. }) => {
                Self::Conflict {
                    message: value.to_string(),
                    current,
                }
            }
            StoreError::RunNotFound(id) => Self::NotFound(format!("run not found: {id}")),
            StoreError::RunAlreadyExists(_) => Self::Conflict {
                message: value.to_string(),
                current: 0,
            },
            StoreError::Run(_) => Self::BadRequest(value.to_string()),
            other => Self::Internal(other.to_string()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::from_args()?;
    if let Some(parent) = settings.database.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let store = GraphStore::open(&settings.database)?;
    store.seed_if_missing(&default_control_plane())?;
    // Ship working methodology rather than an empty editor. An improved
    // bundled workflow replaces a copy nobody has saved; the moment a user
    // saves one it is theirs and an upgrade leaves it alone.
    for skill in cortex_skills::bundled_skills() {
        match import_skill_markdown(skill.source, skill.markdown) {
            Ok(graph) => match store.seed_or_refresh_unsaved(&graph) {
                Ok(true) => println!("Seeded methodology: {}", graph.name),
                Ok(false) => {}
                Err(error) => {
                    eprintln!("cortex-server: could not seed skill {}: {error}", skill.id);
                }
            },
            Err(error) => eprintln!(
                "cortex-server: bundled skill {} is invalid: {error}",
                skill.id
            ),
        }
    }
    let state = AppState { store };

    let api = Router::new()
        .route("/api/status", get(status))
        .route("/api/skills/compile", post(compile_skill))
        .route("/api/skills/export", post(export_skill))
        .route("/api/shadow/metrics", get(shadow_metrics))
        .route("/api/shadow/samples", get(shadow_samples))
        .route("/api/usage/summary", get(usage_summary))
        .route("/api/usage/quality", get(usage_quality))
        .route("/api/usage/samples", get(usage_samples))
        .route("/api/usage/reports", get(usage_reports).post(usage_report))
        .route("/api/adapters/{agent}", get(adapter_bundle))
        .merge(docs::routes())
        .merge(graphs::routes())
        .merge(library::routes())
        .merge(runs::routes())
        .merge(sequences::routes())
        .with_state(state);
    let use_embedded = !settings.explicit_ui_directory && !EMBEDDED_UI.entries().is_empty();
    let app = if use_embedded {
        api.fallback(embedded_ui)
    } else {
        api.fallback_service(
            ServeDir::new(&settings.ui_directory).append_index_html_on_directories(true),
        )
    };
    let listener = tokio::net::TcpListener::bind(settings.address).await?;
    println!("Cortex Loom UI: http://{}", settings.address);
    println!(
        "UI assets: {}",
        if use_embedded {
            "embedded in the binary".to_owned()
        } else {
            settings.ui_directory.display().to_string()
        }
    );
    println!("Database: {}", settings.database.display());
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn status(State(state): State<AppState>) -> Result<Json<StatusResponse>, ApiError> {
    let graph_count = state.store.list()?.len();
    Ok(Json(StatusResponse {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
        graph_count,
    }))
}

async fn compile_skill(
    Json(request): Json<CompileSkillRequest>,
) -> Result<Json<GraphDocument>, ApiError> {
    if request.markdown.len() > 2 * 1024 * 1024 {
        return Err(ApiError::BadRequest(
            "skill Markdown exceeds the 2 MiB limit".to_owned(),
        ));
    }
    let graph = import_skill_markdown(&request.source, &request.markdown)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    Ok(Json(graph))
}

async fn export_skill(
    Json(graph): Json<GraphDocument>,
) -> Result<Json<ExportSkillResponse>, ApiError> {
    let markdown =
        export_skill_markdown(&graph).map_err(|error| ApiError::BadRequest(error.to_string()))?;
    Ok(Json(ExportSkillResponse { markdown }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShadowQuery {
    operation: Option<String>,
    model: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdapterQuery {
    graph_id: Option<String>,
}

/// Preview-only: returns vendor wiring content; nothing is written by the
/// server.
async fn adapter_bundle(
    State(state): State<AppState>,
    AxumPath(agent): AxumPath<String>,
    Query(query): Query<AdapterQuery>,
) -> Result<Json<AdapterBundle>, ApiError> {
    let agent = AgentKind::parse(&agent)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown agent: {agent}")))?;
    let id = query.graph_id.as_deref().unwrap_or("default-control-plane");
    let graph = state
        .store
        .get(id)?
        .ok_or_else(|| ApiError::NotFound(format!("graph not found: {id}")))?;
    let bundle = export_adapter(&graph, agent, &McpLaunch::default())
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    Ok(Json(bundle))
}

fn parse_shadow_operation(value: Option<&str>) -> Result<Option<ShadowOperation>, ApiError> {
    match value {
        None => Ok(None),
        Some(raw) => ShadowOperation::parse(raw)
            .map(Some)
            .ok_or_else(|| ApiError::BadRequest(format!("unknown shadow operation: {raw}"))),
    }
}

/// Bounded read-only aggregates; shadow output never influences workflows.
async fn shadow_metrics(
    State(state): State<AppState>,
    Query(query): Query<ShadowQuery>,
) -> Result<Json<Vec<ShadowAggregate>>, ApiError> {
    let operation = parse_shadow_operation(query.operation.as_deref())?;
    Ok(Json(
        state
            .store
            .shadow()
            .aggregate(operation, query.model.as_deref())?,
    ))
}

async fn shadow_samples(
    State(state): State<AppState>,
    Query(query): Query<ShadowQuery>,
) -> Result<Json<Vec<ShadowSampleRow>>, ApiError> {
    let operation = parse_shadow_operation(query.operation.as_deref())?;
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    Ok(Json(state.store.shadow().list(
        operation,
        query.model.as_deref(),
        limit,
    )?))
}

/// Bounded read of the append-only token-accounting ledger.
async fn usage_summary(State(state): State<AppState>) -> Result<Json<UsageSummary>, ApiError> {
    Ok(Json(state.store.usage().summary()?))
}

/// Savings joined with run outcomes: only clean succeeded runs are credited.
async fn usage_quality(State(state): State<AppState>) -> Result<Json<QualitySummary>, ApiError> {
    Ok(Json(state.store.usage().quality_summary()?))
}

/// Append one self-reported upstream consumption record.
async fn usage_report(
    State(state): State<AppState>,
    Json(report): Json<UsageReport>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if report.agent.trim().is_empty() {
        return Err(ApiError::BadRequest("agent must not be empty".to_owned()));
    }
    let id = state.store.usage().insert_report(&report)?;
    Ok(Json(json!({"recorded": true, "id": id})))
}

async fn usage_reports(
    State(state): State<AppState>,
    Query(query): Query<ShadowQuery>,
) -> Result<Json<Vec<UsageReportRow>>, ApiError> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    Ok(Json(state.store.usage().list_reports(limit)?))
}

async fn usage_samples(
    State(state): State<AppState>,
    Query(query): Query<ShadowQuery>,
) -> Result<Json<Vec<UsageSampleRow>>, ApiError> {
    let operation = match query.operation.as_deref() {
        None => None,
        Some(raw) => Some(
            UsageOperation::parse(raw)
                .ok_or_else(|| ApiError::BadRequest(format!("unknown usage operation: {raw}")))?,
        ),
    };
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    Ok(Json(state.store.usage().list(operation, limit)?))
}

/// Serve one embedded UI asset; extensionless paths fall back to the SPA
/// index. Lookup is by exact relative path in the baked file map, so path
/// traversal cannot escape the bundle.
async fn embedded_ui(uri: axum::http::Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    let candidate = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    let file = EMBEDDED_UI.get_file(candidate).or_else(|| {
        (!candidate.contains('.'))
            .then(|| EMBEDDED_UI.get_file("index.html"))
            .flatten()
    });
    match file {
        Some(file) => (
            [(
                axum::http::header::CONTENT_TYPE,
                content_type(&file.path().to_string_lossy()),
            )],
            file.contents(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

struct Settings {
    address: SocketAddr,
    database: PathBuf,
    ui_directory: PathBuf,
    /// True when the operator explicitly chose a directory; embedded assets
    /// are then bypassed.
    explicit_ui_directory: bool,
}

impl Settings {
    fn from_args() -> Result<Self, Box<dyn std::error::Error>> {
        let mut address = env::var("CORTEX_LOOM_ADDRESS")
            .unwrap_or_else(|_| DEFAULT_ADDRESS.to_owned())
            .parse()?;
        let mut database =
            env::var_os("CORTEX_LOOM_DB").map_or_else(default_database, PathBuf::from);
        let env_ui_directory = env::var_os("CORTEX_LOOM_UI_DIR").map(PathBuf::from);
        let mut explicit_ui_directory = env_ui_directory.is_some();
        let mut ui_directory = env_ui_directory.unwrap_or_else(|| PathBuf::from("ui/dist"));
        let mut arguments = env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--bind" => address = next_value(&mut arguments, "--bind")?.parse()?,
                "--db" => database = PathBuf::from(next_value(&mut arguments, "--db")?),
                "--ui-dir" => {
                    ui_directory = PathBuf::from(next_value(&mut arguments, "--ui-dir")?);
                    explicit_ui_directory = true;
                }
                other => return Err(format!("unknown argument: {other}").into()),
            }
        }
        Ok(Self {
            address,
            database,
            ui_directory,
            explicit_ui_directory,
        })
    }
}

fn default_database() -> PathBuf {
    Path::new(".cortex-loom").join("cortex-loom.db")
}

fn next_value(
    arguments: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    arguments
        .next()
        .ok_or_else(|| format!("{flag} requires a value").into())
}
