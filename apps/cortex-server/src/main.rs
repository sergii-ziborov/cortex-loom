use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use cortex_domain::{GraphDocument, default_control_plane};
use cortex_skills::{export_skill_markdown, import_skill_markdown};
use cortex_store::{GraphStore, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::services::ServeDir;

mod runs;

const DEFAULT_ADDRESS: &str = "127.0.0.1:43817";

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GraphSummary {
    id: String,
    name: String,
    revision: u64,
    node_count: usize,
    edge_count: usize,
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
    let state = AppState { store };

    let api = Router::new()
        .route("/api/status", get(status))
        .route("/api/graphs", get(list_graphs))
        .route("/api/graphs/{id}", get(get_graph).put(save_graph))
        .route("/api/skills/compile", post(compile_skill))
        .route("/api/skills/export", post(export_skill))
        .merge(runs::routes())
        .with_state(state);
    let app = api.fallback_service(
        ServeDir::new(&settings.ui_directory).append_index_html_on_directories(true),
    );
    let listener = tokio::net::TcpListener::bind(settings.address).await?;
    println!("Cortex Loom UI: http://{}", settings.address);
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

async fn list_graphs(State(state): State<AppState>) -> Result<Json<Vec<GraphSummary>>, ApiError> {
    Ok(Json(
        state
            .store
            .list()?
            .into_iter()
            .map(|graph| GraphSummary {
                id: graph.id,
                name: graph.name,
                revision: graph.revision,
                node_count: graph.nodes.len(),
                edge_count: graph.edges.len(),
            })
            .collect(),
    ))
}

async fn get_graph(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<GraphDocument>, ApiError> {
    state
        .store
        .get(&id)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("graph not found: {id}")))
}

async fn save_graph(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(graph): Json<GraphDocument>,
) -> Result<Json<GraphDocument>, ApiError> {
    if id != graph.id {
        return Err(ApiError::BadRequest(
            "path graph id must match document id".to_owned(),
        ));
    }
    Ok(Json(state.store.save(&graph)?))
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

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

struct Settings {
    address: SocketAddr,
    database: PathBuf,
    ui_directory: PathBuf,
}

impl Settings {
    fn from_args() -> Result<Self, Box<dyn std::error::Error>> {
        let mut address = env::var("CORTEX_LOOM_ADDRESS")
            .unwrap_or_else(|_| DEFAULT_ADDRESS.to_owned())
            .parse()?;
        let mut database =
            env::var_os("CORTEX_LOOM_DB").map_or_else(default_database, PathBuf::from);
        let mut ui_directory = env::var_os("CORTEX_LOOM_UI_DIR")
            .map_or_else(|| PathBuf::from("ui/dist"), PathBuf::from);
        let mut arguments = env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--bind" => address = next_value(&mut arguments, "--bind")?.parse()?,
                "--db" => database = PathBuf::from(next_value(&mut arguments, "--db")?),
                "--ui-dir" => {
                    ui_directory = PathBuf::from(next_value(&mut arguments, "--ui-dir")?);
                }
                other => return Err(format!("unknown argument: {other}").into()),
            }
        }
        Ok(Self {
            address,
            database,
            ui_directory,
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
