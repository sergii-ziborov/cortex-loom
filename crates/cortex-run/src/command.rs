use serde::{Deserialize, Serialize};

use crate::{ExecutorIdentity, HumanDecision, NodeOutcome};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RunCommand {
    StartNode {
        expected_revision: u64,
        node_id: String,
        /// Required to act on a node whose lease another executor holds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executor: Option<ExecutorIdentity>,
    },
    SubmitEvidence {
        expected_revision: u64,
        node_id: String,
        evidence_id: String,
        submitted_by: String,
        source: String,
        locator: String,
        #[serde(default)]
        digest: Option<String>,
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executor: Option<ExecutorIdentity>,
    },
    CompleteNode {
        expected_revision: u64,
        node_id: String,
        outcome: NodeOutcome,
        #[serde(default)]
        selected_edge_ids: Vec<String>,
        #[serde(default)]
        evidence_ids: Vec<String>,
        #[serde(default)]
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executor: Option<ExecutorIdentity>,
    },
    DecideHumanGate {
        expected_revision: u64,
        node_id: String,
        decision: HumanDecision,
        actor: String,
        reason: String,
        #[serde(default)]
        selected_edge_ids: Vec<String>,
        #[serde(default)]
        evidence_ids: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executor: Option<ExecutorIdentity>,
    },
    ClaimLease {
        expected_revision: u64,
        node_id: String,
        executor: ExecutorIdentity,
        ttl_seconds: u32,
    },
    ReleaseLease {
        expected_revision: u64,
        node_id: String,
        executor: ExecutorIdentity,
    },
    InvalidateEvidence {
        expected_revision: u64,
        evidence_id: String,
        actor: String,
        reason: String,
    },
    TriggerRetry {
        expected_revision: u64,
        retry_node_id: String,
        reason: String,
    },
    Cancel {
        expected_revision: u64,
        reason: String,
    },
}

impl RunCommand {
    #[must_use]
    pub const fn expected_revision(&self) -> u64 {
        match self {
            Self::StartNode {
                expected_revision, ..
            }
            | Self::SubmitEvidence {
                expected_revision, ..
            }
            | Self::CompleteNode {
                expected_revision, ..
            }
            | Self::DecideHumanGate {
                expected_revision, ..
            }
            | Self::ClaimLease {
                expected_revision, ..
            }
            | Self::ReleaseLease {
                expected_revision, ..
            }
            | Self::InvalidateEvidence {
                expected_revision, ..
            }
            | Self::TriggerRetry {
                expected_revision, ..
            }
            | Self::Cancel {
                expected_revision, ..
            } => *expected_revision,
        }
    }
}
