use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use cortex_domain::GraphDocument;
use serde::Serialize;

use crate::{ApiError, AppState};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GraphSummary {
    id: String,
    name: String,
    revision: u64,
    node_count: usize,
    edge_count: usize,
    description: String,
    origin: String,
    origin_kind: &'static str,
    kinds: Vec<&'static str>,
    /// Immutable source template for an editable Cortex sequence copy.
    template_id: Option<String>,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/graphs", get(list_graphs))
        .route("/api/graphs/{id}", get(get_graph).put(save_graph))
}

async fn list_graphs(State(state): State<AppState>) -> Result<Json<Vec<GraphSummary>>, ApiError> {
    Ok(Json(state.store.list()?.iter().map(summarize).collect()))
}

async fn get_graph(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<GraphDocument>, ApiError> {
    state
        .store
        .get(&id)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("graph not found: {id}")))
}

async fn save_graph(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(graph): Json<GraphDocument>,
) -> Result<Json<GraphDocument>, ApiError> {
    if id != graph.id {
        return Err(ApiError::BadRequest(
            "path graph id must match document id".to_owned(),
        ));
    }
    Ok(Json(state.store.save(&graph)?))
}

fn summarize(graph: &GraphDocument) -> GraphSummary {
    let mut kinds: Vec<&'static str> = graph
        .nodes
        .iter()
        .map(|node| node.kind.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    kinds.truncate(8);
    GraphSummary {
        id: graph.id.clone(),
        name: graph.name.clone(),
        revision: graph.revision,
        node_count: graph.nodes.len(),
        edge_count: graph.edges.len(),
        description: graph
            .metadata
            .get("description")
            .cloned()
            .unwrap_or_default(),
        origin: graph
            .metadata
            .get("library")
            .or_else(|| graph.metadata.get("source"))
            .cloned()
            .unwrap_or_else(|| "local".to_owned()),
        origin_kind: origin_kind(graph),
        kinds,
        template_id: graph.metadata.get("sequence.templateId").cloned(),
    }
}

const BUNDLED_SOURCE_PREFIX: &str = "cortex-skills/fixtures/";

fn origin_kind(graph: &GraphDocument) -> &'static str {
    if graph.metadata.contains_key("library") {
        return "imported";
    }
    match graph.metadata.get("source") {
        Some(source) if source.starts_with(BUNDLED_SOURCE_PREFIX) => "bundled",
        _ => "local",
    }
}
