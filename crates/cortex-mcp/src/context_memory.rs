//! Load high-signal Cortex run events for Weavatrix `memory_context`.

use cortex_run::{RunEvent, RunEventKind, RunStatus};
use cortex_store::GraphStore;
use cortex_weavatrix::{PriorRunEvent, PriorRunMemory, asks_for_prior_attempts};

const RECENT_RUNS: usize = 8;
const EVENTS_PER_RUN: u64 = 80;

pub(crate) fn load_prior(store: &GraphStore, run_id: Option<&str>, task: &str) -> PriorRunMemory {
    let mut events = Vec::new();
    if let Some(run_id) = run_id
        && let Ok(loaded) = store.runs().events(run_id, 0, 200)
    {
        events.extend(loaded.into_iter().filter_map(from_run_event));
    }
    if events.is_empty() && asks_for_prior_attempts(task) {
        if let Ok(runs) = store.runs().list(None, RECENT_RUNS) {
            for run in runs {
                if !worth_loading(&run.status) {
                    continue;
                }
                if let Ok(loaded) = store.runs().events(&run.id, 0, EVENTS_PER_RUN as usize) {
                    events.extend(loaded.into_iter().filter_map(from_run_event));
                }
            }
        }
    }
    PriorRunMemory::from_parts(events)
}

fn worth_loading(status: &RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Failed | RunStatus::Cancelled | RunStatus::Running
    )
}

fn from_run_event(event: RunEvent) -> Option<PriorRunEvent> {
    let kind = match event.kind {
        RunEventKind::NodeFailed => "node_failed",
        RunEventKind::HumanRejected => "human_rejected",
        RunEventKind::EvidenceInvalidated => "evidence_invalidated",
        RunEventKind::RetryTriggered => "retry_triggered",
        RunEventKind::Cancelled => "cancelled",
        RunEventKind::Created
        | RunEventKind::NodeStarted
        | RunEventKind::NodeSucceeded
        | RunEventKind::EvidenceSubmitted
        | RunEventKind::LeaseClaimed
        | RunEventKind::LeaseReleased
        | RunEventKind::HumanApproved => return None,
    };
    Some(PriorRunEvent {
        run_id: if event.run_id.is_empty() {
            "run".to_owned()
        } else {
            event.run_id
        },
        sequence: event.sequence,
        kind: kind.to_owned(),
        node_id: event.node_id,
        detail: event.detail,
        recorded_at: event.recorded_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_run::RunEventKind;

    fn event(kind: RunEventKind, detail: &str) -> RunEvent {
        RunEvent {
            run_id: "run-9".to_owned(),
            graph_id: "graph".to_owned(),
            graph_revision: 0,
            sequence: 3,
            kind,
            command: None,
            node_id: Some("gate".to_owned()),
            edge_ids: Vec::new(),
            evidence_ids: Vec::new(),
            detail: Some(detail.to_owned()),
            run_status: RunStatus::Failed,
            recorded_at: 1_700_000_010,
        }
    }

    #[test]
    fn only_failures_and_retries_are_mapped() {
        assert!(from_run_event(event(RunEventKind::NodeStarted, "go")).is_none());
        let failed = from_run_event(event(RunEventKind::NodeFailed, "thin packet")).unwrap();
        assert_eq!(failed.kind, "node_failed");
        assert_eq!(failed.detail.as_deref(), Some("thin packet"));
    }
}
