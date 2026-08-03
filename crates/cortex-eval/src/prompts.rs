//! Pinned prompts, schemas, and parsers shared by the offline benchmark and
//! shadow mode.
//!
//! Keeping these single-sourced means shadow observations stay directly
//! comparable with offline calibration runs under the same
//! [`crate::PROMPT_VERSION`] and [`crate::SCHEMA_VERSION`].

use std::fmt::Write as _;

use cortex_context::estimate_tokens;
use cortex_ollama::{ChatMessage, StructuredChatRequest};
use cortex_router::ModelTier;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::fixtures::ALLOWED_ACTIONS;

pub const CLASSIFICATION_OUTPUT_TOKENS: u32 = 128;
pub const EXTRACTION_OUTPUT_TOKENS: u32 = 256;
pub const COMPRESSION_OUTPUT_TOKENS: u32 = 768;

const PROMPT_OVERHEAD_TOKENS: u32 = 32;

pub const CLASSIFICATION_SYSTEM: &str = "You classify one engineering task for a routing policy. Reply with JSON only. Tiers: none = deterministic tooling or repository graph analysis without any model; local_small = bounded structured extraction over supplied text; local_medium = citation-preserving summarization or advisory drafting over supplied evidence; upstream_strong = anything that mutates code or state, security, authentication, concurrency, migrations, releases, deployment, publication, or ambiguous work. When uncertain choose upstream_strong.";

pub const EXTRACTION_SYSTEM: &str = "You extract fields literally present in one task description. Reply with JSON only. action is one of: add, fix, remove, rename, move, refactor, document, test, update, other. files lists file paths exactly as written. symbols lists function, constant, or type names exactly as written. Never invent entries; use empty arrays when nothing is present.";

pub const COMPRESSION_SYSTEM: &str = "You compress evidence into one short grounded briefing for a coding agent. Keep the summary under 120 words. Cite evidence inline with bracketed IDs such as [WX-GRAPH], and list every cited ID in evidenceIds. Use only supplied IDs and never invent one. Reply with JSON only.";

/// One evidence block rendered into a compression prompt.
#[derive(Debug, Clone, Copy)]
pub struct EvidenceBlock<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub content: &'a str,
}

#[must_use]
pub fn tier_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tier": {"type": "string", "enum": ["none", "local_small", "local_medium", "upstream_strong"]}
        },
        "required": ["tier"],
        "additionalProperties": false
    })
}

#[must_use]
pub fn extraction_schema() -> Value {
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

#[must_use]
pub fn compression_schema() -> Value {
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

#[must_use]
pub fn classification_request(profile: &str, task: &str) -> StructuredChatRequest {
    request(
        profile,
        CLASSIFICATION_SYSTEM,
        format!("Task: {task}"),
        tier_schema(),
        CLASSIFICATION_OUTPUT_TOKENS,
    )
}

#[must_use]
pub fn extraction_request(profile: &str, text: &str) -> StructuredChatRequest {
    request(
        profile,
        EXTRACTION_SYSTEM,
        format!("Task: {text}"),
        extraction_schema(),
        EXTRACTION_OUTPUT_TOKENS,
    )
}

#[must_use]
pub fn compression_request(
    profile: &str,
    task: &str,
    evidence: &[EvidenceBlock<'_>],
) -> StructuredChatRequest {
    let mut blocks = String::new();
    for block in evidence {
        let _ = write!(
            blocks,
            "## [{}] {}\n{}\n\n",
            block.id, block.source, block.content
        );
    }
    request(
        profile,
        COMPRESSION_SYSTEM,
        format!("Task: {task}\n\nEvidence:\n\n{blocks}"),
        compression_schema(),
        COMPRESSION_OUTPUT_TOKENS,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TierResponse {
    tier: ModelTier,
}

/// Parse a tier draft; the error is the schema-failure detail.
pub fn parse_tier(content: &str) -> Result<ModelTier, String> {
    serde_json::from_str::<TierResponse>(content)
        .map(|response| response.tier)
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExtractionDraft {
    pub action: String,
    pub files: Vec<String>,
    pub symbols: Vec<String>,
}

pub fn parse_extraction(content: &str) -> Result<ExtractionDraft, String> {
    serde_json::from_str(content).map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompressionDraft {
    pub summary: String,
    pub evidence_ids: Vec<String>,
}

pub fn parse_compression(content: &str) -> Result<CompressionDraft, String> {
    serde_json::from_str(content).map_err(|error| error.to_string())
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
