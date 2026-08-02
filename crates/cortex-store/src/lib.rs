use std::fmt::{Display, Formatter};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use cortex_domain::{GraphDocument, GraphError};
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone)]
pub struct GraphStore {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug)]
pub enum StoreError {
    Database(rusqlite::Error),
    Json(serde_json::Error),
    Graph(GraphError),
    LockPoisoned,
    Conflict {
        graph_id: String,
        expected: u64,
        current: u64,
    },
}

impl Display for StoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "graph database error: {error}"),
            Self::Json(error) => write!(formatter, "graph JSON error: {error}"),
            Self::Graph(error) => write!(formatter, "graph validation error: {error}"),
            Self::LockPoisoned => formatter.write_str("graph database lock was poisoned"),
            Self::Conflict {
                graph_id,
                expected,
                current,
            } => write!(
                formatter,
                "graph {graph_id} revision conflict: expected {expected}, current {current}"
            ),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<GraphError> for StoreError {
    fn from(value: GraphError) -> Self {
        Self::Graph(value)
    }
}

impl GraphStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, StoreError> {
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS graphs (
               id TEXT PRIMARY KEY,
               revision INTEGER NOT NULL,
               document TEXT NOT NULL,
               updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS graph_history (
               graph_id TEXT NOT NULL,
               revision INTEGER NOT NULL,
               document TEXT NOT NULL,
               archived_at INTEGER NOT NULL,
               PRIMARY KEY (graph_id, revision)
             );",
        )?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn seed_if_missing(&self, graph: &GraphDocument) -> Result<GraphDocument, StoreError> {
        graph.validate()?;
        if let Some(current) = self.get(&graph.id)? {
            return Ok(current);
        }
        let document = serde_json::to_string(graph)?;
        let now = unix_timestamp();
        self.lock()?.execute(
            "INSERT OR IGNORE INTO graphs (id, revision, document, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![graph.id, graph.revision, document, now],
        )?;
        self.get(&graph.id)?
            .ok_or_else(|| StoreError::Database(rusqlite::Error::QueryReturnedNoRows))
    }

    pub fn get(&self, id: &str) -> Result<Option<GraphDocument>, StoreError> {
        let document = self
            .lock()?
            .query_row("SELECT document FROM graphs WHERE id = ?1", [id], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        document
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .transpose()
    }

    pub fn save(&self, graph: &GraphDocument) -> Result<GraphDocument, StoreError> {
        graph.validate()?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let current = transaction
            .query_row(
                "SELECT revision, document FROM graphs WHERE id = ?1",
                [&graph.id],
                |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;

        let next_revision = match current {
            Some((current_revision, current_document)) => {
                if graph.revision != current_revision {
                    return Err(StoreError::Conflict {
                        graph_id: graph.id.clone(),
                        expected: graph.revision,
                        current: current_revision,
                    });
                }
                transaction.execute(
                    "INSERT OR IGNORE INTO graph_history
                     (graph_id, revision, document, archived_at) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        graph.id,
                        current_revision,
                        current_document,
                        unix_timestamp()
                    ],
                )?;
                current_revision.saturating_add(1)
            }
            None => {
                if graph.revision != 0 {
                    return Err(StoreError::Conflict {
                        graph_id: graph.id.clone(),
                        expected: graph.revision,
                        current: 0,
                    });
                }
                1
            }
        };

        let mut saved = graph.clone();
        saved.revision = next_revision;
        let document = serde_json::to_string(&saved)?;
        transaction.execute(
            "INSERT INTO graphs (id, revision, document, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
               revision = excluded.revision,
               document = excluded.document,
               updated_at = excluded.updated_at",
            params![saved.id, saved.revision, document, unix_timestamp()],
        )?;
        transaction.commit()?;
        Ok(saved)
    }

    pub fn list(&self) -> Result<Vec<GraphDocument>, StoreError> {
        let connection = self.lock()?;
        let mut statement =
            connection.prepare("SELECT document FROM graphs ORDER BY updated_at DESC, id ASC")?;
        let documents = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        documents
            .into_iter()
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .collect()
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_domain::default_control_plane;

    #[test]
    fn save_uses_optimistic_revision_and_keeps_history() {
        let store = GraphStore::open_in_memory().expect("in-memory store");
        let seeded = store
            .seed_if_missing(&default_control_plane())
            .expect("seed graph");
        let mut changed = seeded.clone();
        changed.name = "Changed".to_owned();
        let saved = store.save(&changed).expect("save graph");
        assert_eq!(saved.revision, seeded.revision + 1);

        let error = store.save(&changed).expect_err("stale write must fail");
        assert!(matches!(error, StoreError::Conflict { current: 2, .. }));
    }

    #[test]
    fn a_new_graph_must_start_at_revision_zero() {
        let store = GraphStore::open_in_memory().expect("in-memory store");
        let mut graph = default_control_plane();
        graph.id = "new".to_owned();
        graph.revision = 0;
        assert_eq!(store.save(&graph).expect("create graph").revision, 1);
    }
}
