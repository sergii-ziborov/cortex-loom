//! Suite execution against one candidate profile.

use std::fmt::Write as _;

use cortex_context::estimate_tokens;
use cortex_ollama::{ChatMessage, DevicePlacement, StructuredChatRequest};
use cortex_router::ModelTier;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backend::EvalBackend;
use crate::comparators::{citation_metrics, classification_outcome, token_delta};
use crate::fixtures::{
    ALLOWED_ACTIONS, ClassificationFixture, CompressionFixture, ExtractionFixture, FixtureSet,
};
use crate::metrics::{
    ClassificationAggregate, ClassificationSample, CompressionAggregate, CompressionSample,
    ExtractionAggregate, ExtractionMatches, ExtractionSample, LatencyStats,
    aggregate_classification, aggregate_compression, aggregate_extraction, latency_stats,
};
use crate::verdict::{CalibrationVerdict, judge};

const PROMPT_OVERHEAD_TOKENS: u32 = 32;
const CLASSIFICATION_OUTPUT_TOKENS: u32 = 128;
const EXTRACTION_OUTPUT_TOKENS: u32 = 256;
const COMPRESSION_OUTPUT_TOKENS: u32 = 768;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuiteSelection {
    pub classification: bool,
    pub extraction: bool,
    pub compression: bool,
}

impl SuiteSelection {
    #[must_use]
    pub const fn all() -> Self {
        Self {
            classification: true,
            extraction: true,
            compression: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvalProfile {
    /// Profile name registered in the Ollama client configuration.
    pub id: String,
    pub tier: ModelTier,
    /// Exact model tag; never substituted.
    pub model: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStatus {
    Evaluated,
    ModelAbsent,
    DiscoveryFailed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReport {
    pub profile_id: String,
    pub tier: ModelTier,
    pub model: String,
    pub status: ProfileStatus,
    pub digest: Option<String>,
    pub device: Option<DevicePlacement>,
    pub classification: Option<ClassificationAggregate>,
    pub extraction: Option<ExtractionAggregate>,
    pub compression: Option<CompressionAggregate>,
    pub latency: LatencyStats,
    pub verdict: CalibrationVerdict,
    pub classification_samples: Vec<ClassificationSample>,
    pub extraction_samples: Vec<ExtractionSample>,
    pub compression_samples: Vec<CompressionSample>,
}

/// Run the selected suites for one profile. Absent models are skipped
/// fail-closed: no pull, no substitute, an explicit status instead.
pub fn run_profile(
    backend: &dyn EvalBackend,
    profile: &EvalProfile,
    fixtures: &FixtureSet,
    selection: SuiteSelection,
    limit: Option<usize>,
) -> ProfileReport {
    let mut report = empty_report(profile);
    let installed = match backend.installed_models() {
        Ok(models) => models,
        Err(error) => {
            eprintln!("[cortex-eval] {}: discovery failed: {error}", profile.id);
            report.status = ProfileStatus::DiscoveryFailed;
            return report;
        }
    };
    let Some(model) = installed
        .iter()
        .find(|model| model.model == profile.model || model.name == profile.model)
    else {
        eprintln!(
            "[cortex-eval] {}: model {} is not installed; skipping (no hidden pull)",
            profile.id, profile.model
        );
        report.status = ProfileStatus::ModelAbsent;
        return report;
    };
    report.digest = Some(model.digest.clone());

    let mut latencies = Vec::new();
    if selection.classification {
        let taken = bounded(&fixtures.classification, limit);
        for (index, fixture) in taken.iter().enumerate() {
            let sample = run_classification(backend, &profile.id, fixture);
            progress(
                &profile.id,
                "classification",
                index,
                taken.len(),
                sample.error.as_deref(),
            );
            if sample.error.is_none() {
                latencies.push(sample.latency_ms);
            }
            report.classification_samples.push(sample);
        }
        report.classification = Some(aggregate_classification(&report.classification_samples));
    }
    if selection.extraction {
        let taken = bounded(&fixtures.extraction, limit);
        for (index, fixture) in taken.iter().enumerate() {
            let sample = run_extraction(backend, &profile.id, fixture);
            progress(
                &profile.id,
                "extraction",
                index,
                taken.len(),
                sample.error.as_deref(),
            );
            if sample.error.is_none() {
                latencies.push(sample.latency_ms);
            }
            report.extraction_samples.push(sample);
        }
        report.extraction = Some(aggregate_extraction(&report.extraction_samples));
    }
    if selection.compression {
        let taken = bounded(&fixtures.compression, limit);
        for (index, fixture) in taken.iter().enumerate() {
            let sample = run_compression(backend, &profile.id, fixture);
            progress(
                &profile.id,
                "compression",
                index,
                taken.len(),
                sample.error.as_deref(),
            );
            if sample.error.is_none() {
                latencies.push(sample.latency_ms);
            }
            report.compression_samples.push(sample);
        }
        report.compression = Some(aggregate_compression(&report.compression_samples));
    }

    report.device = backend.running_models().ok().and_then(|running| {
        running
            .iter()
            .find(|entry| entry.model == profile.model || entry.name == profile.model)
            .map(|entry| entry.placement)
    });
    report.latency = latency_stats(&latencies);
    report.verdict = judge(
        report.classification.as_ref(),
        report.extraction.as_ref(),
        report.compression.as_ref(),
    );
    report
}

fn empty_report(profile: &EvalProfile) -> ProfileReport {
    ProfileReport {
        profile_id: profile.id.clone(),
        tier: profile.tier,
        model: profile.model.clone(),
        status: ProfileStatus::Evaluated,
        digest: None,
        device: None,
        classification: None,
        extraction: None,
        compression: None,
        latency: latency_stats(&[]),
        verdict: judge(None, None, None),
        classification_samples: Vec::new(),
        extraction_samples: Vec::new(),
        compression_samples: Vec::new(),
    }
}

fn bounded<T>(fixtures: &[T], limit: Option<usize>) -> &[T] {
    let count = limit.unwrap_or(fixtures.len()).min(fixtures.len());
    &fixtures[..count]
}

fn progress(profile: &str, suite: &str, index: usize, total: usize, error: Option<&str>) {
    match error {
        None => eprintln!("[cortex-eval] {profile} {suite} {}/{total}", index + 1),
        Some(error) => eprintln!(
            "[cortex-eval] {profile} {suite} {}/{total}: {error}",
            index + 1
        ),
    }
}

fn request(
    profile: &str,
    system: &str,
    user: String,
    schema: Value,
    output_tokens: u32,
) -> StructuredChatRequest {
    let estimated_input_tokens = estimate_tokens(system)
        .saturating_add(estimate_tokens(&user))
        .saturating_add(PROMPT_OVERHEAD_TOKENS);
    StructuredChatRequest {
        profile: profile.to_owned(),
        messages: vec![ChatMessage::system(system), ChatMessage::user(user)],
        schema,
        estimated_input_tokens,
        requested_output_tokens: output_tokens,
    }
}

const CLASSIFICATION_SYSTEM: &str = "You classify one engineering task for a routing policy. Reply with JSON only. Tiers: none = deterministic tooling or repository graph analysis without any model; local_small = bounded structured extraction over supplied text; local_medium = citation-preserving summarization or advisory drafting over supplied evidence; upstream_strong = anything that mutates code or state, security, authentication, concurrency, migrations, releases, deployment, publication, or ambiguous work. When uncertain choose upstream_strong.";

fn tier_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tier": {"type": "string", "enum": ["none", "local_small", "local_medium", "upstream_strong"]}
        },
        "required": ["tier"],
        "additionalProperties": false
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TierResponse {
    tier: ModelTier,
}

fn run_classification(
    backend: &dyn EvalBackend,
    profile: &str,
    fixture: &ClassificationFixture,
) -> ClassificationSample {
    let call = backend.structured(&request(
        profile,
        CLASSIFICATION_SYSTEM,
        format!("Task: {}", fixture.task),
        tier_schema(),
        CLASSIFICATION_OUTPUT_TOKENS,
    ));
    let (observed, latency_ms, error) = match call {
        Ok(timed) => match serde_json::from_str::<TierResponse>(&timed.content) {
            Ok(parsed) => (Some(parsed.tier), timed.latency_ms, None),
            Err(parse_error) => (None, timed.latency_ms, Some(parse_error.to_string())),
        },
        Err(error) => (None, 0, Some(error)),
    };
    ClassificationSample {
        fixture_id: fixture.id.clone(),
        gold_tier: fixture.gold_tier,
        observed_tier: observed,
        schema_valid: observed.is_some(),
        outcome: classification_outcome(fixture.gold_tier, observed),
        latency_ms,
        error,
    }
}

const EXTRACTION_SYSTEM: &str = "You extract fields literally present in one task description. Reply with JSON only. action is one of: add, fix, remove, rename, move, refactor, document, test, update, other. files lists file paths exactly as written. symbols lists function, constant, or type names exactly as written. Never invent entries; use empty arrays when nothing is present.";

fn extraction_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {"type": "string", "enum": ALLOWED_ACTIONS},
            "files": {"type": "array", "items": {"type": "string"}},
            "symbols": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["action", "files", "symbols"],
        "additionalProperties": false
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractionResponse {
    action: String,
    files: Vec<String>,
    symbols: Vec<String>,
}

fn normalized(values: &[String]) -> Vec<String> {
    let mut sorted: Vec<String> = values.iter().map(|value| value.trim().to_owned()).collect();
    sorted.sort();
    sorted.dedup();
    sorted
}

fn run_extraction(
    backend: &dyn EvalBackend,
    profile: &str,
    fixture: &ExtractionFixture,
) -> ExtractionSample {
    let call = backend.structured(&request(
        profile,
        EXTRACTION_SYSTEM,
        format!("Task: {}", fixture.text),
        extraction_schema(),
        EXTRACTION_OUTPUT_TOKENS,
    ));
    let no_match = ExtractionMatches {
        action: false,
        files: false,
        symbols: false,
    };
    let (schema_valid, matches, latency_ms, error) = match call {
        Ok(timed) => match serde_json::from_str::<ExtractionResponse>(&timed.content) {
            Ok(parsed) => {
                let matches = ExtractionMatches {
                    action: parsed.action.eq_ignore_ascii_case(&fixture.gold.action),
                    files: normalized(&parsed.files) == normalized(&fixture.gold.files),
                    symbols: normalized(&parsed.symbols) == normalized(&fixture.gold.symbols),
                };
                (true, matches, timed.latency_ms, None)
            }
            Err(parse_error) => (
                false,
                no_match,
                timed.latency_ms,
                Some(parse_error.to_string()),
            ),
        },
        Err(error) => (false, no_match, 0, Some(error)),
    };
    ExtractionSample {
        fixture_id: fixture.id.clone(),
        schema_valid,
        matches,
        latency_ms,
        error,
    }
}

const COMPRESSION_SYSTEM: &str = "You compress evidence into one short grounded briefing for a coding agent. Keep the summary under 120 words. Cite evidence inline with bracketed IDs such as [WX-GRAPH], and list every cited ID in evidenceIds. Use only supplied IDs and never invent one. Reply with JSON only.";

fn compression_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "summary": {"type": "string"},
            "evidenceIds": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["summary", "evidenceIds"],
        "additionalProperties": false
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompressionResponse {
    summary: String,
    evidence_ids: Vec<String>,
}

fn run_compression(
    backend: &dyn EvalBackend,
    profile: &str,
    fixture: &CompressionFixture,
) -> CompressionSample {
    let mut blocks = String::new();
    for evidence in &fixture.evidence {
        let _ = write!(
            blocks,
            "## [{}] {}\n{}\n\n",
            evidence.id, evidence.source, evidence.content
        );
    }
    let call = backend.structured(&request(
        profile,
        COMPRESSION_SYSTEM,
        format!("Task: {}\n\nEvidence:\n\n{blocks}", fixture.task),
        compression_schema(),
        COMPRESSION_OUTPUT_TOKENS,
    ));
    let supplied: Vec<String> = fixture
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect();
    let contents: Vec<&str> = fixture
        .evidence
        .iter()
        .map(|evidence| evidence.content.as_str())
        .collect();
    let (schema_valid, citations, delta, latency_ms, error) = match call {
        Ok(timed) => match serde_json::from_str::<CompressionResponse>(&timed.content) {
            Ok(parsed) => {
                let citations = citation_metrics(
                    &supplied,
                    &fixture.must_cite,
                    &parsed.summary,
                    &parsed.evidence_ids,
                );
                let delta = token_delta(&contents, &parsed.summary);
                (true, Some(citations), Some(delta), timed.latency_ms, None)
            }
            Err(parse_error) => (
                false,
                None,
                None,
                timed.latency_ms,
                Some(parse_error.to_string()),
            ),
        },
        Err(error) => (false, None, None, 0, Some(error)),
    };
    CompressionSample {
        fixture_id: fixture.id.clone(),
        schema_valid,
        citations,
        token_delta: delta,
        latency_ms,
        error,
    }
}
