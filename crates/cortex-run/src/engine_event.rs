use super::{RunCommand, RunDocument, RunEvent, RunEventKind};

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
