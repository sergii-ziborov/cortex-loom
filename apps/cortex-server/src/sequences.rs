use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use cortex_domain::GraphDocument;
use cortex_sequences::{
    ActivationHints, ActiveStepPacket, SequenceDiagnostic, TemplateVersion, active_step_packet,
    instantiate_template, lint_sequence, templates,
};
use serde::{Deserialize, Serialize};

use crate::{ApiError, AppState};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TemplateSummary {
    id: &'static str,
    version: TemplateVersion,
    title: &'static str,
    description: &'static str,
    changelog: &'static str,
    activation: ActivationHints,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TemplateDetail {
    #[serde(flatten)]
    summary: TemplateSummary,
    markdown: &'static str,
    graph: GraphDocument,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopyRequest {
    graph_id: String,
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CopyResponse {
    created: bool,
    graph: GraphDocument,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActiveStepRequest {
    graph_id: String,
    node_id: String,
    #[serde(default)]
    evidence_ids: Vec<String>,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/sequences/templates", get(list_templates))
        .route("/api/sequences/templates/{id}", get(get_template))
        .route("/api/sequences/templates/{id}/copy", post(copy_template))
        .route("/api/sequences/lint", post(lint))
        .route("/api/sequences/active-step", post(read_active_step))
}

async fn list_templates() -> Json<Vec<TemplateSummary>> {
    Json(templates().iter().map(summary).collect())
}

async fn get_template(Path(id): Path<String>) -> Result<Json<TemplateDetail>, ApiError> {
    let template = find_template(&id)?;
    let graph = instantiate_template(
        template.id,
        &format!("template-preview-{}", template.id),
        template.title,
    )
    .map_err(|error| sequence_error(&error))?;
    Ok(Json(TemplateDetail {
        summary: summary(template),
        markdown: template.markdown,
        graph,
    }))
}

async fn copy_template(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<CopyRequest>,
) -> Result<Json<CopyResponse>, ApiError> {
    find_template(&id)?;
    let graph = instantiate_template(&id, &request.graph_id, &request.name)
        .map_err(|error| sequence_error(&error))?;
    let created = state.store.get(&graph.id)?.is_none();
    let graph = state.store.seed_if_missing(&graph)?;
    Ok(Json(CopyResponse { created, graph }))
}

async fn lint(Json(graph): Json<GraphDocument>) -> Json<Vec<SequenceDiagnostic>> {
    Json(lint_sequence(&graph))
}

async fn read_active_step(
    State(state): State<AppState>,
    Json(request): Json<ActiveStepRequest>,
) -> Result<Json<ActiveStepPacket>, ApiError> {
    let graph = state
        .store
        .get(&request.graph_id)?
        .ok_or_else(|| ApiError::NotFound(format!("graph not found: {}", request.graph_id)))?;
    active_step_packet(&graph, &request.node_id, &request.evidence_ids)
        .map(Json)
        .map_err(|error| sequence_error(&error))
}

fn find_template(id: &str) -> Result<&'static cortex_sequences::SequenceTemplate, ApiError> {
    templates()
        .iter()
        .find(|template| template.id == id)
        .ok_or_else(|| ApiError::NotFound(format!("sequence template not found: {id}")))
}

fn summary(template: &'static cortex_sequences::SequenceTemplate) -> TemplateSummary {
    TemplateSummary {
        id: template.id,
        version: template.version,
        title: template.title,
        description: template.description,
        changelog: template.changelog,
        activation: template.activation,
    }
}

fn sequence_error(error: &cortex_sequences::SequenceError) -> ApiError {
    ApiError::BadRequest(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_store::GraphStore;

    fn state() -> AppState {
        AppState {
            store: GraphStore::open_in_memory().expect("store"),
        }
    }

    #[tokio::test]
    async fn exposes_exactly_seven_immutable_templates() {
        let Json(list) = list_templates().await;
        assert_eq!(list.len(), 7);
        assert_eq!(list[0].id, "discover-and-plan");
        assert_eq!(list[6].id, "sequence-authoring");
    }

    #[tokio::test]
    async fn copying_twice_does_not_overwrite_an_edited_sequence() {
        let state = state();
        let request = || CopyRequest {
            graph_id: "my-debugging".to_owned(),
            name: "My debugging".to_owned(),
        };
        let Json(first) = copy_template(
            State(state.clone()),
            Path("root-cause-debugging".to_owned()),
            Json(request()),
        )
        .await
        .expect("first copy");
        assert!(first.created);

        let mut edited = first.graph;
        edited.name = "My edited debugging".to_owned();
        let edited = state.store.save(&edited).expect("save edit");
        let Json(second) = copy_template(
            State(state),
            Path("root-cause-debugging".to_owned()),
            Json(request()),
        )
        .await
        .expect("second copy");
        assert!(!second.created);
        assert_eq!(second.graph, edited);
    }

    #[tokio::test]
    async fn active_step_response_does_not_leak_inactive_instructions() {
        let state = state();
        let graph = instantiate_template(
            "bounded-implementation",
            "my-implementation",
            "My implementation",
        )
        .expect("template");
        let mut executable = graph.nodes.iter().filter_map(|node| {
            node.config
                .get("instruction")
                .and_then(serde_json::Value::as_str)
                .map(|instruction| (node.id.clone(), instruction.to_owned()))
        });
        let (active, _) = executable.next().expect("active instruction");
        let (_, inactive_instruction) = executable.next().expect("inactive instruction");
        state.store.seed_if_missing(&graph).expect("seed");

        let Json(packet) = read_active_step(
            State(state),
            Json(ActiveStepRequest {
                graph_id: graph.id,
                node_id: active,
                evidence_ids: vec!["evidence-1".to_owned()],
            }),
        )
        .await
        .expect("packet");
        let encoded = serde_json::to_string(&packet).expect("json");
        assert!(!encoded.contains(&inactive_instruction));
        assert_eq!(packet.evidence_ids, ["evidence-1"]);
    }
}
