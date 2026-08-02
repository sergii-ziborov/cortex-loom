use std::collections::BTreeMap;
use std::time::Duration;

use cortex_router::ExecutionTarget;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelProfile {
    /// Exact Ollama model tag. It is never replaced with a smaller model.
    pub model: String,
    pub max_input_tokens: u32,
    pub max_output_tokens: u32,
    pub context_tokens: u32,
}

impl ModelProfile {
    #[must_use]
    pub fn new(
        model: impl Into<String>,
        max_input_tokens: u32,
        max_output_tokens: u32,
        context_tokens: u32,
    ) -> Self {
        Self {
            model: model.into(),
            max_input_tokens,
            max_output_tokens,
            context_tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OllamaConfig {
    pub base_url: String,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub max_request_bytes: usize,
    pub max_response_bytes: u64,
    pub profiles: BTreeMap<String, ModelProfile>,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".to_owned(),
            connect_timeout: Duration::from_secs(2),
            request_timeout: Duration::from_secs(120),
            read_timeout: Duration::from_secs(120),
            write_timeout: Duration::from_secs(10),
            max_request_bytes: 1_048_576,
            max_response_bytes: 1_048_576,
            // Selecting a model is an explicit deployment decision.
            profiles: BTreeMap::new(),
        }
    }
}

impl OllamaConfig {
    #[must_use]
    pub fn with_profile(mut self, name: impl Into<String>, profile: ModelProfile) -> Self {
        self.profiles.insert(name.into(), profile);
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }

    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftRequest {
    pub profile: String,
    pub messages: Vec<ChatMessage>,
    /// Stable IDs supplied alongside the prompt; draft citations must be a subset.
    pub evidence_ids: Vec<String>,
    pub estimated_input_tokens: u32,
    pub requested_output_tokens: u32,
}

impl DraftRequest {
    #[must_use]
    pub fn new(
        profile: impl Into<String>,
        messages: Vec<ChatMessage>,
        evidence_ids: Vec<String>,
        estimated_input_tokens: u32,
        requested_output_tokens: u32,
    ) -> Self {
        Self {
            profile: profile.into(),
            messages,
            evidence_ids,
            estimated_input_tokens,
            requested_output_tokens,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalDraft {
    pub summary: String,
    pub evidence_ids: Vec<String>,
    pub unresolved: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "code", content = "detail")]
pub enum QualityFailure {
    RouterRejected,
    SchemaInvalid(String),
    EmptySummary,
    UnknownEvidenceIds(Vec<String>),
    UnresolvedIssues(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DraftAssessment {
    pub target: ExecutionTarget,
    pub draft: Option<LocalDraft>,
    pub failures: Vec<QualityFailure>,
}

impl DraftAssessment {
    #[must_use]
    pub const fn is_accepted(&self) -> bool {
        matches!(self.target, ExecutionTarget::Ollama) && self.failures.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionInfo {
    pub version: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevicePlacement {
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningModel {
    pub name: String,
    pub model: String,
    pub size: u64,
    pub size_vram: u64,
    pub digest: String,
    pub placement: DevicePlacement,
}
