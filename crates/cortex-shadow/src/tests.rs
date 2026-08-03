use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};

use cortex_eval::backend::{EvalBackend, ScriptedBackend, TimedContent};
use cortex_ollama::{ModelInfo, RunningModel, StructuredChatRequest};
use cortex_router::{ModelTier, RiskLevel, TaskClass};
use cortex_store::{GraphStore, ShadowOperation};

use crate::worker::{WorkerModels, process};
use crate::{
    CompressionSnapshot, RoutingSnapshot, ShadowConfig, ShadowEvidence, ShadowTask, spawn,
    spawn_with_backend,
};

fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |key| {
        pairs
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| (*value).to_owned())
    }
}

fn both_models() -> WorkerModels {
    WorkerModels {
        small: Some("qwen-test:4b".to_owned()),
        medium: Some("qwen-test:9b".to_owned()),
    }
}

fn routing(tier: ModelTier) -> RoutingSnapshot {
    RoutingSnapshot {
        tier,
        class: TaskClass::Implementation,
        risk: RiskLevel::Medium,
    }
}

fn classification_task(tier: ModelTier) -> ShadowTask {
    ShadowTask::RouteClassification {
        task: "Fix the failing unit test".to_owned(),
        deterministic: routing(tier),
    }
}

#[test]
fn shadow_is_off_by_default_and_requires_explicit_configuration() {
    let config = ShadowConfig::from_lookup(lookup(&[]));
    assert!(!config.enabled);
    assert!(!config.is_active());

    // Enabled without any model is still inactive.
    let enabled_only = ShadowConfig::from_lookup(lookup(&[("CORTEX_SHADOW", "1")]));
    assert!(enabled_only.enabled && !enabled_only.is_active());

    // A model without the explicit switch is inactive too.
    let model_only = ShadowConfig::from_lookup(lookup(&[("CORTEX_SHADOW_SMALL", "qwen-test:4b")]));
    assert!(!model_only.is_active());

    let store = GraphStore::open_in_memory().unwrap().shadow();
    assert!(spawn(enabled_only, store).unwrap().is_none());
}

#[test]
fn active_configuration_parses_bounds() {
    let config = ShadowConfig::from_lookup(lookup(&[
        ("CORTEX_SHADOW", "1"),
        ("CORTEX_SHADOW_SMALL", "qwen-test:4b"),
        ("CORTEX_SHADOW_TIMEOUT_MS", "1500"),
        ("CORTEX_SHADOW_QUEUE", "8"),
    ]));
    assert!(config.is_active());
    assert_eq!(config.timeout_ms, 1_500);
    assert_eq!(config.queue_capacity, 8);
    assert_eq!(config.medium_model, None);
}

#[test]
fn classification_records_agreement_and_missed_escalation() {
    let agree = ScriptedBackend::new(
        Vec::new(),
        vec![Ok("{\"tier\":\"upstream_strong\"}".to_owned())],
    );
    let sample = process(
        &agree,
        &both_models(),
        &classification_task(ModelTier::UpstreamStrong),
    )
    .expect("small model is configured");
    assert_eq!(sample.operation, ShadowOperation::RouteClassification);
    assert_eq!(sample.schema_valid, Some(true));
    assert_eq!(sample.agreement, Some(true));
    assert!(!sample.missed_escalation);
    assert!(sample.shadow_summary.is_some());

    let missed = ScriptedBackend::new(
        Vec::new(),
        vec![Ok("{\"tier\":\"local_small\"}".to_owned())],
    );
    let sample = process(
        &missed,
        &both_models(),
        &classification_task(ModelTier::UpstreamStrong),
    )
    .unwrap();
    assert_eq!(sample.agreement, Some(false));
    assert!(sample.missed_escalation, "the key safety metric");

    let invalid = ScriptedBackend::new(Vec::new(), vec![Ok("not json".to_owned())]);
    let sample = process(
        &invalid,
        &both_models(),
        &classification_task(ModelTier::UpstreamStrong),
    )
    .unwrap();
    assert_eq!(sample.schema_valid, Some(false));
    assert!(!sample.missed_escalation);
    assert!(sample.error.is_some());

    let failing = ScriptedBackend::new(Vec::new(), vec![Err("timeout".to_owned())]);
    let sample = process(
        &failing,
        &both_models(),
        &classification_task(ModelTier::UpstreamStrong),
    )
    .unwrap();
    assert_eq!(sample.schema_valid, None);
    assert_eq!(sample.error.as_deref(), Some("timeout"));
    assert_eq!(sample.latency_ms, None);
}

#[test]
fn compression_checks_citations_against_the_deterministic_packet() {
    let task = ShadowTask::ContextCompression {
        task: "Summarize the evidence".to_owned(),
        evidence: vec![ShadowEvidence {
            id: "WX-A".to_owned(),
            source: "weavatrix:a".to_owned(),
            content: "alpha evidence ".repeat(30),
        }],
        deterministic: CompressionSnapshot {
            included_ids: vec!["TASK".to_owned(), "WX-A".to_owned()],
            omitted_ids: Vec::new(),
            selected_estimated_tokens: 400,
            requires_upstream: true,
        },
    };
    let backend = ScriptedBackend::new(
        Vec::new(),
        vec![Ok(
            "{\"summary\":\"See [WX-A] and [WX-FAKE].\",\"evidenceIds\":[]}".to_owned(),
        )],
    );
    let sample = process(&backend, &both_models(), &task).expect("medium model configured");
    assert_eq!(sample.operation, ShadowOperation::ContextCompression);
    assert_eq!(sample.schema_valid, Some(true));
    let preserved = sample.citation_preserved_ratio.unwrap();
    assert!((preserved - 1.0).abs() < 1e-9, "TASK is not required");
    assert_eq!(sample.hallucinated_citations, Some(1));
    let delta = sample.token_estimate_delta.unwrap();
    assert!(delta < 0, "draft is smaller than the deterministic packet");
    assert!(!sample.missed_escalation, "compression never relaxes gates");

    let unconfigured = WorkerModels {
        small: Some("qwen-test:4b".to_owned()),
        medium: None,
    };
    let skipped = ScriptedBackend::new(Vec::new(), vec![Ok("unused".to_owned())]);
    assert!(process(&skipped, &unconfigured, &task).is_none());
    assert_eq!(skipped.remaining(), 1, "no model call happened");
}

struct BlockingBackend {
    gate: Mutex<Receiver<()>>,
}

impl EvalBackend for BlockingBackend {
    fn version(&self) -> Result<String, String> {
        Ok("blocking".to_owned())
    }

    fn installed_models(&self) -> Result<Vec<ModelInfo>, String> {
        Ok(Vec::new())
    }

    fn running_models(&self) -> Result<Vec<RunningModel>, String> {
        Ok(Vec::new())
    }

    fn structured(&self, _request: &StructuredChatRequest) -> Result<TimedContent, String> {
        let gate = self
            .gate
            .lock()
            .map_err(|_| "gate lock poisoned".to_owned())?;
        let _ = gate.recv();
        Err("released".to_owned())
    }
}

#[test]
fn a_full_queue_drops_samples_without_blocking_the_caller() {
    let (release, gate) = channel();
    let backend = BlockingBackend {
        gate: Mutex::new(gate),
    };
    let config = ShadowConfig {
        enabled: true,
        small_model: Some("qwen-test:4b".to_owned()),
        medium_model: None,
        timeout_ms: 1_000,
        queue_capacity: 1,
    };
    let store = GraphStore::open_in_memory().unwrap().shadow();
    let handle = spawn_with_backend(config, store, backend)
        .unwrap()
        .expect("active configuration spawns a runner");

    for _ in 0..4 {
        handle.observe(classification_task(ModelTier::UpstreamStrong));
    }
    assert!(handle.dropped() >= 1, "overflow must drop, not block");

    // Operations without a configured model are ignored, not queued.
    handle.observe(ShadowTask::ContextCompression {
        task: "ignored".to_owned(),
        evidence: Vec::new(),
        deterministic: CompressionSnapshot {
            included_ids: Vec::new(),
            omitted_ids: Vec::new(),
            selected_estimated_tokens: 0,
            requires_upstream: true,
        },
    });

    drop(release);
}
