use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use cortex_domain::default_control_plane;
use cortex_run::{NodeRunStatus, RunCommand};

use super::*;
use crate::GraphStore;

#[test]
fn run_snapshot_and_events_survive_reload() {
    let graphs = GraphStore::open_in_memory().expect("store");
    let graph = graphs
        .seed_if_missing(&default_control_plane())
        .expect("seed");
    let runs = graphs.runs();
    let created = runs.create("run-1", &graph).expect("create run");
    let started = runs
        .apply(
            "run-1",
            &RunCommand::StartNode {
                expected_revision: created.revision,
                node_id: "request".to_owned(),
                executor: None,
            },
        )
        .expect("start node");

    let loaded = runs.get("run-1").expect("load").expect("run");
    assert_eq!(loaded, started);
    assert_eq!(loaded.nodes[0].status, NodeRunStatus::Running);
    assert_eq!(
        runs.get_graph("run-1")
            .expect("load graph")
            .expect("graph")
            .revision,
        graph.revision
    );
    let events = runs.events("run-1", 0, 10).expect("events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].sequence, 2);
}

#[test]
fn stale_run_update_keeps_snapshot_unchanged() {
    let graphs = GraphStore::open_in_memory().expect("store");
    let graph = graphs
        .seed_if_missing(&default_control_plane())
        .expect("seed");
    let runs = graphs.runs();
    runs.create("run-1", &graph).expect("create run");
    let error = runs
        .apply(
            "run-1",
            &RunCommand::StartNode {
                expected_revision: 0,
                node_id: "request".to_owned(),
                executor: None,
            },
        )
        .expect_err("stale command");
    assert!(matches!(error, StoreError::Run(_)));
    assert_eq!(runs.events("run-1", 0, 10).expect("events").len(), 1);
}

#[test]
fn concurrent_connections_yield_one_transition_and_one_conflict() {
    let path = temporary_database();
    let first = GraphStore::open(&path).expect("first store");
    let graph = first
        .seed_if_missing(&default_control_plane())
        .expect("seed");
    first.runs().create("run-1", &graph).expect("create run");
    let second = GraphStore::open(&path).expect("second store");
    let barrier = Arc::new(Barrier::new(2));

    let first_task = spawn_start(first.runs(), Arc::clone(&barrier));
    let second_task = spawn_start(second.runs(), Arc::clone(&barrier));
    let results = [
        first_task.join().expect("first task"),
        second_task.join().expect("second task"),
    ];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(StoreError::Run(cortex_run::RunError::RevisionConflict {
                    expected: 1,
                    current: 2
                }))
            ))
            .count(),
        1
    );
    assert_eq!(
        first.runs().events("run-1", 0, 10).expect("events").len(),
        2
    );
    drop(first);
    drop(second);
    let _ = std::fs::remove_file(path);
}

#[test]
fn replay_verifies_the_persisted_snapshot() {
    let graphs = GraphStore::open_in_memory().expect("store");
    let graph = graphs
        .seed_if_missing(&default_control_plane())
        .expect("seed");
    let runs = graphs.runs();
    let created = runs.create("run-1", &graph).expect("create run");
    runs.apply(
        "run-1",
        &RunCommand::StartNode {
            expected_revision: created.revision,
            node_id: "request".to_owned(),
            executor: None,
        },
    )
    .expect("start node");
    let verification = runs.verify_replay("run-1").expect("verify replay");
    assert!(verification.matches_persisted);
    assert_eq!(verification.event_count, 2);
}

#[test]
fn replay_rejects_an_event_sequence_gap() {
    let graphs = GraphStore::open_in_memory().expect("store");
    let graph = graphs
        .seed_if_missing(&default_control_plane())
        .expect("seed");
    let runs = graphs.runs();
    let created = runs.create("run-1", &graph).expect("create run");
    let started = runs
        .apply(
            "run-1",
            &RunCommand::StartNode {
                expected_revision: created.revision,
                node_id: "request".to_owned(),
                executor: None,
            },
        )
        .expect("start node");
    runs.apply(
        "run-1",
        &RunCommand::Cancel {
            expected_revision: started.revision,
            reason: "stop".to_owned(),
        },
    )
    .expect("cancel");
    runs.lock()
        .expect("lock")
        .execute(
            "DELETE FROM run_events WHERE run_id = ?1 AND sequence = 2",
            ["run-1"],
        )
        .expect("delete event");
    assert!(matches!(
        runs.verify_replay("run-1"),
        Err(StoreError::Run(cortex_run::RunError::ReplayMismatch {
            sequence: 3,
            ..
        }))
    ));
}

#[test]
fn recent_events_returns_the_tail_in_chronological_order() {
    let graphs = GraphStore::open_in_memory().expect("store");
    let graph = graphs
        .seed_if_missing(&default_control_plane())
        .expect("seed");
    let runs = graphs.runs();
    let created = runs.create("run-1", &graph).expect("create run");
    let started = runs
        .apply(
            "run-1",
            &RunCommand::StartNode {
                expected_revision: created.revision,
                node_id: "request".to_owned(),
                executor: None,
            },
        )
        .expect("start node");
    runs.apply(
        "run-1",
        &RunCommand::Cancel {
            expected_revision: started.revision,
            reason: "stop".to_owned(),
        },
    )
    .expect("cancel");
    let tail = runs.recent_events("run-1", 2).expect("tail");
    assert_eq!(tail.len(), 2);
    assert_eq!(tail[0].sequence, 2);
    assert_eq!(tail[1].sequence, 3);
    assert_eq!(runs.events("run-1", 0, 10).expect("all")[0].sequence, 1);
}

#[test]
fn workspace_bind_survives_reload_and_does_not_break_replay() {
    let graphs = GraphStore::open_in_memory().expect("store");
    let graph = graphs
        .seed_if_missing(&default_control_plane())
        .expect("seed");
    let runs = graphs.runs();
    runs.create("run-1", &graph).expect("create run");
    let bound = runs
        .bind_workspace("run-1", Some("repo:alpha"), Some("snap:1"))
        .expect("bind");
    assert_eq!(bound.repository_id.as_deref(), Some("repo:alpha"));
    assert_eq!(bound.snapshot_id.as_deref(), Some("snap:1"));
    let loaded = runs.get("run-1").expect("load").expect("run");
    assert_eq!(loaded.repository_id, bound.repository_id);
    assert!(
        runs.verify_replay("run-1")
            .expect("verify")
            .matches_persisted
    );
}

fn spawn_start(
    runs: RunStore,
    barrier: Arc<Barrier>,
) -> std::thread::JoinHandle<Result<RunDocument, StoreError>> {
    std::thread::spawn(move || {
        barrier.wait();
        runs.apply(
            "run-1",
            &RunCommand::StartNode {
                expected_revision: 1,
                node_id: "request".to_owned(),
                executor: None,
            },
        )
    })
}

fn temporary_database() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "cortex-loom-run-test-{}-{nonce}.db",
        std::process::id()
    ))
}
