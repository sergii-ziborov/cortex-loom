//! Compile prior Cortex run events into a Weavatrix `memory_context` call.
//!
//! The planner stays independent of `cortex-run`. Callers map their own event
//! types into [`PriorRunEvent`]. Only high-signal failures, rejections,
//! invalidations, retries, and cancellations become facts.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use weavatrix_rust::memory::{
    AgentId, ContextRequest, EntityId, EventId, EventStore, Evidence, ExpectedVersion, FactId,
    InMemoryStore, MemoryEvent, MemoryFact, MemoryNode, NewEvent, SessionId, StoredEvent, StreamId,
    Timestamp,
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
    /// Keep the newest high-signal events, then restore chronological order.
    #[must_use]
    pub fn from_parts(events: Vec<PriorRunEvent>) -> Self {
        let mut kept: Vec<PriorRunEvent> = events.into_iter().filter(is_high_signal).collect();
        kept.sort_by(|left, right| {
            right
                .recorded_at
                .cmp(&left.recorded_at)
                .then(right.sequence.cmp(&left.sequence))
        });
        kept.truncate(MAX_EVENTS);
        kept.reverse();
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
        let seed = EntityId::new(task_fingerprint(task)).ok()?;
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
    let task_id = EntityId::new(task_fingerprint(task)).map_err(|error| error.to_string())?;
    let mut store = InMemoryStore::default();
    let bootstrap = pending_event(
        "bootstrap",
        "bootstrap",
        Timestamp::from_unix_micros(1),
        &agent,
        MemoryEvent::NodeUpserted {
            node: MemoryNode::new(task_id.clone(), "task", truncate(task, 160))
                .map_err(|error| error.to_string())?,
        },
        None,
        None,
    )?;
    let current_task_event = bootstrap.id.clone();
    append_batch(&mut store, "bootstrap", vec![bootstrap])?;
    for event in &memory.events {
        let at = Timestamp::from_unix_micros(event.recorded_at.saturating_mul(1_000_000).max(2));
        let attempt = EntityId::new(hashed_id(
            "attempt",
            &format!("{}:{}", event.run_id, event.sequence),
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
        let attempt_event = pending_event(
            &format!("{}:{}:attempt", event.run_id, event.sequence),
            &event.run_id,
            at,
            &agent,
            MemoryEvent::NodeUpserted {
                node: MemoryNode::new(attempt.clone(), event.kind.as_str(), truncate(&label, 160))
                    .map_err(|error| error.to_string())?,
            },
            Some(current_task_event.clone()),
            None,
        )?;
        let attempt_id = attempt_event.id.clone();
        let evidence = Evidence::new("run_event", format!("run:{}", event.run_id))
            .map_err(|error| error.to_string())?
            .with_locator(format!("seq:{}", event.sequence));
        let fact = MemoryFact::new(
            FactId::new(hashed_id(
                "fact",
                &format!("follows:{}:{}", event.run_id, event.sequence),
            ))
            .map_err(|error| error.to_string())?,
            task_id.clone(),
            "follows_attempt",
            attempt,
            at,
            at,
            agent.clone(),
            SessionId::new(hashed_id("session", &event.run_id))
                .map_err(|error| error.to_string())?,
            evidence,
        )
        .map_err(|error| error.to_string())?;
        let fact_event = pending_event(
            &format!("{}:{}:fact", event.run_id, event.sequence),
            &event.run_id,
            at,
            &agent,
            MemoryEvent::FactRecorded { fact },
            Some(current_task_event.clone()),
            Some(attempt_id),
        )?;
        append_batch(&mut store, &event.run_id, vec![attempt_event, fact_event])?;
    }
    Ok(store.load_all(None, usize::MAX))
}

fn append_batch(
    store: &mut InMemoryStore<MemoryEvent>,
    run_id: &str,
    events: Vec<NewEvent<MemoryEvent>>,
) -> Result<(), String> {
    let stream = StreamId::new(hashed_id("stream", run_id)).map_err(|error| error.to_string())?;
    let expected = store
        .stream_version(&stream)
        .map_or(ExpectedVersion::NoStream, |_| ExpectedVersion::Any);
    store
        .append_owned(&stream, expected, events)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn pending_event(
    id_key: &str,
    stream_key: &str,
    at: Timestamp,
    agent: &AgentId,
    payload: MemoryEvent,
    correlation_id: Option<EventId>,
    causation_id: Option<EventId>,
) -> Result<NewEvent<MemoryEvent>, String> {
    let event_type = payload.event_type().to_owned();
    let mut event = NewEvent::new(
        EventId::new(hashed_id("event", id_key)).map_err(|error| error.to_string())?,
        event_type,
        at,
        at,
        agent.clone(),
        SessionId::new(hashed_id("session", stream_key)).map_err(|error| error.to_string())?,
        payload,
    )
    .map_err(|error| error.to_string())?;
    if let Some(id) = correlation_id {
        event = event.correlated_with(id);
    }
    if let Some(id) = causation_id {
        event = event.caused_by(id);
    }
    Ok(event)
}

fn task_fingerprint(task: &str) -> String {
    hashed_id("task", &normalize_text(task))
}

fn hashed_id(prefix: &str, raw: &str) -> String {
    let digest = Sha256::digest(normalize_text(raw).as_bytes());
    format!("{prefix}:{}", hex::encode(digest))
        .chars()
        .take(prefix.len() + 1 + 16)
        .collect()
}

fn normalize_text(value: &str) -> String {
    value
        .nfkc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
    fn memory_arguments_link_the_current_task_without_rewriting_the_failure() {
        let memory = PriorRunMemory::from_parts(vec![failed()]);
        let arguments = memory
            .memory_arguments("still failing compile_context", 600)
            .expect("arguments");
        let events = arguments["events"].as_array().expect("events");
        assert!(events.len() >= 3);
        let blob = arguments.to_string();
        assert!(blob.contains("follows_attempt"));
        assert!(blob.contains("evidence gate rejected"));
        assert!(!blob.contains("\"relation\":\"node_failed\""));
        assert!(
            blob.contains("causation_id"),
            "facts must keep a causal link to the prior attempt event"
        );
        assert_eq!(arguments["request"]["token_budget"], 600);
    }

    #[test]
    fn russian_wordings_do_not_collapse_to_the_same_task_id() {
        assert_ne!(
            task_fingerprint("предыдущая попытка молча пропустила архив"),
            task_fingerprint("ещё раз проверь этот ран")
        );
        assert_eq!(
            task_fingerprint("  compile_context  "),
            task_fingerprint("compile_context")
        );
    }

    fn replay_arguments(arguments: &Value) -> weavatrix_rust::memory::MemoryProjection {
        let events =
            serde_json::from_value::<Vec<StoredEvent<MemoryEvent>>>(arguments["events"].clone())
                .expect("stored events");
        assert_eq!(events[0].metadata.global_position, 0);
        assert_eq!(events[0].metadata.stream_version, 0);
        weavatrix_rust::memory::replay_owned(events).expect("replay")
    }

    #[test]
    fn memory_arguments_replay_and_keep_run_evidence() {
        let memory = PriorRunMemory::from_parts(vec![failed()]);
        let arguments = memory
            .memory_arguments("still failing compile_context", 600)
            .expect("arguments");
        let projection = replay_arguments(&arguments);
        let request = serde_json::from_value::<ContextRequest>(arguments["request"].clone())
            .expect("request");
        let bundle = weavatrix_rust::memory::ContextCompiler::new(
            weavatrix_rust::memory::BytesTokenEstimator::default(),
        )
        .compile(&projection, &request)
        .expect("compile");
        assert_eq!(bundle.view.facts.len(), 1);
        assert_eq!(bundle.view.facts[0].relation, "follows_attempt");
        assert_eq!(bundle.view.facts[0].evidence[0].source, "run:run-1");
        assert_eq!(
            bundle.view.facts[0].evidence[0].locator.as_deref(),
            Some("seq:4")
        );
        assert!(
            bundle
                .view
                .nodes
                .iter()
                .any(|node| node.label.contains("evidence gate"))
        );
    }

    #[test]
    fn two_runs_start_stream_versions_at_zero() {
        let second = PriorRunEvent {
            run_id: "run-2".to_owned(),
            sequence: 1,
            detail: Some("ещё одна попытка".to_owned()),
            recorded_at: 1_700_000_100,
            ..failed()
        };
        let memory = PriorRunMemory::from_parts(vec![failed(), second]);
        let arguments = memory
            .memory_arguments("still failing compile_context", 800)
            .expect("arguments");
        let events =
            serde_json::from_value::<Vec<StoredEvent<MemoryEvent>>>(arguments["events"].clone())
                .expect("stored events");
        assert_eq!(events.len(), 5);
        assert_eq!(events[1].metadata.global_position, 1);
        assert_eq!(events[1].metadata.stream_version, 0);
        assert_eq!(events[3].metadata.stream_version, 0);
        let projection = replay_arguments(&arguments);
        let request = serde_json::from_value::<ContextRequest>(arguments["request"].clone())
            .expect("request");
        let bundle = weavatrix_rust::memory::ContextCompiler::new(
            weavatrix_rust::memory::BytesTokenEstimator::default(),
        )
        .compile(&projection, &request)
        .expect("compile");
        assert_eq!(bundle.view.facts.len(), 2);
        assert!(
            bundle
                .view
                .nodes
                .iter()
                .any(|node| node.label.contains("ещё"))
        );
    }

    #[test]
    fn several_events_on_one_run_keep_monotonic_stream_versions() {
        let later = PriorRunEvent {
            sequence: 8,
            recorded_at: 1_700_000_050,
            ..failed()
        };
        let memory = PriorRunMemory::from_parts(vec![failed(), later]);
        let arguments = memory
            .memory_arguments("still failing compile_context", 800)
            .expect("arguments");
        let events =
            serde_json::from_value::<Vec<StoredEvent<MemoryEvent>>>(arguments["events"].clone())
                .expect("stored events");
        assert_eq!(events[1].metadata.stream_version, 0);
        assert_eq!(events[4].metadata.stream_version, 3);
        replay_arguments(&arguments);
    }

    #[test]
    fn tiny_budget_fails_after_successful_replay() {
        let memory = PriorRunMemory::from_parts(vec![failed()]);
        let arguments = memory
            .memory_arguments("still failing compile_context", 1)
            .expect("arguments");
        let projection = replay_arguments(&arguments);
        let request = serde_json::from_value::<ContextRequest>(arguments["request"].clone())
            .expect("request");
        assert!(
            weavatrix_rust::memory::ContextCompiler::new(
                weavatrix_rust::memory::BytesTokenEstimator::default(),
            )
            .compile(&projection, &request)
            .is_err()
        );
    }

    #[test]
    fn from_parts_keeps_the_newest_high_signal_events() {
        let mut events = Vec::new();
        for sequence in 1..=40 {
            events.push(PriorRunEvent {
                sequence,
                recorded_at: 1_700_000_000 + i64::try_from(sequence).unwrap_or(i64::MAX),
                ..failed()
            });
        }
        let memory = PriorRunMemory::from_parts(events);
        assert_eq!(memory.events.len(), 32);
        assert_eq!(memory.events.first().map(|event| event.sequence), Some(9));
        assert_eq!(memory.events.last().map(|event| event.sequence), Some(40));
    }
}
