use std::fmt::{Display, Formatter};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use cortex_domain::{GraphDocument, GraphError};
use cortex_run::RunError;
use rusqlite::{Connection, OptionalExtension, params};

mod run_store;
mod shadow_store;
mod usage_store;

pub use run_store::RunStore;
pub use shadow_store::{
    ShadowAggregate, ShadowOperation, ShadowSample, ShadowSampleRow, ShadowStore,
};
pub use usage_store::{
    QualitySummary, RunQuality, UsageOperation, UsageReport, UsageReportRow, UsageSample,
    UsageSampleRow, UsageStore, UsageSummary,
};

#[derive(Clone)]
pub struct GraphStore {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug)]
pub enum StoreError {
    Database(rusqlite::Error),
    Json(serde_json::Error),
    Graph(GraphError),
    Run(RunError),
    LockPoisoned,
    IntegerOutOfRange {
        field: &'static str,
        value: u64,
    },
    InvalidStoredRevision(i64),
    Conflict {
        graph_id: String,
        expected: u64,
        current: u64,
    },
    RunAlreadyExists(String),
    RunNotFound(String),
}

impl Display for StoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "graph database error: {error}"),
            Self::Json(error) => write!(formatter, "graph JSON error: {error}"),
            Self::Graph(error) => write!(formatter, "graph validation error: {error}"),
            Self::Run(error) => write!(formatter, "run transition error: {error}"),
            Self::LockPoisoned => formatter.write_str("graph database lock was poisoned"),
            Self::IntegerOutOfRange { field, value } => {
                write!(
                    formatter,
                    "{field} value {value} exceeds the SQLite integer range"
                )
            }
            Self::InvalidStoredRevision(value) => {
                write!(
                    formatter,
                    "graph database contains an invalid revision: {value}"
                )
            }
            Self::Conflict {
                graph_id,
                expected,
                current,
            } => write!(
                formatter,
                "graph {graph_id} revision conflict: expected {expected}, current {current}"
            ),
            Self::RunAlreadyExists(id) => write!(formatter, "run already exists: {id}"),
            Self::RunNotFound(id) => write!(formatter, "run not found: {id}"),
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

impl From<RunError> for StoreError {
    fn from(value: RunError) -> Self {
        Self::Run(value)
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
             PRAGMA busy_timeout = 5000;
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
             );
             CREATE TABLE IF NOT EXISTS runs (
               id TEXT PRIMARY KEY,
               graph_id TEXT NOT NULL,
               graph_revision INTEGER NOT NULL,
               revision INTEGER NOT NULL,
               document TEXT NOT NULL,
               graph_document TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS runs_by_graph
               ON runs (graph_id, updated_at DESC);
             CREATE TABLE IF NOT EXISTS run_events (
               run_id TEXT NOT NULL,
               sequence INTEGER NOT NULL,
               event TEXT NOT NULL,
               recorded_at INTEGER NOT NULL,
               PRIMARY KEY (run_id, sequence),
               FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS shadow_samples (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               created_at INTEGER NOT NULL,
               operation TEXT NOT NULL CHECK (operation IN
                 ('route_classification','context_compression')),
               model_tag TEXT NOT NULL,
               device TEXT,
               latency_ms INTEGER,
               input_digest TEXT NOT NULL,
               deterministic_summary TEXT NOT NULL,
               shadow_summary TEXT,
               schema_valid INTEGER,
               agreement INTEGER,
               missed_escalation INTEGER NOT NULL DEFAULT 0,
               citation_preserved_ratio REAL,
               hallucinated_citations INTEGER,
               token_estimate_delta INTEGER,
               error TEXT
             );
             CREATE INDEX IF NOT EXISTS shadow_samples_by_group
               ON shadow_samples (operation, model_tag, id DESC);
             CREATE TABLE IF NOT EXISTS usage_samples (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               created_at INTEGER NOT NULL,
               operation TEXT NOT NULL CHECK (operation IN
                 ('route_work','context_compile')),
               run_id TEXT,
               target TEXT,
               model_tier TEXT,
               task_class TEXT,
               budget_tokens INTEGER,
               raw_tokens INTEGER,
               selected_tokens INTEGER,
               omitted_tokens INTEGER,
               requires_upstream INTEGER,
               latency_ms INTEGER
             );
             CREATE TABLE IF NOT EXISTS usage_reports (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               created_at INTEGER NOT NULL,
               run_id TEXT,
               agent TEXT NOT NULL,
               input_tokens INTEGER NOT NULL,
               output_tokens INTEGER NOT NULL,
               note TEXT
             );",
        )?;
        ensure_column(&connection, "usage_samples", "run_id", "run_id TEXT")?;
        // Renamed from `saved_tokens`: the value is omitted-evidence volume,
        // not a measured saving. Older databases keep the stale column; the
        // new one starts empty rather than pretending the old numbers meant
        // something they did not.
        ensure_column(
            &connection,
            "usage_samples",
            "omitted_tokens",
            "omitted_tokens INTEGER",
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
        let revision = sqlite_integer(graph.revision, "revision")?;
        self.lock()?.execute(
            "INSERT OR IGNORE INTO graphs (id, revision, document, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![graph.id, revision, document, now],
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
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;

        let next_revision = if let Some((stored_revision, current_document)) = current {
            let current_revision = u64::try_from(stored_revision)
                .map_err(|_| StoreError::InvalidStoredRevision(stored_revision))?;
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
                    stored_revision,
                    current_document,
                    unix_timestamp()
                ],
            )?;
            current_revision.saturating_add(1)
        } else {
            if graph.revision != 0 {
                return Err(StoreError::Conflict {
                    graph_id: graph.id.clone(),
                    expected: graph.revision,
                    current: 0,
                });
            }
            1
        };

        let mut saved = graph.clone();
        saved.revision = next_revision;
        let document = serde_json::to_string(&saved)?;
        let saved_revision = sqlite_integer(saved.revision, "revision")?;
        transaction.execute(
            "INSERT INTO graphs (id, revision, document, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
               revision = excluded.revision,
               document = excluded.document,
               updated_at = excluded.updated_at",
            params![saved.id, saved_revision, document, unix_timestamp()],
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

    #[must_use]
    pub fn runs(&self) -> RunStore {
        RunStore::new(Arc::clone(&self.connection))
    }

    #[must_use]
    pub fn shadow(&self) -> ShadowStore {
        ShadowStore::new(Arc::clone(&self.connection))
    }

    #[must_use]
    pub fn usage(&self) -> UsageStore {
        UsageStore::new(Arc::clone(&self.connection))
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

fn sqlite_integer(value: u64, field: &'static str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::IntegerOutOfRange { field, value })
}

/// Additive migration for databases created before a column existed.
fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), StoreError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == column);
    if !exists {
        connection.execute(&format!("ALTER TABLE {table} ADD COLUMN {definition}"), [])?;
    }
    Ok(())
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
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
