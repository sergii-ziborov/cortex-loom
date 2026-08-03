//! Background observation worker: one dedicated thread, blocking client,
//! append-only samples. Nothing here can reach back into the hot path.

use std::fmt::Write as _;
use std::sync::mpsc::Receiver;

use cortex_context::estimate_tokens;
use cortex_eval::backend::EvalBackend;
use cortex_eval::comparators::{citation_metrics, classification_outcome};
use cortex_eval::prompts::{
    EvidenceBlock, classification_request, compression_request, parse_compression, parse_tier,
};
use cortex_ollama::{DevicePlacement, ModelProfile};
use cortex_store::{ShadowOperation, ShadowSample, ShadowStore};
use sha2::{Digest, Sha256};

use crate::{CompressionSnapshot, RoutingSnapshot, ShadowEvidence, ShadowTask};

pub const SHADOW_SMALL_PROFILE: &str = "shadow-small";
pub const SHADOW_MEDIUM_PROFILE: &str = "shadow-medium";

const MAX_INPUT_TOKENS: u32 = 6_144;
const MAX_OUTPUT_TOKENS: u32 = 1_024;
const CONTEXT_TOKENS: u32 = 8_192;

pub(crate) fn shadow_profile(model: &str) -> ModelProfile {
    ModelProfile::new(model, MAX_INPUT_TOKENS, MAX_OUTPUT_TOKENS, CONTEXT_TOKENS)
}

pub(crate) struct WorkerModels {
    pub small: Option<String>,
    pub medium: Option<String>,
}

pub(crate) fn run(
    receiver: &Receiver<ShadowTask>,
    backend: &dyn EvalBackend,
    store: &ShadowStore,
    models: &WorkerModels,
) {
    while let Ok(task) = receiver.recv() {
        if let Some(sample) = process(backend, models, &task)
            && let Err(error) = store.insert(&sample)
        {
            eprintln!("[cortex-shadow] sample insert failed: {error}");
        }
    }
}

/// Turn one observation task into one immutable sample. Returns `None` when
/// the operation has no configured model.
pub fn process(
    backend: &dyn EvalBackend,
    models: &WorkerModels,
    task: &ShadowTask,
) -> Option<ShadowSample> {
    match task {
        ShadowTask::RouteClassification {
            task,
            deterministic,
        } => models
            .small
            .as_deref()
            .map(|model| classify(backend, model, task, *deterministic)),
        ShadowTask::ContextCompression {
            task,
            evidence,
            deterministic,
        } => models
            .medium
            .as_deref()
            .map(|model| compress(backend, model, task, evidence, deterministic)),
    }
}

fn classify(
    backend: &dyn EvalBackend,
    model: &str,
    task: &str,
    deterministic: RoutingSnapshot,
) -> ShadowSample {
    let mut sample = base_sample(
        ShadowOperation::RouteClassification,
        model,
        digest(&["route_classification", task]),
        to_compact_json(&deterministic),
    );
    match backend.structured(&classification_request(SHADOW_SMALL_PROFILE, task)) {
        Ok(timed) => {
            sample.latency_ms = Some(timed.latency_ms);
            match parse_tier(&timed.content) {
                Ok(tier) => {
                    // Fail-closed comparison: a shadow tier below a
                    // deterministic upstream decision is a missed escalation
                    // and can never relax the escalation itself.
                    let outcome = classification_outcome(deterministic.tier, Some(tier));
                    sample.schema_valid = Some(true);
                    sample.agreement = Some(outcome.agreement);
                    sample.missed_escalation = outcome.missed_escalation;
                    sample.shadow_summary =
                        Some(format!("{{\"tier\":{}}}", to_compact_json(&tier)));
                }
                Err(parse_error) => {
                    sample.schema_valid = Some(false);
                    sample.agreement = Some(false);
                    sample.error = Some(parse_error);
                }
            }
            sample.device = device(backend, model);
        }
        Err(error) => sample.error = Some(error),
    }
    sample
}

fn compress(
    backend: &dyn EvalBackend,
    model: &str,
    task: &str,
    evidence: &[ShadowEvidence],
    deterministic: &CompressionSnapshot,
) -> ShadowSample {
    let mut digest_parts: Vec<&str> = vec!["context_compression", task];
    for item in evidence {
        digest_parts.push(&item.id);
        digest_parts.push(&item.content);
    }
    let mut sample = base_sample(
        ShadowOperation::ContextCompression,
        model,
        digest(&digest_parts),
        to_compact_json(deterministic),
    );

    let blocks: Vec<EvidenceBlock<'_>> = evidence
        .iter()
        .map(|item| EvidenceBlock {
            id: &item.id,
            source: &item.source,
            content: &item.content,
        })
        .collect();
    match backend.structured(&compression_request(SHADOW_MEDIUM_PROFILE, task, &blocks)) {
        Ok(timed) => {
            sample.latency_ms = Some(timed.latency_ms);
            match parse_compression(&timed.content) {
                Ok(draft) => {
                    let supplied: Vec<String> =
                        evidence.iter().map(|item| item.id.clone()).collect();
                    // The draft must preserve the deterministic citations that
                    // are actual evidence fragments; the synthetic TASK id is
                    // excluded by intersecting with the supplied evidence.
                    let must_cite: Vec<String> = deterministic
                        .included_ids
                        .iter()
                        .filter(|id| supplied.contains(id))
                        .cloned()
                        .collect();
                    let citations = citation_metrics(
                        &supplied,
                        &must_cite,
                        &draft.summary,
                        &draft.evidence_ids,
                    );
                    sample.schema_valid = Some(true);
                    sample.citation_preserved_ratio = Some(citations.preserved_ratio);
                    sample.hallucinated_citations =
                        Some(u32::try_from(citations.hallucinated.len()).unwrap_or(u32::MAX));
                    sample.token_estimate_delta = Some(
                        i64::from(estimate_tokens(&draft.summary))
                            - i64::from(deterministic.selected_estimated_tokens),
                    );
                    sample.shadow_summary = Some(to_compact_json(&serde_json::json!({
                        "summary": draft.summary,
                        "evidenceIds": draft.evidence_ids,
                    })));
                }
                Err(parse_error) => {
                    sample.schema_valid = Some(false);
                    sample.error = Some(parse_error);
                }
            }
            sample.device = device(backend, model);
        }
        Err(error) => sample.error = Some(error),
    }
    sample
}

fn base_sample(
    operation: ShadowOperation,
    model: &str,
    input_digest: String,
    deterministic_summary: String,
) -> ShadowSample {
    ShadowSample {
        operation,
        model_tag: model.to_owned(),
        device: None,
        latency_ms: None,
        input_digest,
        deterministic_summary,
        shadow_summary: None,
        schema_valid: None,
        agreement: None,
        missed_escalation: false,
        citation_preserved_ratio: None,
        hallucinated_citations: None,
        token_estimate_delta: None,
        error: None,
    }
}

/// SHA-256 of the canonical input; raw payloads are never persisted.
fn digest(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    let digest = hasher.finalize();
    let mut rendered = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(rendered, "{byte:02x}");
    }
    rendered
}

fn to_compact_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned())
}

fn device(backend: &dyn EvalBackend, model: &str) -> Option<String> {
    backend.running_models().ok().and_then(|running| {
        running
            .iter()
            .find(|entry| entry.model == model || entry.name == model)
            .map(|entry| match entry.placement {
                DevicePlacement::Cpu => "cpu".to_owned(),
                DevicePlacement::Gpu => "gpu".to_owned(),
            })
    })
}
