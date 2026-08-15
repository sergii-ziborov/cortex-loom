//! Load high-signal Cortex run events for Weavatrix `memory_context`.
//!
//! Until repository/commit/task-signature matching exists, prior memory is
//! loaded only for an explicit `runId`. Scanning recent Failed runs by
//! "previous attempt" wording mixes foreign context into the current task.

use cortex_run::{RunEvent, RunEventKind, RunStatus};
use cortex_store::GraphStore;
use cortex_weavatrix::{PriorRunEvent, PriorRunMemory};

const TAIL_EVENTS: usize = 200;

pub(crate) fn load_prior(store: &GraphStore, run_id: Option<&str>) -> PriorRunMemory {
    let Some(run_id) = run_id.filter(|id| !id.trim().is_empty()) else {
        return PriorRunMemory::default();
    };
    let Ok(Some(run)) = store.runs().get(run_id) else {
        return PriorRunMemory::default();
    };
    if run.status == RunStatus::Running {
        return PriorRunMemory::default();
    }
    match store.runs().recent_events(run_id, TAIL_EVENTS) {
        Ok(loaded) => {
            PriorRunMemory::from_parts(loaded.into_iter().filter_map(from_run_event).collect())
        }
        Err(_) => PriorRunMemory::default(),
    }
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
    use cortex_run::{RunEventKind, RunStatus};

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

    #[test]
    fn missing_run_id_loads_nothing() {
        let store = GraphStore::open_in_memory().unwrap();
        assert!(load_prior(&store, None).is_empty());
        assert!(load_prior(&store, Some("")).is_empty());
        assert!(load_prior(&store, Some("   ")).is_empty());
    }

    #[test]
    fn running_run_is_not_loaded() {
        let store = GraphStore::open_in_memory().unwrap();
        let graph = store
            .seed_if_missing(&cortex_domain::default_control_plane())
            .unwrap();
        store.runs().create("run-open", &graph).unwrap();
        assert!(load_prior(&store, Some("run-open")).is_empty());
    }

    #[test]
    fn completed_run_loads_high_signal_tail() {
        use cortex_run::RunCommand;

        let store = GraphStore::open_in_memory().unwrap();
        let graph = store
            .seed_if_missing(&cortex_domain::default_control_plane())
            .unwrap();
        let created = store.runs().create("run-done", &graph).unwrap();
        let started = store
            .runs()
            .apply(
                "run-done",
                &RunCommand::StartNode {
                    expected_revision: created.revision,
                    node_id: "request".to_owned(),
                    executor: None,
                },
            )
            .unwrap();
        store
            .runs()
            .apply(
                "run-done",
                &RunCommand::Cancel {
                    expected_revision: started.revision,
                    reason: "previous attempt failed the gate".to_owned(),
                },
            )
            .unwrap();
        let memory = load_prior(&store, Some("run-done"));
        assert_eq!(memory.events.len(), 1);
        assert_eq!(memory.events[0].kind, "cancelled");
    }
}
