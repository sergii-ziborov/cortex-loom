//! Provider-independent local inference: which model, which device, which
//! job — and what was actually confirmed.
//!
//! Cortex Loom needs local models for three narrow jobs whose output can be
//! checked mechanically: embedding vectors for retrieval ordering, labels
//! from a closed set for routing, and per-revision digests computed off the
//! hot path. It does not need a local assistant, and this crate deliberately
//! offers no way to ask for one.
//!
//! ## Why a provider abstraction
//!
//! `cortex-ollama` speaks one runtime's API and is pinned to loopback. That
//! was right until the target became an accelerator: `OpenVINO` Model Server
//! serves the NPU and the GPU behind an OpenAI-compatible endpoint, and
//! llama.cpp, LM Studio and vLLM speak the same shape. One trait covers all
//! of them, and — the part that matters — the **loopback check lives in one
//! place**, so a new backend cannot forget it.
//!
//! ## What this crate refuses to do
//!
//! * It will not report a device a runtime did not confirm. See
//!   [`device::Placement`].
//! * It will not select a profile that has not passed its calibration gate.
//! * It will not reach a non-loopback host, and there is no flag to make it.

pub mod device;
pub mod endpoint;
mod micro_extract;
pub mod openai;
pub mod profile;

pub use device::{Device, DevicePolicy, Placement};
pub use endpoint::{EndpointError, LoopbackUrl};
pub use micro_extract::{MicroExtractError, MicroExtractOutput, MicroExtractRequest};
pub use openai::OpenAiProvider;
pub use profile::{LlmProfile, ProfileRegistry, Role, Runtime, SelectionError};

use std::fmt::{Display, Formatter};

/// A bounded embedding request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedRequest {
    pub inputs: Vec<String>,
}

/// A bounded classification request over a closed label set.
///
/// The labels travel with the request so the caller — not the model — decides
/// what answers exist. A reply outside the set is a failure, not a new label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyRequest {
    pub instruction: String,
    pub input: String,
    pub labels: Vec<String>,
}

/// What a provider returned, with the placement it could confirm.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderResponse<T> {
    pub value: T,
    pub placement: Placement,
    /// Wall-clock milliseconds, so a latency-tolerant role can be shown to be
    /// costing what it was budgeted.
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    Endpoint(EndpointError),
    /// The runtime answered, but not in the shape the role requires.
    Schema(String),
    /// The model returned a label outside the closed set it was given.
    UnknownLabel {
        got: String,
        expected: Vec<String>,
    },
    Transport(String),
    Timeout {
        after_ms: u64,
    },
}

impl Display for ProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Endpoint(error) => Display::fmt(error, formatter),
            Self::Schema(message) => write!(formatter, "invalid response shape: {message}"),
            Self::UnknownLabel { got, expected } => write!(
                formatter,
                "model returned {got:?}, which is not one of [{}]",
                expected.join(", ")
            ),
            Self::Transport(message) => write!(formatter, "transport failure: {message}"),
            Self::Timeout { after_ms } => write!(formatter, "no answer after {after_ms} ms"),
        }
    }
}

impl std::error::Error for ProviderError {}

impl From<EndpointError> for ProviderError {
    fn from(error: EndpointError) -> Self {
        Self::Endpoint(error)
    }
}

/// One local inference runtime.
///
/// Implementations are synchronous to match the rest of the workspace and
/// because every call is bounded by its profile's timeout anyway.
pub trait LlmProvider {
    /// The profile this provider was built for.
    fn profile(&self) -> &LlmProfile;

    /// # Errors
    ///
    /// Returns the reason the runtime could not produce vectors.
    fn embed(
        &self,
        request: &EmbedRequest,
    ) -> Result<ProviderResponse<Vec<Vec<f32>>>, ProviderError>;

    /// # Errors
    ///
    /// Returns [`ProviderError::UnknownLabel`] when the answer is outside the
    /// closed set, rather than passing an invented label upward.
    fn classify(
        &self,
        request: &ClassifyRequest,
    ) -> Result<ProviderResponse<String>, ProviderError>;

    /// Extract literal fields from verified input under a closed schema.
    ///
    /// # Errors
    ///
    /// Returns a schema error for unknown fields or any value absent from the
    /// verified input.
    fn micro_extract(
        &self,
        request: &MicroExtractRequest,
    ) -> Result<ProviderResponse<MicroExtractOutput>, ProviderError>;
}

/// Validate a classification answer against the closed set.
///
/// Shared so every backend fails the same way: matching is case-insensitive
/// and ignores surrounding punctuation, because small models pad answers, but
/// it never accepts a label that was not offered.
///
/// # Errors
///
/// Returns [`ProviderError::UnknownLabel`] when nothing matches.
pub fn resolve_label(answer: &str, labels: &[String]) -> Result<String, ProviderError> {
    let cleaned = answer
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .to_ascii_lowercase();
    labels
        .iter()
        .find(|label| label.to_ascii_lowercase() == cleaned)
        .cloned()
        .ok_or_else(|| ProviderError::UnknownLabel {
            got: answer.trim().to_owned(),
            expected: labels.to_vec(),
        })
}

#[cfg(test)]
mod tests {
    use super::{MicroExtractRequest, ProviderError, resolve_label};

    fn labels() -> Vec<String> {
        vec![
            "deterministic".to_owned(),
            "local_small".to_owned(),
            "upstream".to_owned(),
        ]
    }

    #[test]
    fn padding_is_forgiven_but_invention_is_not() {
        assert_eq!(
            resolve_label("  upstream. ", &labels()).unwrap(),
            "upstream"
        );
        assert_eq!(
            resolve_label("LOCAL_SMALL", &labels()).unwrap(),
            "local_small"
        );
        assert_eq!(
            resolve_label("probably upstream", &labels()),
            Err(ProviderError::UnknownLabel {
                got: "probably upstream".to_owned(),
                expected: labels(),
            }),
            "a hedge is not a label"
        );
        assert!(matches!(
            resolve_label("medium", &labels()),
            Err(ProviderError::UnknownLabel { .. })
        ));
    }

    #[test]
    fn micro_extraction_accepts_only_verified_bounded_literal_fields() {
        assert!(MicroExtractRequest::new("", &["identifier"]).is_err());
        assert!(MicroExtractRequest::new("const PORT = 43817", &[]).is_err());
        let request = MicroExtractRequest::new(
            "const PORT = 43817; CORTEX_SEMANTIC=1",
            &["identifier", "env"],
        )
        .unwrap();
        assert!(
            request
                .validate_output(&serde_json::json!({
                    "identifier": ["PORT"],
                    "env": ["CORTEX_SEMANTIC"]
                }))
                .is_ok()
        );
        assert!(
            request
                .validate_output(&serde_json::json!({"identifier": ["invented"]}))
                .is_err()
        );
        assert!(
            request
                .validate_output(&serde_json::json!({"route": ["upstream"]}))
                .is_err()
        );
    }
}
