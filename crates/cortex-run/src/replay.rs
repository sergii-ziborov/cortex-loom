use cortex_domain::GraphDocument;

use crate::{
    MAX_REPLAY_EVENTS, RunDocument, RunError, RunEvent, RunEventKind, apply_command, create_run,
};

pub fn replay_events(graph: &GraphDocument, events: &[RunEvent]) -> Result<RunDocument, RunError> {
    if events.is_empty() {
        return Err(RunError::ReplayEmpty);
    }
    if events.len() > MAX_REPLAY_EVENTS {
        return Err(RunError::ReplayTooLarge(events.len()));
    }
    let first = &events[0];
    if first.kind != RunEventKind::Created || first.sequence != 1 || first.command.is_some() {
        return Err(mismatch(first.sequence, "first event must be created"));
    }
    if first.run_id.trim().is_empty()
        || first.graph_id != graph.id
        || first.graph_revision != graph.revision
    {
        return Err(mismatch(
            first.sequence,
            "created event does not identify this graph snapshot",
        ));
    }
    let (mut run, created) = create_run(graph, first.run_id.clone(), first.recorded_at)?;
    if created != *first {
        return Err(mismatch(first.sequence, "created event payload differs"));
    }
    for event in &events[1..] {
        if event.sequence != run.revision.saturating_add(1) {
            return Err(mismatch(event.sequence, "event sequence is not contiguous"));
        }
        if event.run_id != run.id
            || event.graph_id != run.graph_id
            || event.graph_revision != run.graph_revision
        {
            return Err(mismatch(
                event.sequence,
                "event identity differs from the created event",
            ));
        }
        let command = event
            .command
            .as_ref()
            .ok_or_else(|| mismatch(event.sequence, "event command is missing"))?;
        let replayed = apply_command(&mut run, graph, command, event.recorded_at)?;
        if replayed != *event {
            return Err(mismatch(
                event.sequence,
                "event payload differs from deterministic replay",
            ));
        }
    }
    Ok(run)
}

fn mismatch(sequence: u64, message: &str) -> RunError {
    RunError::ReplayMismatch {
        sequence,
        message: message.to_owned(),
    }
}
