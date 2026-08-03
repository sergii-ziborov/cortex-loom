use std::fmt::{Display, Formatter};

use cortex_domain::GraphError;

use crate::{
    MAX_EVIDENCE_FIELD_BYTES, MAX_EVIDENCE_ID_BYTES, MAX_EVIDENCE_IDS, MAX_EVIDENCE_SUBMISSIONS,
    MAX_REPLAY_EVENTS, MAX_RETRY_ATTEMPTS, MAX_RUN_DETAIL_BYTES, MAX_RUN_ID_BYTES, NodeRunStatus,
    RunStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    Graph(GraphError),
    EmptyRunId,
    RunIdTooLarge(usize),
    EmptyGraph,
    CyclicFlow,
    GraphMismatch,
    RevisionConflict {
        expected: u64,
        current: u64,
    },
    RunFinished(RunStatus),
    NodeNotFound(String),
    EdgeNotFound(String),
    InvalidNodeState {
        node: String,
        expected: NodeRunStatus,
        current: NodeRunStatus,
    },
    InvalidConditionalSelection(String),
    EvidenceRequired(String),
    TooManyEvidenceIds(usize),
    TooManyEvidenceSubmissions(usize),
    EmptyEvidenceField(&'static str),
    EvidenceIdTooLarge(usize),
    EvidenceFieldTooLarge {
        field: &'static str,
        size: usize,
    },
    DuplicateEvidence(String),
    UnknownEvidence(String),
    EvidenceNodeMismatch {
        id: String,
        node: String,
    },
    EvidenceAttemptMismatch {
        id: String,
        attempt: u32,
    },
    HumanDecisionRequired(String),
    InvalidHumanGate(String),
    RetryCommandRequired(String),
    InvalidRetry(String),
    RetryLimitReached {
        node: String,
        limit: u32,
    },
    RetryLimitTooLarge(u32),
    DetailTooLarge(usize),
    EmptyCancellationReason,
    ReplayEmpty,
    ReplayTooLarge(usize),
    ReplayMismatch {
        sequence: u64,
        message: String,
    },
}

impl Display for RunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EvidenceRequired(_)
            | Self::TooManyEvidenceIds(_)
            | Self::TooManyEvidenceSubmissions(_)
            | Self::EmptyEvidenceField(_)
            | Self::EvidenceIdTooLarge(_)
            | Self::EvidenceFieldTooLarge { .. }
            | Self::DuplicateEvidence(_)
            | Self::UnknownEvidence(_)
            | Self::EvidenceNodeMismatch { .. }
            | Self::EvidenceAttemptMismatch { .. } => self.fmt_evidence(formatter),
            Self::HumanDecisionRequired(_)
            | Self::InvalidHumanGate(_)
            | Self::RetryCommandRequired(_)
            | Self::InvalidRetry(_)
            | Self::RetryLimitReached { .. }
            | Self::RetryLimitTooLarge(_)
            | Self::DetailTooLarge(_)
            | Self::EmptyCancellationReason
            | Self::ReplayEmpty
            | Self::ReplayTooLarge(_)
            | Self::ReplayMismatch { .. } => self.fmt_control(formatter),
            _ => self.fmt_core(formatter),
        }
    }
}

impl RunError {
    fn fmt_core(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Graph(error) => write!(formatter, "run graph is invalid: {error}"),
            Self::EmptyRunId => formatter.write_str("run id must not be empty"),
            Self::RunIdTooLarge(size) => {
                write!(
                    formatter,
                    "run id has {size} bytes; limit is {MAX_RUN_ID_BYTES}"
                )
            }
            Self::EmptyGraph => formatter.write_str("an empty graph cannot be executed"),
            Self::CyclicFlow => {
                formatter.write_str("executable graph edges must be acyclic in run schema v1")
            }
            Self::GraphMismatch => formatter.write_str("run graph snapshot does not match the run"),
            Self::RevisionConflict { expected, current } => {
                write!(
                    formatter,
                    "run revision conflict: expected {expected}, current {current}"
                )
            }
            Self::RunFinished(status) => write!(formatter, "run is already {status:?}"),
            Self::NodeNotFound(id) => write!(formatter, "run node not found: {id}"),
            Self::EdgeNotFound(id) => write!(formatter, "run edge not found: {id}"),
            Self::InvalidNodeState {
                node,
                expected,
                current,
            } => write!(
                formatter,
                "node {node} must be {expected:?}, but is {current:?}"
            ),
            Self::InvalidConditionalSelection(message) => {
                write!(formatter, "invalid conditional transition: {message}")
            }
            _ => unreachable!("non-core run error"),
        }
    }

    fn fmt_evidence(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EvidenceRequired(node) => {
                write!(
                    formatter,
                    "node {node} requires submitted evidence before success"
                )
            }
            Self::TooManyEvidenceIds(count) => {
                write!(
                    formatter,
                    "command has {count} evidence ids; limit is {MAX_EVIDENCE_IDS}"
                )
            }
            Self::TooManyEvidenceSubmissions(count) => write!(
                formatter,
                "run has {count} evidence submissions; limit is {MAX_EVIDENCE_SUBMISSIONS}"
            ),
            Self::EmptyEvidenceField(field) => {
                write!(formatter, "evidence {field} must not be empty")
            }
            Self::EvidenceIdTooLarge(size) => {
                write!(
                    formatter,
                    "evidence id has {size} bytes; limit is {MAX_EVIDENCE_ID_BYTES}"
                )
            }
            Self::EvidenceFieldTooLarge { field, size } => write!(
                formatter,
                "evidence {field} has {size} bytes; limit is {MAX_EVIDENCE_FIELD_BYTES}"
            ),
            Self::DuplicateEvidence(id) => write!(formatter, "evidence id already exists: {id}"),
            Self::UnknownEvidence(id) => write!(formatter, "evidence was not submitted: {id}"),
            Self::EvidenceNodeMismatch { id, node } => {
                write!(formatter, "evidence {id} was not submitted for node {node}")
            }
            Self::EvidenceAttemptMismatch { id, attempt } => {
                write!(
                    formatter,
                    "evidence {id} was not submitted for attempt {attempt}"
                )
            }
            _ => unreachable!("non-evidence run error"),
        }
    }

    fn fmt_control(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HumanDecisionRequired(node) => {
                write!(formatter, "human gate {node} requires decide_human_gate")
            }
            Self::InvalidHumanGate(message) => write!(formatter, "invalid human gate: {message}"),
            Self::RetryCommandRequired(node) => {
                write!(formatter, "retry node {node} requires trigger_retry")
            }
            Self::InvalidRetry(message) => write!(formatter, "invalid retry: {message}"),
            Self::RetryLimitReached { node, limit } => {
                write!(formatter, "node {node} reached retry attempt limit {limit}")
            }
            Self::RetryLimitTooLarge(limit) => {
                write!(
                    formatter,
                    "retry limit {limit} exceeds {MAX_RETRY_ATTEMPTS}"
                )
            }
            Self::DetailTooLarge(size) => {
                write!(
                    formatter,
                    "run detail has {size} bytes; limit is {MAX_RUN_DETAIL_BYTES}"
                )
            }
            Self::EmptyCancellationReason => {
                formatter.write_str("cancellation reason must not be empty")
            }
            Self::ReplayEmpty => formatter.write_str("run replay requires a created event"),
            Self::ReplayTooLarge(count) => {
                write!(
                    formatter,
                    "replay has {count} events; limit is {MAX_REPLAY_EVENTS}"
                )
            }
            Self::ReplayMismatch { sequence, message } => {
                write!(
                    formatter,
                    "replay mismatch at sequence {sequence}: {message}"
                )
            }
            _ => unreachable!("non-control run error"),
        }
    }
}

impl std::error::Error for RunError {}

impl From<GraphError> for RunError {
    fn from(value: GraphError) -> Self {
        Self::Graph(value)
    }
}
