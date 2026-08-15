use serde::{Deserialize, Serialize};

use crate::RunCommand;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunDocument {
    pub schema_version: String,
    pub id: String,
    pub graph_id: String,
    pub graph_revision: u64,
    pub revision: u64,
    pub status: RunStatus,
    pub nodes: Vec<NodeRunState>,
    pub edges: Vec<EdgeRunState>,
    #[serde(default)]
    pub evidence: Vec<EvidenceSubmission>,
    pub created_at: i64,
    pub updated_at: i64,
    /// Workspace identity for later prior-run matching. Not replayed: the
    /// store keeps these on the run row and overlays them on read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    /// External oracle that can credit the run as quality-equivalent.
    /// Absent means the run can be a clean run, never quality-equivalent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle: Option<OracleAttestation>,
}

/// What actually proved the final artifact. Not a run status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OracleAttestation {
    /// `hidden_tests`, `ci`, `review`, or `acceptance`.
    pub kind: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_hash: Option<String>,
    pub attested_by: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NodeRunState {
    pub node_id: String,
    pub status: NodeRunStatus,
    pub attempt: u32,
    #[serde(default)]
    pub activated_by: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub human_decision: Option<HumanDecisionRecord>,
    /// Current executor lease; history lives in the append-only events.
    #[serde(default)]
    pub lease: Option<NodeLeaseState>,
}

/// Who executes work on behalf of a run node. Explicit identity is required
/// to claim a lease; command actors remain free-form audit strings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutorIdentity {
    pub kind: ExecutorKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorKind {
    Human,
    UpstreamAgent,
    LocalModel,
    Service,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NodeLeaseState {
    pub executor: ExecutorIdentity,
    pub claimed_at: i64,
    /// Expiry is evaluated lazily against each command's timestamp, so it is
    /// deterministic under replay; no background timer exists.
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeRunStatus {
    Pending,
    Ready,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRunState {
    pub edge_id: String,
    pub status: EdgeRunStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeRunStatus {
    Dormant,
    Pending,
    Traversed,
    NotTaken,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeOutcome {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceSubmission {
    pub id: String,
    pub node_id: String,
    pub attempt: u32,
    pub submitted_by: String,
    pub source: String,
    pub locator: String,
    #[serde(default)]
    pub digest: Option<String>,
    pub summary: String,
    pub submitted_at: i64,
    /// Recorded invalidation; the submission itself is never deleted, but an
    /// invalidated id can no longer be cited by later commands.
    #[serde(default)]
    pub invalidated: Option<EvidenceInvalidation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceInvalidation {
    pub actor: String,
    pub reason: String,
    pub invalidated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HumanDecision {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HumanDecisionRecord {
    pub decision: HumanDecision,
    pub actor: String,
    pub reason: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    pub decided_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunEvent {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub graph_id: String,
    #[serde(default)]
    pub graph_revision: u64,
    pub sequence: u64,
    pub kind: RunEventKind,
    #[serde(default)]
    pub command: Option<RunCommand>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub edge_ids: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub detail: Option<String>,
    pub run_status: RunStatus,
    pub recorded_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    Created,
    NodeStarted,
    NodeSucceeded,
    NodeFailed,
    EvidenceSubmitted,
    EvidenceInvalidated,
    LeaseClaimed,
    LeaseReleased,
    HumanApproved,
    HumanRejected,
    RetryTriggered,
    OracleAttested,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayVerification {
    pub matches_persisted: bool,
    pub persisted_revision: u64,
    pub replayed_revision: u64,
    pub event_count: usize,
    pub run_status: RunStatus,
}
