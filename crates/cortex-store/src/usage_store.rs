//! Append-only usage telemetry: the token-accounting ledger from
//! docs/evaluation.md. Records are measurement data with no workflow
//! authority; the store exposes inserts, bounded reads, and one bounded
//! summary Ã¢â‚¬â€ no update or delete surface exists.

use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use super::{StoreError, unix_timestamp};
#[path = "usage_helpers.rs"]
mod usage_helpers;
#[path = "usage_quality.rs"]
mod usage_quality;

use usage_helpers::percentile;
pub use usage_quality::{QualitySummary, RunQuality};

/// Summaries cover at most this many most-recent samples.
const SUMMARY_WINDOW: usize = 10_000;
/// Quality summaries load at most this many distinct runs.
const MAX_QUALITY_RUNS: usize = 50;

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
    /// Run this call was made for, when the caller supplied one; links the
    /// ledger to run outcomes for quality-equivalent accounting.
    pub run_id: Option<String>,
    /// Routing decision fields; `None` for compile samples.
    pub target: Option<String>,
    pub model_tier: Option<String>,
    pub task_class: Option<String>,
    /// Compilation fields; `None` for routing samples.
    pub budget_tokens: Option<u32>,
    pub raw_tokens: Option<u32>,
    pub selected_tokens: Option<u32>,
    pub omitted_tokens: Option<u32>,
    pub requires_upstream: Option<bool>,
    pub latency_ms: Option<u64>,
    /// Split runtime accounting. Absent on older rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_accounting: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSampleRow {
    pub id: i64,
    pub created_at: i64,
    #[serde(flatten)]
    pub sample: UsageSample,
}

/// Upstream-side consumption self-reported by an executor agent. This closes
/// the token balance without access to vendor billing; it is honest
/// self-reporting, not verification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub run_id: Option<String>,
    pub agent: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageReportRow {
    pub id: i64,
    pub created_at: i64,
    #[serde(flatten)]
    pub report: UsageReport,
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
    pub omitted_tokens_total: u64,
    pub requires_upstream_count: u32,
    pub compile_latency_p50_ms: u64,
    pub compile_latency_p95_ms: u64,
    /// Self-reported upstream consumption over the report window.
    pub upstream_reports: u32,
    pub upstream_input_tokens_total: u64,
    pub upstream_output_tokens_total: u64,
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
             (created_at, operation, run_id, target, model_tier, task_class, budget_tokens,
              raw_tokens, selected_tokens, omitted_tokens, requires_upstream, latency_ms,
              token_accounting)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                unix_timestamp(),
                sample.operation.as_str(),
                sample.run_id,
                sample.target,
                sample.model_tier,
                sample.task_class,
                sample.budget_tokens,
                sample.raw_tokens,
                sample.selected_tokens,
                sample.omitted_tokens,
                sample.requires_upstream,
                sample
                    .latency_ms
                    .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                sample.token_accounting,
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

    /// Append one immutable upstream report and return its row id.
    pub fn insert_report(&self, report: &UsageReport) -> Result<i64, StoreError> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO usage_reports
             (created_at, run_id, agent, input_tokens, output_tokens, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                unix_timestamp(),
                report.run_id,
                report.agent,
                i64::try_from(report.input_tokens).unwrap_or(i64::MAX),
                i64::try_from(report.output_tokens).unwrap_or(i64::MAX),
                report.note,
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    /// Most recent upstream reports, newest first, bounded to 100 rows.
    pub fn list_reports(&self, limit: usize) -> Result<Vec<UsageReportRow>, StoreError> {
        let limit = i64::try_from(limit.clamp(1, 100)).unwrap_or(100);
        self.query_reports(limit)
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
            omitted_tokens_total: 0,
            requires_upstream_count: 0,
            compile_latency_p50_ms: 0,
            compile_latency_p95_ms: 0,
            upstream_reports: 0,
            upstream_input_tokens_total: 0,
            upstream_output_tokens_total: 0,
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
                    summary.omitted_tokens_total +=
                        u64::from(row.sample.omitted_tokens.unwrap_or(0));
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
        for row in self.query_reports(window)? {
            summary.upstream_reports += 1;
            summary.upstream_input_tokens_total += row.report.input_tokens;
            summary.upstream_output_tokens_total += row.report.output_tokens;
        }
        Ok(summary)
    }

    fn query(
        &self,
        operation: Option<UsageOperation>,
        limit: i64,
    ) -> Result<Vec<UsageSampleRow>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, created_at, operation, run_id, target, model_tier, task_class,
                    budget_tokens, raw_tokens, selected_tokens, omitted_tokens,
                    requires_upstream, latency_ms, token_accounting
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
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                        row.get::<_, Option<i64>>(8)?,
                        row.get::<_, Option<i64>>(9)?,
                        row.get::<_, Option<i64>>(10)?,
                        row.get::<_, Option<bool>>(11)?,
                        row.get::<_, Option<i64>>(12)?,
                        row.get::<_, Option<String>>(13)?,
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
                        run_id: row.3,
                        target: row.4,
                        model_tier: row.5,
                        task_class: row.6,
                        budget_tokens: as_u32(row.7),
                        raw_tokens: as_u32(row.8),
                        selected_tokens: as_u32(row.9),
                        omitted_tokens: as_u32(row.10),
                        requires_upstream: row.11,
                        latency_ms: row.12.and_then(|value| u64::try_from(value).ok()),
                        token_accounting: row.13,
                    },
                })
            })
            .collect()
    }

    fn query_reports(&self, limit: i64) -> Result<Vec<UsageReportRow>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, created_at, run_id, agent, input_tokens, output_tokens, note
             FROM usage_reports ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = statement
            .query_map([limit], |row| {
                Ok(UsageReportRow {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    report: UsageReport {
                        run_id: row.get(2)?,
                        agent: row.get(3)?,
                        input_tokens: row
                            .get::<_, i64>(4)
                            .map(|value| u64::try_from(value).unwrap_or(0))?,
                        output_tokens: row
                            .get::<_, i64>(5)
                            .map(|value| u64::try_from(value).unwrap_or(0))?,
                        note: row.get(6)?,
                    },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

#[cfg(test)]
#[path = "usage_store_tests.rs"]
mod tests;
