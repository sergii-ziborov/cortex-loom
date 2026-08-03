mod command;
mod engine;
mod error;
mod evidence;
mod flow;
mod human;
mod model;
mod replay;
mod retry;
mod transition;

pub use command::RunCommand;
pub use engine::{apply_command, create_run};
pub use error::RunError;
pub use model::{
    EdgeRunState, EdgeRunStatus, EvidenceSubmission, HumanDecision, HumanDecisionRecord,
    NodeOutcome, NodeRunState, NodeRunStatus, ReplayVerification, RunDocument, RunEvent,
    RunEventKind, RunStatus,
};
pub use replay::replay_events;

pub const RUN_SCHEMA_VERSION: &str = "cortex-loom.run.v1";
pub const MAX_RUN_ID_BYTES: usize = 256;
pub const MAX_EVIDENCE_SUBMISSIONS: usize = 4_096;
pub const MAX_EVIDENCE_IDS: usize = 256;
pub const MAX_EVIDENCE_ID_BYTES: usize = 1_024;
pub const MAX_EVIDENCE_SUMMARY_BYTES: usize = 8 * 1024;
pub const MAX_EVIDENCE_FIELD_BYTES: usize = 2 * 1024;
pub const MAX_RUN_DETAIL_BYTES: usize = 16 * 1024;
pub const MAX_REPLAY_EVENTS: usize = 100_000;
pub const MAX_RETRY_ATTEMPTS: u32 = 20;

#[cfg(test)]
mod tests;
