//! Append-only usage telemetry: the token-accounting ledger from
//! docs/evaluation.md. Records are measurement data with no workflow
//! authority; the store exposes inserts, bounded reads, and one bounded
//! summary — no update or delete surface exists.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use cortex_run::{HumanDecision, RunDocument, RunStatus};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{StoreError, unix_timestamp};

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

/// Quality signals for one run the ledger attributes savings to.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunQuality {
    pub run_id: String,
    /// `None` when the run id was supplied but no such run exists.
    pub status: Option<String>,
    /// Any node needed more than one attempt (repair loops).
    pub retried: bool,
    /// Any human or review gate recorded a rejection.
    pub rejected: bool,
    /// Succeeded with no retries and no rejections — savings on this run are
    /// creditable per docs/evaluation.md.
    pub quality_equivalent: bool,
    pub compile_calls: u32,
    pub selected_tokens: u64,
    pub saved_tokens: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualitySummary {
    pub attributed_runs: u32,
    pub quality_equivalent_runs: u32,
    /// The only savings figure that may be reported as real savings.
    pub quality_equivalent_saved_tokens: u64,
    /// Savings on failed, retried, rejected, or unfinished runs.
    pub unproven_saved_tokens: u64,
    /// Compile samples in the window without a run id.
    pub unattributed_samples: u32,
    pub runs: Vec<RunQuality>,
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
              raw_tokens, selected_tokens, saved_tokens, requires_upstream, latency_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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

    /// Join the ledger with run outcomes: savings count as real only on
    /// succeeded runs without retries or rejections.
    pub fn quality_summary(&self) -> Result<QualitySummary, StoreError> {
        let window = i64::try_from(SUMMARY_WINDOW).unwrap_or(10_000);
        let rows = self.query(Some(UsageOperation::ContextCompile), window)?;
        let mut order: Vec<String> = Vec::new();
        let mut per_run: BTreeMap<String, (u32, u64, u64)> = BTreeMap::new();
        let mut unattributed = 0_usize;
        for row in &rows {
            let Some(run_id) = &row.sample.run_id else {
                unattributed += 1;
                continue;
            };
            if !per_run.contains_key(run_id) {
                order.push(run_id.clone());
            }
            let entry = per_run.entry(run_id.clone()).or_insert((0, 0, 0));
            entry.0 += 1;
            entry.1 += u64::from(row.sample.selected_tokens.unwrap_or(0));
            entry.2 += u64::from(row.sample.saved_tokens.unwrap_or(0));
        }

        let mut summary = QualitySummary {
            attributed_runs: 0,
            quality_equivalent_runs: 0,
            quality_equivalent_saved_tokens: 0,
            unproven_saved_tokens: 0,
            unattributed_samples: u32::try_from(unattributed).unwrap_or(u32::MAX),
            runs: Vec::new(),
        };
        for run_id in order.into_iter().take(MAX_QUALITY_RUNS) {
            let (compile_calls, selected_tokens, saved_tokens) = per_run[&run_id];
            let document = self
                .lock()?
                .query_row(
                    "SELECT document FROM runs WHERE id = ?1",
                    [&run_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let run: Option<RunDocument> = document
                .map(|value| serde_json::from_str(&value))
                .transpose()?;
            let status = run.as_ref().map(|run| status_name(run.status).to_owned());
            let retried = run
                .as_ref()
                .is_some_and(|run| run.nodes.iter().any(|node| node.attempt > 1));
            let rejected = run.as_ref().is_some_and(|run| {
                run.nodes.iter().any(|node| {
                    node.human_decision
                        .as_ref()
                        .is_some_and(|record| record.decision == HumanDecision::Rejected)
                })
            });
            let quality_equivalent = run
                .as_ref()
                .is_some_and(|run| run.status == RunStatus::Succeeded)
                && !retried
                && !rejected;
            summary.attributed_runs += 1;
            if quality_equivalent {
                summary.quality_equivalent_runs += 1;
                summary.quality_equivalent_saved_tokens += saved_tokens;
            } else {
                summary.unproven_saved_tokens += saved_tokens;
            }
            summary.runs.push(RunQuality {
                run_id,
                status,
                retried,
                rejected,
                quality_equivalent,
                compile_calls,
                selected_tokens,
                saved_tokens,
            });
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
                    budget_tokens, raw_tokens, selected_tokens, saved_tokens,
                    requires_upstream, latency_ms
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
                        saved_tokens: as_u32(row.10),
                        requires_upstream: row.11,
                        latency_ms: row.12.and_then(|value| u64::try_from(value).ok()),
                    },
                })
            })
            .collect()
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

fn status_name(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "cancelled",
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
            run_id: None,
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
            run_id: None,
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

    fn attributed(run_id: &str, raw: u32, selected: u32) -> UsageSample {
        UsageSample {
            run_id: Some(run_id.to_owned()),
            ..compile_sample(raw, selected, 5)
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
    fn quality_summary_credits_only_clean_succeeded_runs() {
        use cortex_domain::default_control_plane;
        use cortex_run::{NodeOutcome, RunCommand};

        let graph_store = GraphStore::open_in_memory().unwrap();
        let seeded = graph_store
            .seed_if_missing(&default_control_plane())
            .unwrap();
        let runs = graph_store.runs();

        // A clean succeeded run: walk the default graph to completion.
        let mut clean = runs.create("clean", &seeded).unwrap();
        for node in [
            "request",
            "scan",
            "weavatrix",
            "skill",
            "local",
            "gate",
            "upstream",
            "result",
        ] {
            clean = runs
                .apply(
                    "clean",
                    &RunCommand::StartNode {
                        expected_revision: clean.revision,
                        node_id: node.to_owned(),
                        executor: None,
                    },
                )
                .unwrap();
            clean = runs
                .apply(
                    "clean",
                    &RunCommand::CompleteNode {
                        expected_revision: clean.revision,
                        node_id: node.to_owned(),
                        outcome: NodeOutcome::Succeeded,
                        selected_edge_ids: Vec::new(),
                        evidence_ids: Vec::new(),
                        detail: None,
                        executor: None,
                    },
                )
                .unwrap();
        }
        assert_eq!(clean.status, cortex_run::RunStatus::Succeeded);

        // An unfinished run stays unproven.
        runs.create("open", &seeded).unwrap();

        let usage = graph_store.usage();
        usage.insert(&attributed("clean", 7_500, 1_500)).unwrap();
        usage.insert(&attributed("clean", 7_500, 1_500)).unwrap();
        usage.insert(&attributed("open", 7_500, 1_500)).unwrap();
        usage.insert(&attributed("ghost", 100, 50)).unwrap();
        usage.insert(&compile_sample(100, 50, 1)).unwrap();

        let quality = usage.quality_summary().unwrap();
        assert_eq!(quality.attributed_runs, 3);
        assert_eq!(quality.quality_equivalent_runs, 1);
        assert_eq!(quality.quality_equivalent_saved_tokens, 12_000);
        assert_eq!(quality.unproven_saved_tokens, 6_000 + 50);
        assert_eq!(quality.unattributed_samples, 1);
        let clean_row = quality
            .runs
            .iter()
            .find(|row| row.run_id == "clean")
            .unwrap();
        assert!(clean_row.quality_equivalent && !clean_row.retried && !clean_row.rejected);
        assert_eq!(clean_row.compile_calls, 2);
        let ghost_row = quality
            .runs
            .iter()
            .find(|row| row.run_id == "ghost")
            .unwrap();
        assert_eq!(ghost_row.status, None, "missing runs are never creditable");
        assert!(!ghost_row.quality_equivalent);
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
