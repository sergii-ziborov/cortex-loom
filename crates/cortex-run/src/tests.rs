use cortex_domain::{EdgeKind, GraphDocument, NodeKind, default_control_plane};

use super::*;
use crate::tests_leases::{edge, evidence_command, node, simple_graph};

pub(crate) fn command_start(run: &RunDocument, node_id: &str) -> RunCommand {
    RunCommand::StartNode {
        expected_revision: run.revision,
        node_id: node_id.to_owned(),
        executor: None,
    }
}

pub(crate) fn command_complete(
    run: &RunDocument,
    node_id: &str,
    outcome: NodeOutcome,
) -> RunCommand {
    RunCommand::CompleteNode {
        expected_revision: run.revision,
        node_id: node_id.to_owned(),
        outcome,
        selected_edge_ids: Vec::new(),
        evidence_ids: Vec::new(),
        detail: None,
        executor: None,
    }
}

pub(crate) fn apply(run: &mut RunDocument, graph: &GraphDocument, command: &RunCommand) {
    apply_command(run, graph, command, 10).expect("apply command");
}

pub(crate) fn status(run: &RunDocument, node_id: &str) -> NodeRunStatus {
    run.nodes
        .iter()
        .find(|node| node.node_id == node_id)
        .expect("node state")
        .status
}

#[test]
fn default_graph_starts_at_the_single_input_root() {
    let graph = default_control_plane();
    let (run, event) = create_run(&graph, "run-1", 10).expect("create run");
    assert_eq!(run.revision, 1);
    assert_eq!(event.kind, RunEventKind::Created);
    assert_eq!(status(&run, "request"), NodeRunStatus::Ready);
    assert_eq!(status(&run, "scan"), NodeRunStatus::Pending);
    assert_eq!(status(&run, "weavatrix"), NodeRunStatus::Pending);
}

#[test]
fn successful_edges_activate_parallel_nodes_and_join() {
    let graph = default_control_plane();
    let (mut run, _) = create_run(&graph, "run-1", 10).expect("create run");
    let start = command_start(&run, "request");
    apply(&mut run, &graph, &start);
    let complete = command_complete(&run, "request", NodeOutcome::Succeeded);
    apply(&mut run, &graph, &complete);
    assert_eq!(status(&run, "scan"), NodeRunStatus::Ready);
    assert_eq!(status(&run, "weavatrix"), NodeRunStatus::Ready);

    let start = command_start(&run, "scan");
    apply(&mut run, &graph, &start);
    let complete = command_complete(&run, "scan", NodeOutcome::Succeeded);
    apply(&mut run, &graph, &complete);
    assert_eq!(status(&run, "skill"), NodeRunStatus::Ready);

    let start = command_start(&run, "skill");
    apply(&mut run, &graph, &start);
    let complete = command_complete(&run, "skill", NodeOutcome::Succeeded);
    apply(&mut run, &graph, &complete);
    assert_eq!(status(&run, "gate"), NodeRunStatus::Pending);

    let start = command_start(&run, "weavatrix");
    apply(&mut run, &graph, &start);
    let complete = command_complete(&run, "weavatrix", NodeOutcome::Succeeded);
    apply(&mut run, &graph, &complete);
    let start = command_start(&run, "local");
    apply(&mut run, &graph, &start);
    let complete = command_complete(&run, "local", NodeOutcome::Succeeded);
    apply(&mut run, &graph, &complete);
    assert_eq!(status(&run, "gate"), NodeRunStatus::Ready);
}

#[test]
fn failure_takes_fallback_and_marks_success_path_not_taken() {
    let graph = simple_graph(
        NodeKind::Deterministic,
        vec![
            edge("success", "root", "ok", EdgeKind::Success),
            edge("fallback", "root", "recovery", EdgeKind::Fallback),
        ],
    );
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let start = command_start(&run, "root");
    apply(&mut run, &graph, &start);
    let complete = command_complete(&run, "root", NodeOutcome::Failed);
    apply(&mut run, &graph, &complete);
    assert_eq!(status(&run, "ok"), NodeRunStatus::Skipped);
    assert_eq!(status(&run, "recovery"), NodeRunStatus::Ready);
}

#[test]
fn recovered_failure_finishes_after_the_active_sink_succeeds() {
    let graph = simple_graph(
        NodeKind::Deterministic,
        vec![
            edge("success", "root", "ok", EdgeKind::Success),
            edge("fallback", "root", "recovery", EdgeKind::Fallback),
        ],
    );
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let start = command_start(&run, "root");
    apply(&mut run, &graph, &start);
    let complete = command_complete(&run, "root", NodeOutcome::Failed);
    apply(&mut run, &graph, &complete);
    let start = command_start(&run, "recovery");
    apply(&mut run, &graph, &start);
    let complete = command_complete(&run, "recovery", NodeOutcome::Succeeded);
    apply(&mut run, &graph, &complete);
    assert_eq!(run.status, RunStatus::Succeeded);
}

#[test]
fn successful_evidence_gates_require_a_citation() {
    let graph = simple_graph(
        NodeKind::EvidenceGate,
        vec![
            edge("success", "root", "ok", EdgeKind::Success),
            edge("fallback", "root", "recovery", EdgeKind::Fallback),
        ],
    );
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let start = command_start(&run, "root");
    apply(&mut run, &graph, &start);
    let complete = command_complete(&run, "root", NodeOutcome::Succeeded);
    assert_eq!(
        apply_command(&mut run, &graph, &complete, 2),
        Err(RunError::EvidenceRequired("root".to_owned()))
    );
    assert_eq!(status(&run, "root"), NodeRunStatus::Running);
    let submit = evidence_command(&run, "root", "proof-1");
    apply(&mut run, &graph, &submit);
    let cited = RunCommand::CompleteNode {
        expected_revision: run.revision,
        node_id: "root".to_owned(),
        outcome: NodeOutcome::Succeeded,
        selected_edge_ids: Vec::new(),
        evidence_ids: vec!["proof-1".to_owned()],
        detail: None,
        executor: None,
    };
    apply(&mut run, &graph, &cited);
    assert_eq!(status(&run, "root"), NodeRunStatus::Succeeded);
    assert_eq!(run.nodes[0].evidence_ids, vec!["proof-1"]);
}

#[test]
fn branch_requires_one_explicit_conditional_transition() {
    let graph = simple_graph(
        NodeKind::Branch,
        vec![
            edge("left", "root", "ok", EdgeKind::Conditional),
            edge("right", "root", "recovery", EdgeKind::Conditional),
        ],
    );
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let start = command_start(&run, "root");
    apply(&mut run, &graph, &start);
    let missing = command_complete(&run, "root", NodeOutcome::Succeeded);
    assert!(matches!(
        apply_command(&mut run, &graph, &missing, 2),
        Err(RunError::InvalidConditionalSelection(_))
    ));

    let selected = RunCommand::CompleteNode {
        expected_revision: run.revision,
        node_id: "root".to_owned(),
        outcome: NodeOutcome::Succeeded,
        selected_edge_ids: vec!["right".to_owned()],
        evidence_ids: Vec::new(),
        detail: None,
        executor: None,
    };
    apply(&mut run, &graph, &selected);
    assert_eq!(status(&run, "ok"), NodeRunStatus::Skipped);
    assert_eq!(status(&run, "recovery"), NodeRunStatus::Ready);
}

#[test]
fn stale_commands_are_rejected_without_mutation() {
    let graph = default_control_plane();
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let command = RunCommand::StartNode {
        expected_revision: 0,
        node_id: "request".to_owned(),
        executor: None,
    };
    assert_eq!(
        apply_command(&mut run, &graph, &command, 2),
        Err(RunError::RevisionConflict {
            expected: 0,
            current: 1
        })
    );
    assert_eq!(status(&run, "request"), NodeRunStatus::Ready);
}

#[test]
fn command_json_uses_the_public_camel_case_contract() {
    let command = RunCommand::StartNode {
        expected_revision: 7,
        node_id: "root".to_owned(),
        executor: None,
    };
    assert_eq!(
        serde_json::to_value(command).expect("serialize command"),
        serde_json::json!({
            "action": "start_node",
            "expectedRevision": 7,
            "nodeId": "root"
        })
    );
}

#[test]
fn cyclic_executable_flow_is_rejected() {
    let mut graph = simple_graph(
        NodeKind::Deterministic,
        vec![
            edge("one", "root", "ok", EdgeKind::Sequence),
            edge("two", "ok", "root", EdgeKind::Sequence),
        ],
    );
    graph.nodes.pop();
    assert_eq!(create_run(&graph, "run", 0), Err(RunError::CyclicFlow));
}

#[test]
fn evidence_is_immutable_and_scoped_to_the_current_attempt() {
    let graph = simple_graph(
        NodeKind::EvidenceGate,
        vec![edge("success", "root", "ok", EdgeKind::Success)],
    );
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let start = command_start(&run, "root");
    apply(&mut run, &graph, &start);
    let submit = evidence_command(&run, "root", "proof-1");
    apply(&mut run, &graph, &submit);
    assert_eq!(run.evidence[0].attempt, 1);
    assert_eq!(
        apply_command(&mut run, &graph, &submit, 11),
        Err(RunError::RevisionConflict {
            expected: 2,
            current: 3
        })
    );

    let duplicate = RunCommand::SubmitEvidence {
        expected_revision: run.revision,
        node_id: "root".to_owned(),
        evidence_id: "proof-1".to_owned(),
        submitted_by: "weavatrix".to_owned(),
        source: "graph".to_owned(),
        locator: "node:root".to_owned(),
        digest: None,
        summary: "duplicate".to_owned(),
        executor: None,
    };
    assert_eq!(
        apply_command(&mut run, &graph, &duplicate, 12),
        Err(RunError::DuplicateEvidence("proof-1".to_owned()))
    );
}

#[test]
fn human_gate_requires_a_typed_audited_decision() {
    let graph = simple_graph(
        NodeKind::HumanGate,
        vec![
            edge("approved", "root", "ok", EdgeKind::Success),
            edge("rejected", "root", "recovery", EdgeKind::Fallback),
        ],
    );
    let (mut run, _) = create_run(&graph, "run", 0).expect("create run");
    let start = command_start(&run, "root");
    apply(&mut run, &graph, &start);
    let generic = command_complete(&run, "root", NodeOutcome::Succeeded);
    assert_eq!(
        apply_command(&mut run, &graph, &generic, 2),
        Err(RunError::HumanDecisionRequired("root".to_owned()))
    );
    let decision = RunCommand::DecideHumanGate {
        expected_revision: run.revision,
        node_id: "root".to_owned(),
        decision: HumanDecision::Rejected,
        actor: "sergii".to_owned(),
        reason: "Tests do not cover the mutation".to_owned(),
        selected_edge_ids: Vec::new(),
        evidence_ids: Vec::new(),
        executor: None,
    };
    let event = apply_command(&mut run, &graph, &decision, 3).expect("reject gate");
    assert_eq!(event.kind, RunEventKind::HumanRejected);
    assert_eq!(status(&run, "root"), NodeRunStatus::Failed);
    assert_eq!(status(&run, "recovery"), NodeRunStatus::Ready);
    assert_eq!(
        run.nodes[0]
            .human_decision
            .as_ref()
            .expect("decision")
            .actor,
        "sergii"
    );
}

#[test]
fn retry_reopens_only_the_bounded_forward_path() {
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

    let start = command_start(&run, "root");
    apply(&mut run, &graph, &start);
    let fail = command_complete(&run, "root", NodeOutcome::Failed);
    apply(&mut run, &graph, &fail);
    assert_eq!(status(&run, "retry"), NodeRunStatus::Ready);
    let retry = RunCommand::TriggerRetry {
        expected_revision: run.revision,
        retry_node_id: "retry".to_owned(),
        reason: "Transient tool timeout".to_owned(),
    };
    apply(&mut run, &graph, &retry);
    assert_eq!(status(&run, "root"), NodeRunStatus::Ready);
    assert_eq!(status(&run, "retry"), NodeRunStatus::Pending);

    let start = command_start(&run, "root");
    apply(&mut run, &graph, &start);
    let fail = command_complete(&run, "root", NodeOutcome::Failed);
    apply(&mut run, &graph, &fail);
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(status(&run, "retry"), NodeRunStatus::Skipped);
    assert_eq!(
        run.nodes
            .iter()
            .find(|state| state.node_id == "root")
            .expect("root")
            .attempt,
        2
    );
}

#[test]
fn retry_rejects_incoming_transitions_from_other_nodes() {
    let mut graph = simple_graph(
        NodeKind::Deterministic,
        vec![
            edge("retry-edge", "root", "recovery", EdgeKind::Fallback),
            edge("other-edge", "ok", "recovery", EdgeKind::Success),
        ],
    );
    graph.nodes[2].kind = NodeKind::Retry;
    graph.nodes[2].config.insert(
        "targetNodeId".to_owned(),
        serde_json::Value::String("root".to_owned()),
    );
    graph.nodes[2]
        .config
        .insert("maxAttempts".to_owned(), serde_json::json!(2));
    assert!(matches!(
        create_run(&graph, "run", 0),
        Err(RunError::InvalidRetry(message))
            if message.contains("only failure transitions")
    ));
}

#[test]
fn event_stream_replays_to_the_exact_snapshot_and_rejects_tampering() {
    let graph = simple_graph(
        NodeKind::Deterministic,
        vec![edge("success", "root", "ok", EdgeKind::Success)],
    );
    let (mut run, created) = create_run(&graph, "run", 10).expect("create run");
    let mut events = vec![created];
    let start = command_start(&run, "root");
    events.push(apply_command(&mut run, &graph, &start, 11).expect("start"));
    let complete = command_complete(&run, "root", NodeOutcome::Succeeded);
    events.push(apply_command(&mut run, &graph, &complete, 12).expect("complete"));
    assert_eq!(replay_events(&graph, &events).expect("replay"), run);

    events[2].sequence = 4;
    assert!(matches!(
        replay_events(&graph, &events),
        Err(RunError::ReplayMismatch { sequence: 4, .. })
    ));
}
