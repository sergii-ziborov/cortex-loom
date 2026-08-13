//! Common quality, confidence, cost, and latency rows.

use serde::{Deserialize, Serialize};

/// Primary owner of a failed benchmark obligation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureClass {
    WeavatrixBug,
    CortexBug,
    ModelFailure,
    HarnessBug,
    Unclassified,
}

/// Token counters shared by deterministic and live suites.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenCounts {
    pub selected: Option<u32>,
    pub delivered: Option<u32>,
    pub model_prefill: Option<u32>,
    pub model_generation: Option<u32>,
}

/// One task/arm/trial row in the unified scoreboard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreboardRow {
    pub suite: String,
    pub task: String,
    pub arm: String,
    pub trial: usize,
    pub quality_earned: usize,
    pub quality_possible: usize,
    pub sufficient: Option<bool>,
    pub task_success: bool,
    pub false_confidence: bool,
    pub failure_class: Option<FailureClass>,
    pub tokens: TokenCounts,
    pub calls: Option<u32>,
    pub latency_ms: Option<f64>,
    pub artifact: Option<String>,
}

impl ScoreboardRow {
    #[must_use]
    pub fn new(
        suite: impl Into<String>,
        task: impl Into<String>,
        arm: impl Into<String>,
        trial: usize,
        quality_earned: usize,
        quality_possible: usize,
    ) -> Self {
        Self {
            suite: suite.into(),
            task: task.into(),
            arm: arm.into(),
            trial,
            quality_earned,
            quality_possible,
            sufficient: None,
            task_success: quality_earned == quality_possible,
            false_confidence: false,
            failure_class: None,
            tokens: TokenCounts::default(),
            calls: None,
            latency_ms: None,
            artifact: None,
        }
    }

    pub fn refresh_verdict(&mut self) {
        self.false_confidence = self.sufficient == Some(true) && !self.task_success;
        if self.task_success {
            self.failure_class = None;
        }
    }
}

/// Failed task rows must identify a primary owner before publication.
#[must_use]
pub fn has_unclassified_failures(rows: &[ScoreboardRow]) -> bool {
    rows.iter().any(|row| {
        !row.task_success && matches!(row.failure_class, None | Some(FailureClass::Unclassified))
    })
}
