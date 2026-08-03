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
    HumanApproved,
    HumanRejected,
    RetryTriggered,
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
