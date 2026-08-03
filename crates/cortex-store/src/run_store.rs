use std::sync::{Arc, Mutex, MutexGuard};

use cortex_domain::GraphDocument;
use cortex_run::{
    ReplayVerification, RunCommand, RunDocument, RunEvent, apply_command, create_run, replay_events,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{StoreError, sqlite_integer, unix_timestamp};

#[derive(Clone)]
pub struct RunStore {
    connection: Arc<Mutex<Connection>>,
}

impl RunStore {
    pub(super) const fn new(connection: Arc<Mutex<Connection>>) -> Self {
        Self { connection }
    }

    pub fn create(&self, id: &str, graph: &GraphDocument) -> Result<RunDocument, StoreError> {
        let now = unix_timestamp();
        let (run, event) = create_run(graph, id, now)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists = transaction
            .query_row("SELECT 1 FROM runs WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some();
        if exists {
            return Err(StoreError::RunAlreadyExists(id.to_owned()));
        }
        let run_json = serde_json::to_string(&run)?;
        let graph_json = serde_json::to_string(graph)?;
        transaction.execute(
            "INSERT INTO runs
             (id, graph_id, graph_revision, revision, document, graph_document, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run.id,
                run.graph_id,
                sqlite_integer(run.graph_revision, "graph_revision")?,
                sqlite_integer(run.revision, "run_revision")?,
                run_json,
                graph_json,
                run.created_at,
                run.updated_at
            ],
        )?;
        insert_event(&transaction, &run.id, &event)?;
        transaction.commit()?;
        Ok(run)
    }

    pub fn get(&self, id: &str) -> Result<Option<RunDocument>, StoreError> {
        let document = self
            .lock()?
            .query_row("SELECT document FROM runs WHERE id = ?1", [id], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        document
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .transpose()
    }

    pub fn get_graph(&self, id: &str) -> Result<Option<GraphDocument>, StoreError> {
        let document = self
            .lock()?
            .query_row(
                "SELECT graph_document FROM runs WHERE id = ?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        document
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .transpose()
    }

    pub fn list(
        &self,
        graph_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RunDocument>, StoreError> {
        let limit = i64::try_from(limit.clamp(1, 100)).unwrap_or(100);
        let connection = self.lock()?;
        let documents = if let Some(graph_id) = graph_id {
            let mut statement = connection.prepare(
                "SELECT document FROM runs
                 WHERE graph_id = ?1 ORDER BY updated_at DESC, id ASC LIMIT ?2",
            )?;
            statement
                .query_map(params![graph_id, limit], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            let mut statement = connection
                .prepare("SELECT document FROM runs ORDER BY updated_at DESC, id ASC LIMIT ?1")?;
            statement
                .query_map([limit], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        documents
            .into_iter()
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .collect()
    }

    pub fn apply(&self, id: &str, command: &RunCommand) -> Result<RunDocument, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored = transaction
            .query_row(
                "SELECT document, graph_document FROM runs WHERE id = ?1",
                [id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::RunNotFound(id.to_owned()))?;
        let mut run: RunDocument = serde_json::from_str(&stored.0)?;
        let graph: GraphDocument = serde_json::from_str(&stored.1)?;
        let event = apply_command(&mut run, &graph, command, unix_timestamp())?;
        let document = serde_json::to_string(&run)?;
        let changed = transaction.execute(
            "UPDATE runs SET revision = ?1, document = ?2, updated_at = ?3
             WHERE id = ?4 AND revision = ?5",
            params![
                sqlite_integer(run.revision, "run_revision")?,
                document,
                run.updated_at,
                id,
                sqlite_integer(command.expected_revision(), "expected_revision")?
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::Run(cortex_run::RunError::RevisionConflict {
                expected: command.expected_revision(),
                current: run.revision.saturating_sub(1),
            }));
        }
        insert_event(&transaction, id, &event)?;
        transaction.commit()?;
        Ok(run)
    }

    pub fn events(&self, id: &str, after: u64, limit: usize) -> Result<Vec<RunEvent>, StoreError> {
        if self.get(id)?.is_none() {
            return Err(StoreError::RunNotFound(id.to_owned()));
        }
        let after = sqlite_integer(after, "event_sequence")?;
        let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT event FROM run_events
             WHERE run_id = ?1 AND sequence > ?2 ORDER BY sequence ASC LIMIT ?3",
        )?;
        let events = statement
            .query_map(params![id, after, limit], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        events
            .into_iter()
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .collect()
    }

    pub fn verify_replay(&self, id: &str) -> Result<ReplayVerification, StoreError> {
        let connection = self.lock()?;
        let stored = connection
            .query_row(
                "SELECT document, graph_document FROM runs WHERE id = ?1",
                [id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::RunNotFound(id.to_owned()))?;
        let persisted: RunDocument = serde_json::from_str(&stored.0)?;
        let graph: GraphDocument = serde_json::from_str(&stored.1)?;
        let mut statement = connection
            .prepare("SELECT event FROM run_events WHERE run_id = ?1 ORDER BY sequence ASC")?;
        let encoded = statement
            .query_map([id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let events = encoded
            .into_iter()
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .collect::<Result<Vec<RunEvent>, _>>()?;
        let replayed = replay_events(&graph, &events)?;
        Ok(ReplayVerification {
            matches_persisted: replayed == persisted,
            persisted_revision: persisted.revision,
            replayed_revision: replayed.revision,
            event_count: events.len(),
            run_status: replayed.status,
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

fn insert_event(connection: &Connection, run_id: &str, event: &RunEvent) -> Result<(), StoreError> {
    connection.execute(
        "INSERT INTO run_events (run_id, sequence, event, recorded_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            run_id,
            sqlite_integer(event.sequence, "event_sequence")?,
            serde_json::to_string(event)?,
            event.recorded_at
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
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
}
