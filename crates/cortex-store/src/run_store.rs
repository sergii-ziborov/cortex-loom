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
        let row = self
            .lock()?
            .query_row(
                "SELECT document, repository_id, snapshot_id FROM runs WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(document, repository_id, snapshot_id)| {
            decode_run(&document, repository_id, snapshot_id)
        })
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
        let rows = if let Some(graph_id) = graph_id {
            let mut statement = connection.prepare(
                "SELECT document, repository_id, snapshot_id FROM runs
                 WHERE graph_id = ?1 ORDER BY updated_at DESC, id ASC LIMIT ?2",
            )?;
            statement
                .query_map(params![graph_id, limit], decode_run_row)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            let mut statement = connection.prepare(
                "SELECT document, repository_id, snapshot_id FROM runs
                 ORDER BY updated_at DESC, id ASC LIMIT ?1",
            )?;
            statement
                .query_map([limit], decode_run_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        rows.into_iter()
            .map(|(document, repository_id, snapshot_id)| {
                decode_run(&document, repository_id, snapshot_id)
            })
            .collect()
    }

    /// Record workspace identity for later prior-run matching.
    ///
    /// These fields live on the run row, not in the replayed document, so a
    /// bind cannot break `verify_replay`.
    pub fn bind_workspace(
        &self,
        id: &str,
        repository_id: Option<&str>,
        snapshot_id: Option<&str>,
    ) -> Result<RunDocument, StoreError> {
        let changed = self.lock()?.execute(
            "UPDATE runs SET repository_id = ?1, snapshot_id = ?2 WHERE id = ?3",
            params![repository_id, snapshot_id, id],
        )?;
        if changed != 1 {
            return Err(StoreError::RunNotFound(id.to_owned()));
        }
        self.get(id)?
            .ok_or_else(|| StoreError::RunNotFound(id.to_owned()))
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

    /// Newest events first from storage, then reversed to chronological order.
    ///
    /// `events()` walks from the start of the stream. Prior-run memory needs
    /// the tail: a long run's first 80 events are usually `Created` and
    /// `NodeStarted`, not the failure that matters.
    pub fn recent_events(&self, id: &str, limit: usize) -> Result<Vec<RunEvent>, StoreError> {
        if self.get(id)?.is_none() {
            return Err(StoreError::RunNotFound(id.to_owned()));
        }
        let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT event FROM run_events
             WHERE run_id = ?1 ORDER BY sequence DESC LIMIT ?2",
        )?;
        let events = statement
            .query_map(params![id, limit], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut parsed = events
            .into_iter()
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .collect::<Result<Vec<RunEvent>, _>>()?;
        parsed.reverse();
        Ok(parsed)
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

fn decode_run_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, Option<String>, Option<String>)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

fn decode_run(
    document: &str,
    repository_id: Option<String>,
    snapshot_id: Option<String>,
) -> Result<RunDocument, StoreError> {
    let mut run: RunDocument = serde_json::from_str(document)?;
    run.repository_id = repository_id;
    run.snapshot_id = snapshot_id;
    Ok(run)
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
#[path = "run_store_tests.rs"]
mod tests;
