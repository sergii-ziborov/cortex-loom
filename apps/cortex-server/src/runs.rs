use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use cortex_run::{NodeRunStatus, ReplayVerification, RunCommand, RunDocument, RunEvent, RunStatus};
use serde::{Deserialize, Serialize};

use crate::{ApiError, AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRunRequest {
    id: String,
    graph_id: String,
    graph_revision: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListRunsQuery {
    graph_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEventsQuery {
    after: Option<u64>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunSummary {
    id: String,
    graph_id: String,
    graph_revision: u64,
    revision: u64,
    status: RunStatus,
    updated_at: i64,
    ready_count: usize,
    running_count: usize,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/runs", get(list_runs).post(create_run))
        .route("/api/runs/{id}", get(get_run))
        .route("/api/runs/{id}/graph", get(get_run_graph))
        .route("/api/runs/{id}/commands", post(apply_run_command))
        .route("/api/runs/{id}/events", get(list_run_events))
        .route("/api/runs/{id}/replay", post(verify_run_replay))
}

async fn create_run(
    State(state): State<AppState>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<RunDocument>, ApiError> {
    let graph = state
        .store
        .get(&request.graph_id)?
        .ok_or_else(|| ApiError::NotFound(format!("graph not found: {}", request.graph_id)))?;
    if graph.revision != request.graph_revision {
        return Err(ApiError::Conflict {
            message: format!(
                "graph {} revision conflict: expected {}, current {}",
                graph.id, request.graph_revision, graph.revision
            ),
            current: graph.revision,
        });
    }
    Ok(Json(state.store.runs().create(&request.id, &graph)?))
}

async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<ListRunsQuery>,
) -> Result<Json<Vec<RunSummary>>, ApiError> {
    let runs = state
        .store
        .runs()
        .list(query.graph_id.as_deref(), query.limit.unwrap_or(50))?;
    Ok(Json(runs.iter().map(run_summary).collect()))
}

async fn get_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RunDocument>, ApiError> {
    state
        .store
        .runs()
        .get(&id)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("run not found: {id}")))
}

async fn get_run_graph(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<cortex_domain::GraphDocument>, ApiError> {
    state
        .store
        .runs()
        .get_graph(&id)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("run not found: {id}")))
}

async fn apply_run_command(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(command): Json<RunCommand>,
) -> Result<Json<RunDocument>, ApiError> {
    Ok(Json(state.store.runs().apply(&id, &command)?))
}

async fn list_run_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ListEventsQuery>,
) -> Result<Json<Vec<RunEvent>>, ApiError> {
    Ok(Json(state.store.runs().events(
        &id,
        query.after.unwrap_or(0),
        query.limit.unwrap_or(100),
    )?))
}

async fn verify_run_replay(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ReplayVerification>, ApiError> {
    Ok(Json(state.store.runs().verify_replay(&id)?))
}

fn run_summary(run: &RunDocument) -> RunSummary {
    RunSummary {
        id: run.id.clone(),
        graph_id: run.graph_id.clone(),
        graph_revision: run.graph_revision,
        revision: run.revision,
        status: run.status,
        updated_at: run.updated_at,
        ready_count: run
            .nodes
            .iter()
            .filter(|node| node.status == NodeRunStatus::Ready)
            .count(),
        running_count: run
            .nodes
            .iter()
            .filter(|node| node.status == NodeRunStatus::Running)
            .count(),
    }
}
