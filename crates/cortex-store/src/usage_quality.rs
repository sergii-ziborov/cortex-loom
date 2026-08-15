use std::collections::BTreeMap;

use cortex_run::{HumanDecision, RunDocument, RunStatus};
use rusqlite::OptionalExtension;
use serde::Serialize;

use super::usage_helpers::status_name;
use super::{MAX_QUALITY_RUNS, SUMMARY_WINDOW, StoreError, UsageOperation, UsageStore};

/// Quality signals for one run the ledger attributes evidence volume to.
#[allow(clippy::struct_excessive_bools)]
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
    /// Succeeded with no retries and no rejections. A clean walk, not a
    /// quality proof — the suite may still be weak.
    pub clean_run: bool,
    /// Clean run **and** an oracle that passed with an artifact hash.
    pub quality_equivalent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle_kind: Option<String>,
    pub compile_calls: u32,
    pub selected_tokens: u64,
    pub omitted_tokens: u64,
    /// Self-reported upstream consumption attributed to this run.
    pub upstream_reports: u32,
    pub upstream_input_tokens: u64,
    pub upstream_output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualitySummary {
    pub attributed_runs: u32,
    pub clean_runs: u32,
    /// Omitted-evidence volume on clean runs. Still omission volume, not a
    /// measured saving against a baseline.
    pub clean_run_omitted_tokens: u64,
    pub quality_equivalent_runs: u32,
    /// Omitted-evidence volume on oracle-backed quality-equivalent runs.
    pub quality_equivalent_omitted_tokens: u64,
    /// The same volume on failed, retried, rejected, or unfinished runs.
    pub unproven_omitted_tokens: u64,
    /// Compile samples in the window without a run id.
    pub unattributed_samples: u32,
    pub runs: Vec<RunQuality>,
}

impl UsageStore {
    /// Join the ledger with run outcomes. Clean runs and oracle-backed
    /// quality-equivalent runs are counted separately.
    #[allow(clippy::too_many_lines)]
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
            entry.2 += u64::from(row.sample.omitted_tokens.unwrap_or(0));
        }
        let mut per_run_reports: BTreeMap<String, (u32, u64, u64)> = BTreeMap::new();
        for row in self.query_reports(window)? {
            let Some(run_id) = &row.report.run_id else {
                continue;
            };
            if !per_run.contains_key(run_id) && !per_run_reports.contains_key(run_id) {
                order.push(run_id.clone());
            }
            let entry = per_run_reports.entry(run_id.clone()).or_insert((0, 0, 0));
            entry.0 += 1;
            entry.1 += row.report.input_tokens;
            entry.2 += row.report.output_tokens;
        }

        let mut summary = QualitySummary {
            attributed_runs: 0,
            clean_runs: 0,
            clean_run_omitted_tokens: 0,
            quality_equivalent_runs: 0,
            quality_equivalent_omitted_tokens: 0,
            unproven_omitted_tokens: 0,
            unattributed_samples: u32::try_from(unattributed).unwrap_or(u32::MAX),
            runs: Vec::new(),
        };
        for run_id in order.into_iter().take(MAX_QUALITY_RUNS) {
            let (compile_calls, selected_tokens, omitted_tokens) =
                per_run.get(&run_id).copied().unwrap_or((0, 0, 0));
            let (upstream_reports, upstream_input_tokens, upstream_output_tokens) =
                per_run_reports.get(&run_id).copied().unwrap_or((0, 0, 0));
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
            let clean_run = run
                .as_ref()
                .is_some_and(|run| run.status == RunStatus::Succeeded)
                && !retried
                && !rejected;
            let oracle = run.as_ref().and_then(|run| run.oracle.as_ref());
            let quality_equivalent = clean_run
                && oracle.is_some_and(|oracle| oracle.passed && oracle.artifact_hash.is_some());
            summary.attributed_runs += 1;
            if clean_run {
                summary.clean_runs += 1;
                summary.clean_run_omitted_tokens += omitted_tokens;
            }
            if quality_equivalent {
                summary.quality_equivalent_runs += 1;
                summary.quality_equivalent_omitted_tokens += omitted_tokens;
            } else {
                summary.unproven_omitted_tokens += omitted_tokens;
            }
            summary.runs.push(RunQuality {
                run_id,
                status,
                retried,
                rejected,
                clean_run,
                quality_equivalent,
                oracle_kind: oracle.map(|oracle| oracle.kind.clone()),
                compile_calls,
                selected_tokens,
                omitted_tokens,
                upstream_reports,
                upstream_input_tokens,
                upstream_output_tokens,
            });
        }
        Ok(summary)
    }
}
