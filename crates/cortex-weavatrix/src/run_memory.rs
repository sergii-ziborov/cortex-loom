//! Compile prior Cortex run events into a Weavatrix `memory_context` call.
//!
//! The planner stays independent of `cortex-run`. Callers map their own event
//! types into [`PriorRunEvent`]. Only high-signal failures, rejections,
//! invalidations, retries, and cancellations become facts.

use serde_json::{Value, json};
use weavatrix_rust::memory::{
    AgentId, ContextRequest, EntityId, EventId, EventMetadata, Evidence, FactId, MemoryEvent,
    MemoryFact, MemoryNode, SessionId, StoredEvent, StreamId, Timestamp,
};

const HIGH_SIGNAL: &[&str] = &[
    "node_failed",
    "human_rejected",
    "evidence_invalidated",
    "retry_triggered",
    "cancelled",
];

const MAX_EVENTS: usize = 32;

/// One append-only run event that may become temporal memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorRunEvent {
    pub run_id: String,
    pub sequence: u64,
    pub kind: String,
    pub node_id: Option<String>,
    pub detail: Option<String>,
    /// Unix seconds, same clock as `cortex-run` records.
    pub recorded_at: i64,
}

/// Bounded set of prior-attempt events for one compile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PriorRunMemory {
    pub events: Vec<PriorRunEvent>,
}

impl PriorRunMemory {
    /// Keep only high-signal events, oldest first, hard-capped.
    #[must_use]
    pub fn from_parts(events: Vec<PriorRunEvent>) -> Self {
        let mut kept: Vec<PriorRunEvent> = events.into_iter().filter(is_high_signal).collect();
        kept.sort_by(|left, right| {
            left.recorded_at
                .cmp(&right.recorded_at)
                .then(left.sequence.cmp(&right.sequence))
        });
        kept.truncate(MAX_EVENTS);
        Self { events: kept }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Arguments for Weavatrix `memory_context`, or `None` when nothing to ask.
    #[must_use]
    pub fn memory_arguments(&self, task: &str, token_budget: u32) -> Option<Value> {
        if self.is_empty() {
            return None;
        }
        let stored = stored_events(self, task)
            .ok()
            .filter(|items| !items.is_empty())?;
        let known_at = stored
            .iter()
            .map(|event| event.metadata.recorded_at)
            .max()
            .unwrap_or_else(|| Timestamp::from_unix_micros(1));
        let known_at = Timestamp::from_unix_micros(known_at.as_unix_micros().saturating_add(1));
        let seed = EntityId::new(stable_id("task", task)).ok()?;
        let request = ContextRequest::new(
            vec![seed],
            known_at,
            known_at,
            usize::try_from(token_budget.max(1)).unwrap_or(1),
        )
        .ok()?;
        Some(json!({
            "events": stored,
            "request": request,
        }))
    }
}

fn is_high_signal(event: &PriorRunEvent) -> bool {
    HIGH_SIGNAL.contains(&event.kind.as_str())
}

fn stored_events(
    memory: &PriorRunMemory,
    task: &str,
) -> Result<Vec<StoredEvent<MemoryEvent>>, String> {
    let agent = AgentId::new("agent:cortex-run").map_err(|error| error.to_string())?;
    let task_id = EntityId::new(stable_id("task", task)).map_err(|error| error.to_string())?;
    let mut stored = Vec::new();
    let mut position = 0_u64;
    stored.push(stored_event(
        position,
        "bootstrap",
        0,
        Timestamp::from_unix_micros(1),
        &agent,
        MemoryEvent::NodeUpserted {
            node: MemoryNode::new(task_id.clone(), "task", truncate(task, 160))
                .map_err(|error| error.to_string())?,
        },
    )?);
    position += 1;
    for event in &memory.events {
        let at = Timestamp::from_unix_micros(event.recorded_at.saturating_mul(1_000_000).max(2));
        let attempt = EntityId::new(stable_id(
            "attempt",
            &format!("{}-{}", event.run_id, event.sequence),
        ))
        .map_err(|error| error.to_string())?;
        let label = event
            .detail
            .as_deref()
            .filter(|detail| !detail.trim().is_empty())
            .map_or_else(
                || {
                    format!(
                        "{} {}",
                        event.kind,
                        event.node_id.as_deref().unwrap_or("run")
                    )
                },
                ToOwned::to_owned,
            );
        stored.push(stored_event(
            position,
            &event.run_id,
            event.sequence,
            at,
            &agent,
            MemoryEvent::NodeUpserted {
                node: MemoryNode::new(attempt.clone(), event.kind.as_str(), truncate(&label, 160))
                    .map_err(|error| error.to_string())?,
            },
        )?);
        position += 1;
        let evidence = Evidence::new("run_event", format!("run:{}", event.run_id))
            .map_err(|error| error.to_string())?
            .with_locator(format!("seq:{}", event.sequence));
        let fact = MemoryFact::new(
            FactId::new(stable_id(
                "fact",
                &format!("{}-{}", event.run_id, event.sequence),
            ))
            .map_err(|error| error.to_string())?,
            task_id.clone(),
            event.kind.as_str(),
            attempt,
            at,
            at,
            agent.clone(),
            SessionId::new(stable_id("session", &event.run_id))
                .map_err(|error| error.to_string())?,
            evidence,
        )
        .map_err(|error| error.to_string())?;
        stored.push(stored_event(
            position,
            &event.run_id,
            event.sequence,
            at,
            &agent,
            MemoryEvent::FactRecorded { fact },
        )?);
        position += 1;
    }
    Ok(stored)
}

fn stored_event(
    position: u64,
    run_id: &str,
    _sequence: u64,
    at: Timestamp,
    agent: &AgentId,
    payload: MemoryEvent,
) -> Result<StoredEvent<MemoryEvent>, String> {
    let event_type = payload.event_type().to_owned();
    Ok(StoredEvent {
        metadata: EventMetadata {
            id: EventId::new(stable_id("event", &format!("{run_id}-{position}")))
                .map_err(|error| error.to_string())?,
            stream_id: StreamId::new(stable_id("stream", run_id))
                .map_err(|error| error.to_string())?,
            stream_version: position.saturating_add(1),
            global_position: position.saturating_add(1),
            event_type,
            occurred_at: at,
            recorded_at: at,
            agent_id: agent.clone(),
            session_id: SessionId::new(stable_id("session", run_id))
                .map_err(|error| error.to_string())?,
            correlation_id: None,
            causation_id: None,
        },
        payload,
    })
}

fn stable_id(prefix: &str, raw: &str) -> String {
    let mut body = String::new();
    for character in raw.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
            body.push(character);
        } else if character.is_whitespace() && !body.ends_with('-') {
            body.push('-');
        }
    }
    let body = body.trim_matches('-');
    let body = if body.is_empty() { "item" } else { body };
    let mut id = format!("{prefix}:{body}");
    id.truncate(96);
    id.trim_end_matches(['-', ':']).to_owned()
}

fn truncate(value: &str, max: usize) -> String {
    let taken = value.chars().take(max).collect::<String>();
    if taken.trim().is_empty() {
        return "attempt".to_owned();
    }
    taken
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed() -> PriorRunEvent {
        PriorRunEvent {
            run_id: "run-1".to_owned(),
            sequence: 4,
            kind: "node_failed".to_owned(),
            node_id: Some("root".to_owned()),
            detail: Some("evidence gate rejected".to_owned()),
            recorded_at: 1_700_000_000,
        }
    }

    #[test]
    fn only_high_signal_events_become_memory() {
        let started = PriorRunEvent {
            kind: "node_started".to_owned(),
            sequence: 1,
            ..failed()
        };
        let memory = PriorRunMemory::from_parts(vec![started, failed()]);
        assert_eq!(memory.events.len(), 1);
        assert_eq!(memory.events[0].kind, "node_failed");
    }

    #[test]
    fn memory_arguments_seed_the_task_and_record_the_failure() {
        let memory = PriorRunMemory::from_parts(vec![failed()]);
        let arguments = memory
            .memory_arguments("still failing compile_context", 600)
            .expect("arguments");
        let events = arguments["events"].as_array().expect("events");
        assert!(events.len() >= 3);
        let blob = arguments.to_string();
        assert!(blob.contains("node_failed"));
        assert!(blob.contains("compile_context") || blob.contains("compile-context"));
        assert!(blob.contains("evidence gate rejected"));
        assert_eq!(arguments["request"]["token_budget"], 600);
    }
}
