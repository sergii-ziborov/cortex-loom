//! Run-bound methodology packets. The run's immutable graph snapshot is the
//! source of instructions; saved graph edits cannot change an active run.

use std::sync::Arc;

use cortex_context::{RUNTIME_COUNTER, TokenCounter};
use cortex_run::{NodeRunStatus, RunDocument, RunStatus};
use cortex_sequences::active_step_packet;
use mcport::{ConcurrentMcpServer, ToolReply, json};
use serde::Deserialize;
use serde_json::Value;

use crate::CortexMcpState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunStepArgs {
    id: String,
    expected_revision: u64,
    node_id: String,
}

pub(crate) fn register(
    server: ConcurrentMcpServer,
    state: &Arc<CortexMcpState>,
) -> ConcurrentMcpServer {
    let state = Arc::clone(state);
    server.typed_tool(
        "run_step_read",
        "Read a ready or running step from the run's immutable graph snapshot. Rejects stale revisions and leased nodes; host-only lease owners use the trusted run API.",
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "minLength": 1, "maxLength": 256},
                "expectedRevision": {"type": "integer", "minimum": 1},
                "nodeId": {"type": "string", "minLength": 1, "maxLength": 256}
            },
            "required": ["id", "expectedRevision", "nodeId"],
            "additionalProperties": false
        }),
        move |context, arguments: RunStepArgs| {
            if context.is_cancelled() {
                return ToolReply::error("cancelled");
            }
            match read_run_step(&state, &arguments) {
                Ok(value) => ToolReply::text(value),
                Err(error) => ToolReply::error(error),
            }
        },
    )
}

fn read_run_step(state: &CortexMcpState, args: &RunStepArgs) -> Result<Value, String> {
    let run = state
        .store
        .runs()
        .get(&args.id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("run not found: {}", args.id))?;
    let graph = state
        .store
        .runs()
        .get_graph(&args.id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("run graph snapshot not found: {}", args.id))?;
    compile_run_step(&run, &graph, &args.node_id, args.expected_revision)
}

fn compile_run_step(
    run: &RunDocument,
    graph: &cortex_domain::GraphDocument,
    node_id: &str,
    expected_revision: u64,
) -> Result<Value, String> {
    if run.revision != expected_revision {
        return Err(format!(
            "run revision conflict: expected {expected_revision}, current {}",
            run.revision
        ));
    }
    if run.status != RunStatus::Running {
        return Err(format!("run is not active: {:?}", run.status));
    }
    if graph.id != run.graph_id || graph.revision != run.graph_revision {
        return Err("run graph snapshot identity mismatch".to_owned());
    }
    let node = run
        .nodes
        .iter()
        .find(|node| node.node_id == node_id)
        .ok_or_else(|| format!("node not found in run: {node_id}"))?;
    if !matches!(node.status, NodeRunStatus::Ready | NodeRunStatus::Running) {
        return Err(format!("node {node_id} is not active: {:?}", node.status));
    }
    if node.lease.is_some() {
        return Err(format!(
            "node {node_id} is leased; use the trusted host API"
        ));
    }
    let ids = run
        .evidence
        .iter()
        .filter(|item| {
            item.node_id == node_id && item.attempt == node.attempt && item.invalidated.is_none()
        })
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let packet = active_step_packet(graph, node_id, &ids).map_err(|error| error.to_string())?;
    let reply = json!({
        "runId": run.id,
        "runRevision": run.revision,
        "nodeStatus": node.status,
        "graphSnapshotRevision": graph.revision,
        "packet": packet,
    });
    let tokens = RUNTIME_COUNTER.count(&reply.to_string());
    if tokens > packet.max_input_tokens {
        return Err(format!(
            "run step reply requires {tokens} conservative tokens, exceeds maxInputTokens {}",
            packet.max_input_tokens
        ));
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_sequences::instantiate_template;
    use cortex_store::GraphStore;

    #[test]
    fn run_step_uses_snapshot_and_rejects_stale_or_pending_nodes() {
        let store = GraphStore::open_in_memory().unwrap();
        let graph = instantiate_template("discover-and-plan", "graph", "Plan").unwrap();
        let mut run = store.runs().create("run", &graph).unwrap();
        let snapshot = store.runs().get_graph("run").unwrap().unwrap();
        let step_id = graph
            .nodes
            .iter()
            .find(|node| node.label.starts_with("Ask Weavatrix"))
            .unwrap()
            .id
            .clone();
        run.nodes
            .iter_mut()
            .find(|node| node.node_id == step_id)
            .unwrap()
            .status = NodeRunStatus::Ready;
        let reply = compile_run_step(&run, &snapshot, &step_id, run.revision).unwrap();
        assert_eq!(reply["graphSnapshotRevision"], graph.revision);
        assert_eq!(reply["packet"]["graphId"], graph.id.as_str());

        let mut edited = graph.clone();
        edited.revision += 1;
        edited.nodes[0].label = "edited after run creation".to_owned();
        assert_ne!(snapshot.nodes[0].label, edited.nodes[0].label);
        assert!(compile_run_step(&run, &snapshot, &step_id, run.revision + 1).is_err());
        let pending = run
            .nodes
            .iter()
            .find(|node| node.status == NodeRunStatus::Pending && node.node_id != step_id)
            .unwrap();
        assert!(compile_run_step(&run, &snapshot, &pending.node_id, run.revision).is_err());
    }

    #[test]
    fn invented_and_invalidated_evidence_never_enters_the_step() {
        let graph = instantiate_template("discover-and-plan", "graph", "Plan").unwrap();
        let mut run = cortex_run::create_run(&graph, "run", 1).unwrap().0;
        let ready = graph
            .nodes
            .iter()
            .find(|node| node.label.starts_with("Ask Weavatrix"))
            .unwrap()
            .id
            .clone();
        run.nodes
            .iter_mut()
            .find(|node| node.node_id == ready)
            .unwrap()
            .status = NodeRunStatus::Ready;
        run.evidence.push(cortex_run::EvidenceSubmission {
            id: "invalidated".to_owned(),
            node_id: ready.clone(),
            attempt: 0,
            submitted_by: "test".to_owned(),
            source: "test".to_owned(),
            locator: "test".to_owned(),
            digest: None,
            summary: "test".to_owned(),
            submitted_at: 1,
            invalidated: Some(cortex_run::EvidenceInvalidation {
                actor: "test".to_owned(),
                reason: "stale".to_owned(),
                invalidated_at: 2,
            }),
        });
        let reply = compile_run_step(&run, &graph, &ready, run.revision).unwrap();
        assert_eq!(reply["packet"]["evidenceIds"], json!([]));
    }
}
