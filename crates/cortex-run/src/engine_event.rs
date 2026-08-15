use cortex_domain::GraphDocument;

use super::{RunCommand, RunDocument, RunError, RunEvent, RunEventKind, RunStatus};

pub(super) struct Applied {
    pub(super) kind: RunEventKind,
    pub(super) node_id: Option<String>,
    pub(super) edge_ids: Vec<String>,
    pub(super) evidence_ids: Vec<String>,
    pub(super) detail: Option<String>,
}

pub(super) fn event(
    run: &RunDocument,
    kind: RunEventKind,
    command: Option<RunCommand>,
    applied: Option<Applied>,
    now: i64,
) -> RunEvent {
    let applied = applied.unwrap_or_else(|| Applied {
        kind,
        node_id: None,
        edge_ids: Vec::new(),
        evidence_ids: Vec::new(),
        detail: None,
    });
    RunEvent {
        run_id: run.id.clone(),
        graph_id: run.graph_id.clone(),
        graph_revision: run.graph_revision,
        sequence: run.revision,
        kind,
        command,
        node_id: applied.node_id,
        edge_ids: applied.edge_ids,
        evidence_ids: applied.evidence_ids,
        detail: applied.detail,
        run_status: run.status,
        recorded_at: now,
    }
}

pub(super) fn validate_command_context(
    run: &RunDocument,
    graph: &GraphDocument,
    command: &RunCommand,
) -> Result<(), RunError> {
    if run.graph_id != graph.id || run.graph_revision != graph.revision {
        return Err(RunError::GraphMismatch);
    }
    if command.expected_revision() != run.revision {
        return Err(RunError::RevisionConflict {
            expected: command.expected_revision(),
            current: run.revision,
        });
    }
    if run.status != RunStatus::Running && !matches!(command, RunCommand::AttestOracle { .. }) {
        return Err(RunError::RunFinished(run.status));
    }
    if matches!(command, RunCommand::AttestOracle { .. }) && run.status == RunStatus::Cancelled {
        return Err(RunError::RunFinished(run.status));
    }
    Ok(())
}

pub(super) fn apply_oracle(
    run: &mut RunDocument,
    kind: &str,
    passed: bool,
    artifact_hash: Option<&str>,
    baseline_hash: Option<&str>,
    attested_by: &str,
    reason: &str,
) -> Applied {
    run.oracle = Some(crate::OracleAttestation {
        kind: kind.to_owned(),
        passed,
        artifact_hash: artifact_hash.map(ToOwned::to_owned),
        baseline_hash: baseline_hash.map(ToOwned::to_owned),
        attested_by: attested_by.to_owned(),
        reason: reason.to_owned(),
    });
    Applied {
        kind: RunEventKind::OracleAttested,
        node_id: None,
        edge_ids: Vec::new(),
        evidence_ids: Vec::new(),
        detail: Some(reason.to_owned()),
    }
}

impl Applied {
    pub(super) fn node(kind: RunEventKind, node_id: &str) -> Self {
        Self {
            kind,
            node_id: Some(node_id.to_owned()),
            edge_ids: Vec::new(),
            evidence_ids: Vec::new(),
            detail: None,
        }
    }
}
