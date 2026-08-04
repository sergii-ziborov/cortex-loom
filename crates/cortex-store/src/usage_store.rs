//! Append-only usage telemetry: the token-accounting ledger from
//! docs/evaluation.md. Records are measurement data with no workflow
//! authority; the store exposes inserts, bounded reads, and one bounded
//! summary — no update or delete surface exists.

use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use super::{StoreError, unix_timestamp};

/// Summaries cover at most this many most-recent samples.
const SUMMARY_WINDOW: usize = 10_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageOperation {
    RouteWork,
    ContextCompile,
}

impl UsageOperation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RouteWork => "route_work",
            Self::ContextCompile => "context_compile",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "route_work" => Some(Self::RouteWork),
            "context_compile" => Some(Self::ContextCompile),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSample {
    pub operation: UsageOperation,
    /// Routing decision fields; `None` for compile samples.
    pub target: Option<String>,
    pub model_tier: Option<String>,
    pub task_class: Option<String>,
    /// Compilation fields; `None` for routing samples.
    pub budget_tokens: Option<u32>,
    pub raw_tokens: Option<u32>,
    pub selected_tokens: Option<u32>,
    pub saved_tokens: Option<u32>,
    pub requires_upstream: Option<bool>,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSampleRow {
    pub id: i64,
    pub created_at: i64,
    #[serde(flatten)]
    pub sample: UsageSample,
}

/// Bounded roll-up over the most recent samples.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub window: u32,
    pub route_calls: u32,
    /// Routing decisions that never reach the upstream agent.
    pub routed_away_from_upstream: u32,
    pub compile_calls: u32,
    pub raw_tokens_total: u64,
    pub selected_tokens_total: u64,
    pub saved_tokens_total: u64,
    pub requires_upstream_count: u32,
    pub compile_latency_p50_ms: u64,
    pub compile_latency_p95_ms: u64,
}

#[derive(Clone)]
pub struct UsageStore {
    connection: Arc<Mutex<Connection>>,
}

impl UsageStore {
    pub(super) const fn new(connection: Arc<Mutex<Connection>>) -> Self {
        Self { connection }
    }

    /// Append one immutable sample and return its row id.
    pub fn insert(&self, sample: &UsageSample) -> Result<i64, StoreError> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO usage_samples
             (created_at, operation, target, model_tier, task_class, budget_tokens,
              raw_tokens, selected_tokens, saved_tokens, requires_upstream, latency_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                unix_timestamp(),
                sample.operation.as_str(),
                sample.target,
                sample.model_tier,
                sample.task_class,
                sample.budget_tokens,
                sample.raw_tokens,
                sample.selected_tokens,
                sample.saved_tokens,
                sample.requires_upstream,
                sample
                    .latency_ms
                    .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    /// Most recent samples, newest first, bounded to 100 rows.
    pub fn list(
        &self,
        operation: Option<UsageOperation>,
        limit: usize,
    ) -> Result<Vec<UsageSampleRow>, StoreError> {
        let limit = i64::try_from(limit.clamp(1, 100)).unwrap_or(100);
        self.query(operation, limit)
    }

    /// Roll up the most recent samples into one bounded summary.
    pub fn summary(&self) -> Result<UsageSummary, StoreError> {
        let window = i64::try_from(SUMMARY_WINDOW).unwrap_or(10_000);
        let rows = self.query(None, window)?;
        let mut summary = UsageSummary {
            window: u32::try_from(rows.len()).unwrap_or(u32::MAX),
            route_calls: 0,
            routed_away_from_upstream: 0,
            compile_calls: 0,
            raw_tokens_total: 0,
            selected_tokens_total: 0,
            saved_tokens_total: 0,
            requires_upstream_count: 0,
            compile_latency_p50_ms: 0,
            compile_latency_p95_ms: 0,
        };
        let mut latencies = Vec::new();
        for row in &rows {
            match row.sample.operation {
                UsageOperation::RouteWork => {
                    summary.route_calls += 1;
                    if row.sample.target.as_deref() != Some("upstream") {
                        summary.routed_away_from_upstream += 1;
                    }
                }
                UsageOperation::ContextCompile => {
                    summary.compile_calls += 1;
                    summary.raw_tokens_total += u64::from(row.sample.raw_tokens.unwrap_or(0));
                    summary.selected_tokens_total +=
                        u64::from(row.sample.selected_tokens.unwrap_or(0));
                    summary.saved_tokens_total += u64::from(row.sample.saved_tokens.unwrap_or(0));
                    if row.sample.requires_upstream == Some(true) {
                        summary.requires_upstream_count += 1;
                    }
                    if let Some(latency) = row.sample.latency_ms {
                        latencies.push(latency);
                    }
                }
            }
        }
        latencies.sort_unstable();
        summary.compile_latency_p50_ms = percentile(&latencies, 50);
        summary.compile_latency_p95_ms = percentile(&latencies, 95);
        Ok(summary)
    }

    fn query(
        &self,
        operation: Option<UsageOperation>,
        limit: i64,
    ) -> Result<Vec<UsageSampleRow>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, created_at, operation, target, model_tier, task_class, budget_tokens,
                    raw_tokens, selected_tokens, saved_tokens, requires_upstream, latency_ms
             FROM usage_samples
             WHERE (?1 IS NULL OR operation = ?1)
             ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = statement
            .query_map(
                params![operation.map(UsageOperation::as_str), limit],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                        row.get::<_, Option<i64>>(8)?,
                        row.get::<_, Option<i64>>(9)?,
                        row.get::<_, Option<bool>>(10)?,
                        row.get::<_, Option<i64>>(11)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| {
                let operation = UsageOperation::parse(&row.2).ok_or_else(|| {
                    StoreError::Database(rusqlite::Error::IntegralValueOutOfRange(0, 0))
                })?;
                let as_u32 = |value: Option<i64>| value.and_then(|value| u32::try_from(value).ok());
                Ok(UsageSampleRow {
                    id: row.0,
                    created_at: row.1,
                    sample: UsageSample {
                        operation,
                        target: row.3,
                        model_tier: row.4,
                        task_class: row.5,
                        budget_tokens: as_u32(row.6),
                        raw_tokens: as_u32(row.7),
                        selected_tokens: as_u32(row.8),
                        saved_tokens: as_u32(row.9),
                        requires_upstream: row.10,
                        latency_ms: row.11.and_then(|value| u64::try_from(value).ok()),
                    },
                })
            })
            .collect()
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

/// Nearest-rank percentile over pre-sorted samples; zero when empty.
fn percentile(sorted: &[u64], percentile: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() * usize::from(percentile))
        .div_ceil(100)
        .max(1);
    sorted[rank - 1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GraphStore;

    fn route_sample(target: &str) -> UsageSample {
        UsageSample {
            operation: UsageOperation::RouteWork,
            target: Some(target.to_owned()),
            model_tier: Some("upstream_strong".to_owned()),
            task_class: Some("implementation".to_owned()),
            budget_tokens: None,
            raw_tokens: None,
            selected_tokens: None,
            saved_tokens: None,
            requires_upstream: None,
            latency_ms: None,
        }
    }

    fn compile_sample(raw: u32, selected: u32, latency: u64) -> UsageSample {
        UsageSample {
            operation: UsageOperation::ContextCompile,
            target: None,
            model_tier: None,
            task_class: None,
            budget_tokens: Some(4_000),
            raw_tokens: Some(raw),
            selected_tokens: Some(selected),
            saved_tokens: Some(raw.saturating_sub(selected)),
            requires_upstream: Some(true),
            latency_ms: Some(latency),
        }
    }

    #[test]
    fn usage_ledger_is_append_only_and_summarizes_savings() {
        let store = GraphStore::open_in_memory().unwrap().usage();
        store.insert(&route_sample("upstream")).unwrap();
        store.insert(&route_sample("deterministic")).unwrap();
        store.insert(&compile_sample(7_500, 3_900, 2_000)).unwrap();
        store.insert(&compile_sample(7_500, 1_900, 1_500)).unwrap();

        let summary = store.summary().unwrap();
        assert_eq!(summary.route_calls, 2);
        assert_eq!(summary.routed_away_from_upstream, 1);
        assert_eq!(summary.compile_calls, 2);
        assert_eq!(summary.raw_tokens_total, 15_000);
        assert_eq!(summary.saved_tokens_total, 3_600 + 5_600);
        assert_eq!(summary.requires_upstream_count, 2);
        assert_eq!(summary.compile_latency_p50_ms, 1_500);

        let rows = store
            .list(Some(UsageOperation::ContextCompile), 10)
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].id > rows[1].id, "newest first");
        let rendered = serde_json::to_string(&rows[0]).expect("rows serialize");
        assert!(rendered.contains("\"savedTokens\""));
    }

    #[test]
    fn listing_is_bounded() {
        let store = GraphStore::open_in_memory().unwrap().usage();
        for _ in 0..5 {
            store.insert(&route_sample("upstream")).unwrap();
        }
        assert_eq!(store.list(None, 3).unwrap().len(), 3);
        assert_eq!(store.list(None, 10_000).unwrap().len(), 5);
    }
}
