use std::collections::HashMap;

use cortex_domain::{
    EdgeKind, GRAPH_SCHEMA_VERSION, GraphDocument, GraphEdge, GraphNode, NodeKind, Position,
};

use super::*;
use crate::tests::{apply, command_start, status};

pub(crate) fn evidence_command(run: &RunDocument, node_id: &str, evidence_id: &str) -> RunCommand {
    RunCommand::SubmitEvidence {
        expected_revision: run.revision,
        node_id: node_id.to_owned(),
        evidence_id: evidence_id.to_owned(),
        submitted_by: "weavatrix".to_owned(),
        source: "graph".to_owned(),
        locator: format!("node:{node_id}"),
        digest: Some("sha256:abc".to_owned()),
        summary: "Bounded graph evidence".to_owned(),
        executor: None,
    }
}

pub(crate) fn simple_graph(root_kind: NodeKind, edges: Vec<GraphEdge>) -> GraphDocument {
    GraphDocument {
        schema_version: GRAPH_SCHEMA_VERSION.to_owned(),
        id: "simple".to_owned(),
        name: "Simple".to_owned(),
        revision: 3,
        nodes: vec![
            node("root", root_kind),
            node("ok", NodeKind::Output),
            node("recovery", NodeKind::Output),
        ],
        edges,
        metadata: HashMap::new(),
    }
}

pub(crate) fn node(id: &str, kind: NodeKind) -> GraphNode {
    GraphNode {
        id: id.to_owned(),
        kind,
        label: id.to_owned(),
        description: String::new(),
        position: Position { x: 0.0, y: 0.0 },
        execution: None,
        provenance: Vec::new(),
        config: HashMap::new(),
    }
}

pub(crate) fn edge(id: &str, from: &str, to: &str, kind: EdgeKind) -> GraphEdge {
    GraphEdge {
        id: id.to_owned(),
        from: from.to_owned(),
        to: to.to_owned(),
        kind,
        label: String::new(),
        condition: None,
    }
}

fn upstream(id: &str) -> ExecutorIdentity {
    ExecutorIdentity {
        kind: ExecutorKind::UpstreamAgent,
        id: id.to_owned(),
    }
}

fn claim(run: &RunDocument, node_id: &str, executor: &ExecutorIdentity, ttl: u32) -> RunCommand {
    RunCommand::ClaimLease {
        expected_revision: run.revision,
        node_id: node_id.to_owned(),
        executor: executor.clone(),
        ttl_seconds: ttl,
    }
}

fn start_as(run: &RunDocument, node_id: &str, executor: &ExecutorIdentity) -> RunCommand {
    RunCommand::StartNode {
        expected_revision: run.revision,
        node_id: node_id.to_owned(),
        executor: Some(executor.clone()),
    }
}

fn complete_as(
    run: &RunDocument,
    node_id: &str,
    outcome: NodeOutcome,
    executor: &ExecutorIdentity,
) -> RunCommand {
    RunCommand::CompleteNode {
        expected_revision: run.revision,
        node_id: node_id.to_owned(),
        outcome,
        selected_edge_ids: Vec::new(),
        evidence_ids: Vec::new(),
        detail: None,
        executor: Some(executor.clone()),
    }
}

#[test]
fn leases_grant_exclusive_execution_until_expiry() {
    let graph = simple_graph(
        NodeKind::Deterministic,
        vec![edge("success", "root", "ok", EdgeKind::Success)],
    );
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let claude = upstream("claude-code");
    let codex = upstream("codex");

    let command = claim(&run, "root", &claude, 60);
    let event = apply_command(&mut run, &graph, &command, 10).expect("claim");
    assert_eq!(event.kind, RunEventKind::LeaseClaimed);
    assert!(run.nodes[0].lease.is_some());

    // Anonymous and foreign executors are rejected while the lease is live.
    let anonymous = command_start(&run, "root");
    assert!(matches!(
        apply_command(&mut run, &graph, &anonymous, 20),
        Err(RunError::LeaseHeld { .. })
    ));
    let foreign = start_as(&run, "root", &codex);
    assert!(matches!(
        apply_command(&mut run, &graph, &foreign, 20),
        Err(RunError::LeaseHeld { .. })
    ));
    let steal = claim(&run, "root", &codex, 60);
    assert!(matches!(
        apply_command(&mut run, &graph, &steal, 30),
        Err(RunError::LeaseHeld { .. })
    ));

    let command = start_as(&run, "root", &claude);
    apply_command(&mut run, &graph, &command, 20).expect("holder starts");

    // Expiry is the takeover mechanism: after 10 + 60 seconds anyone may claim.
    let takeover = claim(&run, "root", &codex, 60);
    apply_command(&mut run, &graph, &takeover, 100).expect("expired lease is claimable");
    let command = complete_as(&run, "root", NodeOutcome::Succeeded, &codex);
    apply_command(&mut run, &graph, &command, 110).expect("new holder completes");
    assert_eq!(status(&run, "root"), NodeRunStatus::Succeeded);
    assert!(run.nodes[0].lease.is_none(), "completion clears the lease");
}

#[test]
fn lease_renewal_release_and_bounds() {
    let graph = simple_graph(
        NodeKind::Deterministic,
        vec![edge("success", "root", "ok", EdgeKind::Success)],
    );
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let claude = upstream("claude-code");
    let codex = upstream("codex");

    let too_short = claim(&run, "root", &claude, 1);
    assert!(matches!(
        apply_command(&mut run, &graph, &too_short, 10),
        Err(RunError::InvalidLeaseTtl(1))
    ));
    let command = claim(&run, "root", &claude, 60);
    apply_command(&mut run, &graph, &command, 10).expect("claim");
    let command = claim(&run, "root", &claude, 60);
    apply_command(&mut run, &graph, &command, 40).expect("renew");
    assert_eq!(
        run.nodes[0].lease.as_ref().expect("lease").expires_at,
        100,
        "renewal extends from the renewal time"
    );

    let foreign_release = RunCommand::ReleaseLease {
        expected_revision: run.revision,
        node_id: "root".to_owned(),
        executor: codex.clone(),
    };
    assert!(matches!(
        apply_command(&mut run, &graph, &foreign_release, 50),
        Err(RunError::LeaseHeld { .. })
    ));
    let release = RunCommand::ReleaseLease {
        expected_revision: run.revision,
        node_id: "root".to_owned(),
        executor: claude.clone(),
    };
    let event = apply_command(&mut run, &graph, &release, 50).expect("holder releases");
    assert_eq!(event.kind, RunEventKind::LeaseReleased);
    let command = command_start(&run, "root");
    apply_command(&mut run, &graph, &command, 55).expect("released node is open again");
}

#[test]
fn retry_clears_the_previous_executor_lease() {
    let mut graph = simple_graph(
        NodeKind::Deterministic,
        vec![
            edge("success", "root", "ok", EdgeKind::Success),
            edge("retry-edge", "root", "retry", EdgeKind::Fallback),
        ],
    );
    let mut retry = node("retry", NodeKind::Retry);
    retry.config.insert(
        "targetNodeId".to_owned(),
        serde_json::Value::String("root".to_owned()),
    );
    retry
        .config
        .insert("maxAttempts".to_owned(), serde_json::json!(2));
    graph.nodes[2] = retry;
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let claude = upstream("claude-code");

    let command = claim(&run, "root", &claude, 3_600);
    apply_command(&mut run, &graph, &command, 10).expect("claim");
    let command = start_as(&run, "root", &claude);
    apply_command(&mut run, &graph, &command, 20).expect("start");
    let command = complete_as(&run, "root", NodeOutcome::Failed, &claude);
    apply_command(&mut run, &graph, &command, 30).expect("fail");
    let trigger = RunCommand::TriggerRetry {
        expected_revision: run.revision,
        retry_node_id: "retry".to_owned(),
        reason: "Transient tool timeout".to_owned(),
    };
    apply_command(&mut run, &graph, &trigger, 40).expect("retry");
    assert!(
        run.nodes[0].lease.is_none(),
        "the reopened attempt is not pinned to the previous executor"
    );
    let command = command_start(&run, "root");
    apply_command(&mut run, &graph, &command, 50).expect("anyone may take attempt two");
}

#[test]
fn invalidated_evidence_cannot_be_cited_but_stays_recorded() {
    let graph = simple_graph(
        NodeKind::EvidenceGate,
        vec![edge("success", "root", "ok", EdgeKind::Success)],
    );
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let start = command_start(&run, "root");
    apply(&mut run, &graph, &start);
    let submit = evidence_command(&run, "root", "proof-1");
    apply(&mut run, &graph, &submit);

    let invalidate = RunCommand::InvalidateEvidence {
        expected_revision: run.revision,
        evidence_id: "proof-1".to_owned(),
        actor: "sergii".to_owned(),
        reason: "The cited file changed after submission".to_owned(),
    };
    let event = apply_command(&mut run, &graph, &invalidate, 20).expect("invalidate");
    assert_eq!(event.kind, RunEventKind::EvidenceInvalidated);
    assert!(run.evidence[0].invalidated.is_some(), "record is kept");

    let cite = RunCommand::CompleteNode {
        expected_revision: run.revision,
        node_id: "root".to_owned(),
        outcome: NodeOutcome::Succeeded,
        selected_edge_ids: Vec::new(),
        evidence_ids: vec!["proof-1".to_owned()],
        detail: None,
        executor: None,
    };
    assert!(matches!(
        apply_command(&mut run, &graph, &cite, 30),
        Err(RunError::EvidenceInvalidatedError(id)) if id == "proof-1"
    ));

    let again = RunCommand::InvalidateEvidence {
        expected_revision: run.revision,
        evidence_id: "proof-1".to_owned(),
        actor: "sergii".to_owned(),
        reason: "duplicate".to_owned(),
    };
    assert!(matches!(
        apply_command(&mut run, &graph, &again, 40),
        Err(RunError::EvidenceAlreadyInvalidated(_))
    ));

    let submit = evidence_command(&run, "root", "proof-2");
    apply(&mut run, &graph, &submit);
    let cite_fresh = RunCommand::CompleteNode {
        expected_revision: run.revision,
        node_id: "root".to_owned(),
        outcome: NodeOutcome::Succeeded,
        selected_edge_ids: Vec::new(),
        evidence_ids: vec!["proof-2".to_owned()],
        detail: None,
        executor: None,
    };
    apply_command(&mut run, &graph, &cite_fresh, 50).expect("fresh evidence is citable");
}

#[test]
fn lease_and_invalidation_events_replay_deterministically() {
    let graph = simple_graph(
        NodeKind::Deterministic,
        vec![edge("success", "root", "ok", EdgeKind::Success)],
    );
    let claude = upstream("claude-code");
    let codex = upstream("codex");
    let (mut run, created) = create_run(&graph, "run", 10).expect("create run");
    let mut events = vec![created];

    let command = claim(&run, "root", &claude, 60);
    events.push(apply_command(&mut run, &graph, &command, 15).expect("claim"));
    let command = start_as(&run, "root", &claude);
    events.push(apply_command(&mut run, &graph, &command, 20).expect("start"));
    let command = RunCommand::SubmitEvidence {
        expected_revision: run.revision,
        node_id: "root".to_owned(),
        evidence_id: "proof-1".to_owned(),
        submitted_by: "claude-code".to_owned(),
        source: "graph".to_owned(),
        locator: "node:root".to_owned(),
        digest: None,
        summary: "Bounded graph evidence".to_owned(),
        executor: Some(claude.clone()),
    };
    events.push(apply_command(&mut run, &graph, &command, 25).expect("submit"));
    let command = RunCommand::InvalidateEvidence {
        expected_revision: run.revision,
        evidence_id: "proof-1".to_owned(),
        actor: "sergii".to_owned(),
        reason: "superseded".to_owned(),
    };
    events.push(apply_command(&mut run, &graph, &command, 30).expect("invalidate"));
    // Takeover only replays identically because expiry uses recorded time.
    let command = claim(&run, "root", &codex, 60);
    events.push(apply_command(&mut run, &graph, &command, 90).expect("takeover"));
    let command = complete_as(&run, "root", NodeOutcome::Succeeded, &codex);
    events.push(apply_command(&mut run, &graph, &command, 95).expect("complete"));

    assert_eq!(replay_events(&graph, &events).expect("replay"), run);
}
