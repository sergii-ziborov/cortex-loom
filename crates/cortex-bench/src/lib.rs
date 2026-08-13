//! Three-arm context benchmark.
//!
//! The question this crate answers is narrow and stated on purpose, because
//! the broad version cannot be measured honestly here:
//!
//! > For one engineering task on one repository, how many context tokens does
//! > the upstream agent receive, and how many of the facts the task actually
//! > needs are in them?
//!
//! Three arms answer it:
//!
//! * `naive` — no repository intelligence. The fixture hands the arm the
//!   directories a keyword sweep would open and every matching file is read
//!   whole. This is **generous**: a real agent must first find those
//!   directories, and pays for the search. Treat the naive figure as a lower
//!   bound on the cost of not having any of this.
//! * `weavatrix-raw` — Weavatrix evidence with no control plane: every
//!   fragment concatenated, unbudgeted and unordered, exactly as an agent
//!   receives it when an MCP tool result is pasted into the conversation.
//! * `cortex-loom` — the same Weavatrix bundle through
//!   [`cortex_weavatrix::compile_evidence_bundle`] at a declared budget, with
//!   priority ordering and fail-closed critical evidence.
//!
//! ## What this does not measure
//!
//! Task success, answer quality, latency, and the agent's own reasoning
//! tokens. A cheap arm that omits a needed fact is not a win, which is why
//! every arm is scored against declared [`Anchor`]s and the headline figure is
//! tokens per satisfied fact, not raw token count.

mod external_skills;
pub mod manifest;
pub mod naive;
pub mod probe_tasks;
pub mod report;
pub mod schedule;
pub mod scoreboard;
pub mod sequence;
pub mod sequence_arms;
pub mod tasks;

use cortex_context::estimate_tokens;
use serde::{Deserialize, Serialize};

use crate::manifest::{BenchmarkManifest, McpManifest};
use crate::scoreboard::{FailureClass, ScoreboardRow};

/// Default upstream evidence budget, matching the measured `maxTokens`
/// default the adapter usage contract instructs agents to send.
pub const DEFAULT_BUDGET: u32 = 4_000;

/// Which context-assembly strategy produced a measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArmKind {
    /// Read the candidate files whole; no repository intelligence.
    Naive,
    /// Weavatrix evidence with no budget and no priority ordering.
    WeavatrixRaw,
    /// The same operations the planner chose, each under Weavatrix's own
    /// `token_budget`, concatenated with no compiler.
    ///
    /// This is the control that decides whether Cortex Loom contributes
    /// anything at all: if it matches `cortex-targeted`, the saving is
    /// Weavatrix's and the layer above it is only bookkeeping.
    WeavatrixPlanned,
    /// Weavatrix evidence through the bounded, prioritised compiler.
    CortexLoom,
    /// Task-planned Weavatrix operations through the same compiler.
    CortexLoomTargeted,
    /// The same plan with nothing trimmed, so the operations the budget
    /// normally drops are fetched anyway.
    ///
    /// This is the control for the trimming itself: if it delivers no more
    /// facts than `cortex-targeted`, the trimmed operations were dead weight
    /// and dropping them was free.
    CortexLoomFull,
    /// Targeted plan plus bounded `read_source` windows on search-hit files.
    ///
    /// Asks whether identifier-adjacent facts past a search match can be
    /// recovered without paying the naive whole-file cost.
    CortexLoomSource,
}

impl ArmKind {
    /// Stable identifier used in reports and JSON.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Naive => "naive",
            Self::WeavatrixRaw => "weavatrix-raw",
            Self::WeavatrixPlanned => "weavatrix-planned",
            Self::CortexLoom => "cortex-loom",
            Self::CortexLoomTargeted => "cortex-targeted",
            Self::CortexLoomFull => "cortex-full",
            Self::CortexLoomSource => "cortex-source",
        }
    }

    /// One line describing what the arm stands for.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Naive => "candidate files read whole, no repository intelligence",
            Self::WeavatrixRaw => "Weavatrix evidence, unbudgeted and unordered",
            Self::WeavatrixPlanned => "planned operations, Weavatrix's own budget, no compiler",
            Self::CortexLoom => "the same four operations through the bounded compiler",
            Self::CortexLoomTargeted => "operations planned from the task, then compiled",
            Self::CortexLoomFull => "the same plan with nothing trimmed",
            Self::CortexLoomSource => "targeted plan plus bounded read_source on search hits",
        }
    }
}

/// One fact the task cannot be completed without.
///
/// An anchor is satisfied when **any** of its literals appears in an arm's
/// context, compared case-insensitively. Alternatives exist because the same
/// fact is spelled differently by a source file and by a graph tool.
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    /// Short name for the fact, shown in reports.
    pub id: &'static str,
    /// Literals that each independently prove the fact is present.
    pub any_of: &'static [&'static str],
}

impl Anchor {
    #[must_use]
    pub fn is_satisfied_by(&self, haystack_lowercase: &str) -> bool {
        self.any_of
            .iter()
            .any(|needle| haystack_lowercase.contains(&needle.to_ascii_lowercase()))
    }
}

/// What one arm delivered for one task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmMeasurement {
    pub arm: ArmKind,
    /// False when the arm could not run at all, e.g. Weavatrix was absent.
    /// An unavailable arm is reported, never silently dropped or scored zero.
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    pub context_tokens: u32,
    pub context_chars: usize,
    /// Tokens the MCP tool actually serializes for this arm.
    ///
    /// `context_tokens` is the evidence the compiler selected. It is not what
    /// an agent pays: `weavatrix_context_compile` returns the whole
    /// [`cortex_weavatrix::CompiledEvidenceBundle`] as JSON, so the packet is
    /// escaped and carries its own warnings, sufficiency report, citation ids
    /// and counters. Reporting only the inner figure understates the product.
    /// `None` for arms that have no transport form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_tokens: Option<u32>,
    /// Files, fragments, or packet items, depending on the arm.
    pub units: usize,
    /// Retrieval units executed by this in-process arm.
    pub calls: Option<u32>,
    /// End-to-end arm latency, including graph preparation and compilation.
    pub latency_ms: Option<f64>,
    pub satisfied_anchors: Vec<String>,
    pub missing_anchors: Vec<String>,
    /// Whether the producing layer claimed the evidence was sufficient.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sufficient: Option<bool>,
    /// Primary owner of a failed fact, assigned from evidence lineage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<FailureClass>,
    /// Anything the arm wants on the record, such as omitted evidence ids.
    pub notes: Vec<String>,
}

impl ArmMeasurement {
    /// Fraction of declared facts present in this arm's context.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn recall(&self) -> f64 {
        let total = self.satisfied_anchors.len() + self.missing_anchors.len();
        if total == 0 {
            return 1.0;
        }
        self.satisfied_anchors.len() as f64 / total as f64
    }

    /// Context tokens spent per required fact actually delivered.
    ///
    /// `None` when the arm delivered no facts at all: an arm that answers
    /// nothing has no cost per answer, and averaging it in would flatter it.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn tokens_per_fact(&self) -> Option<f64> {
        if !self.available || self.satisfied_anchors.is_empty() {
            return None;
        }
        Some(f64::from(self.context_tokens) / self.satisfied_anchors.len() as f64)
    }

    fn unavailable(arm: ArmKind, reason: String) -> Self {
        Self {
            arm,
            available: false,
            unavailable_reason: Some(reason),
            context_tokens: 0,
            context_chars: 0,
            delivered_tokens: None,
            units: 0,
            calls: Some(0),
            latency_ms: None,
            satisfied_anchors: Vec::new(),
            missing_anchors: Vec::new(),
            sufficient: None,
            failure_class: Some(FailureClass::HarnessBug),
            notes: Vec::new(),
        }
    }

    /// Recompute the failure owner after scoring or adding sufficiency data.
    pub fn refresh_verdict(&mut self) {
        self.failure_class = if self.missing_anchors.is_empty() {
            None
        } else {
            Some(match self.arm {
                ArmKind::Naive => FailureClass::HarnessBug,
                // These are all operation sets, plans, or compiler views
                // selected by the Cortex adapter. Direct engine losslessness
                // is scored separately by source-truth external suites.
                ArmKind::WeavatrixRaw
                | ArmKind::WeavatrixPlanned
                | ArmKind::CortexLoom
                | ArmKind::CortexLoomTargeted
                | ArmKind::CortexLoomFull
                | ArmKind::CortexLoomSource => FailureClass::CortexBug,
            })
        };
    }
}

/// Score one arm's assembled context against the task's declared facts.
#[must_use]
pub fn measure(arm: ArmKind, context: &str, units: usize, anchors: &[Anchor]) -> ArmMeasurement {
    measure_scoped(arm, context, context, units, anchors)
}

/// Score an arm whose sent context contains text that must not count.
///
/// `sent` is everything the upstream agent receives, and it is what tokens
/// are counted on. `scored` is the part that constitutes *evidence*. The
/// compiled packet echoes the task prompt back inside itself, so a prompt
/// that names `MAX_RETRY_ATTEMPTS` would otherwise satisfy that anchor
/// without the evidence system having found anything — the arm would be
/// scored on the question rather than on the answer.
#[must_use]
pub fn measure_scoped(
    arm: ArmKind,
    sent: &str,
    scored: &str,
    units: usize,
    anchors: &[Anchor],
) -> ArmMeasurement {
    let lowercase = scored.to_ascii_lowercase();
    let mut satisfied_anchors = Vec::new();
    let mut missing_anchors = Vec::new();
    for anchor in anchors {
        if anchor.is_satisfied_by(&lowercase) {
            satisfied_anchors.push(anchor.id.to_owned());
        } else {
            missing_anchors.push(anchor.id.to_owned());
        }
    }
    let mut measurement = ArmMeasurement {
        arm,
        available: true,
        unavailable_reason: None,
        context_tokens: estimate_tokens(sent),
        context_chars: sent.chars().count(),
        delivered_tokens: None,
        units,
        calls: None,
        latency_ms: None,
        satisfied_anchors,
        missing_anchors,
        sufficient: None,
        failure_class: None,
        notes: Vec::new(),
    };
    measurement.refresh_verdict();
    measurement
}

/// Record an arm that could not run, with the reason kept on the report.
#[must_use]
pub fn unavailable(arm: ArmKind, reason: impl Into<String>) -> ArmMeasurement {
    ArmMeasurement::unavailable(arm, reason.into())
}

/// Every arm's result for one task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResult {
    pub task_id: String,
    pub prompt: String,
    pub budget: u32,
    pub anchor_count: usize,
    pub arms: Vec<ArmMeasurement>,
}

impl TaskResult {
    #[must_use]
    pub fn arm(&self, kind: ArmKind) -> Option<&ArmMeasurement> {
        self.arms.iter().find(|arm| arm.arm == kind)
    }
}

/// A whole benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchReport {
    pub schema_version: String,
    pub historical: bool,
    pub manifest: BenchmarkManifest,
    pub repository: String,
    pub budget: u32,
    pub trial: usize,
    /// Set when the caller pinned a timestamp; the harness never reads a
    /// clock itself, so a report is reproducible byte for byte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<String>,
    pub tasks: Vec<TaskResult>,
    pub scoreboard: Vec<ScoreboardRow>,
}

impl BenchReport {
    #[must_use]
    pub fn new(
        repository: &std::path::Path,
        budget: u32,
        trial: usize,
        stamp: Option<String>,
        tasks: Vec<TaskResult>,
        command: &[String],
    ) -> Self {
        let scoreboard = context_scoreboard(&tasks, trial);
        let manifest = BenchmarkManifest::detect(
            "context-probe-v2",
            repository,
            command,
            McpManifest::in_process(),
        );
        Self {
            schema_version: manifest.report_schema.clone(),
            historical: false,
            manifest,
            repository: repository.display().to_string(),
            budget,
            trial,
            stamp,
            tasks,
            scoreboard,
        }
    }
}

fn context_scoreboard(tasks: &[TaskResult], trial: usize) -> Vec<ScoreboardRow> {
    let mut rows = Vec::new();
    for task in tasks {
        for arm in &task.arms {
            let mut row = ScoreboardRow::new(
                "deterministic-probe",
                &task.task_id,
                arm.arm.id(),
                trial,
                arm.satisfied_anchors.len(),
                task.anchor_count,
            );
            row.sufficient = arm.sufficient;
            row.failure_class = arm.failure_class;
            row.tokens.selected = Some(arm.context_tokens);
            row.tokens.delivered = arm.delivered_tokens;
            row.calls = arm.calls;
            row.latency_ms = arm.latency_ms;
            row.refresh_verdict();
            rows.push(row);
        }
    }
    rows
}

/// Reduction in context tokens from `from` to `to`, as a fraction.
///
/// `None` when either arm is unavailable or the baseline is empty. A negative
/// value means the second arm cost *more*, which is reported, not hidden.
#[must_use]
pub fn token_delta(from: &ArmMeasurement, to: &ArmMeasurement) -> Option<f64> {
    if !from.available || !to.available || from.context_tokens == 0 {
        return None;
    }
    Some(1.0 - f64::from(to.context_tokens) / f64::from(from.context_tokens))
}

#[cfg(test)]
mod tests;
