//! Neutral hints supplied by an active workflow skill.
//!
//! The skill compiler and transports do not belong in evidence planning.
//! They may translate their own metadata into this small value, while the
//! deterministic planner remains usable without MCP, UI, or a model.

use serde::{Deserialize, Serialize};

use crate::plan_intent::TaskIntent;

/// An explicit task intent supplied by a workflow.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntentHint {
    IdentifierChange,
    BlastRadius,
    ApiContract,
    ModuleTopology,
    RuntimeConfig,
}

impl From<IntentHint> for TaskIntent {
    fn from(value: IntentHint) -> Self {
        match value {
            IntentHint::IdentifierChange => Self::IdentifierChange,
            IntentHint::BlastRadius => Self::BlastRadius,
            IntentHint::ApiContract => Self::ApiContract,
            IntentHint::ModuleTopology => Self::ModuleTopology,
            IntentHint::RuntimeConfig => Self::RuntimeConfig,
        }
    }
}

/// Deterministic evidence-planning controls supplied by the active skill.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanHints {
    /// Override the lightweight intent classifier when the workflow knows the
    /// evidence shape more precisely than the task prose does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentHint>,
    /// Request or suppress bounded `read_source` follow-up. `None` leaves the
    /// caller's default unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_followup: Option<bool>,
    /// Never ask for an unverified change plan, even if the task wording asks
    /// for one. Useful for gather/verify-only skills.
    #[serde(default)]
    pub skip_change_plan: bool,
}

impl PlanHints {
    #[must_use]
    pub(crate) fn intent_or_detect(self, task: &str) -> TaskIntent {
        self.intent
            .map_or_else(|| crate::plan_intent::detect(task), TaskIntent::from)
    }

    #[must_use]
    pub const fn source_followup_or(self, default: bool) -> bool {
        match self.source_followup {
            Some(value) => value,
            None => default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skill_can_override_all_three_planning_controls() {
        let hints = PlanHints {
            intent: Some(IntentHint::RuntimeConfig),
            source_followup: Some(true),
            skip_change_plan: true,
        };
        assert_eq!(
            hints.intent_or_detect("ordinary prose"),
            TaskIntent::RuntimeConfig
        );
        assert!(hints.source_followup_or(false));
        assert!(hints.skip_change_plan);
    }
}
