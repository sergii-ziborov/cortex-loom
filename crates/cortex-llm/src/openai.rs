//! The OpenAI-compatible backend.
//!
//! One HTTP shape covers every local runtime worth targeting: `OpenVINO` Model
//! Server (which is how the NPU and the GPU are reached), llama.cpp's server,
//! LM Studio, vLLM and `LocalAI`. The paths differ only in prefix, so the
//! prefix is configuration rather than a new backend per vendor.
//!
//! ## On not confirming the device
//!
//! Measured against OVMS 2026.3.0 on 2026-08-05: neither `/v1/config` nor
//! `/metrics` reports which device a servable was compiled for, and the
//! completion response carries no device field either. So this provider
//! **cannot** confirm placement, and it does not pretend to —
//! [`Placement::observed`] stays `None` and telemetry renders
//! `npu (unconfirmed)`.
//!
//! That is deliberately unsatisfying. The alternative — echoing back the
//! device we asked for — would turn a configuration value into a measurement
//! and let the project claim NPU execution on the strength of its own
//! command line. If a runtime ever does report placement, that is what
//! [`OpenAiProvider::with_observed_device`] is for, and nothing else may set
//! it.

use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::device::{Device, Placement};
use crate::endpoint::LoopbackUrl;
use crate::profile::LlmProfile;
use crate::{
    ClassifyRequest, EmbedRequest, LlmProvider, ProviderError, ProviderResponse, resolve_label,
};

/// Largest reply a classification is allowed to produce.
///
/// A label from a closed set is a handful of tokens. Anything longer is a
/// model explaining itself, which is not what was asked for and not what will
/// be accepted.
const CLASSIFY_MAX_TOKENS: u32 = 16;

/// The default OVMS prefix. llama.cpp and LM Studio use `/v1`.
pub const DEFAULT_PATH_PREFIX: &str = "/v3";

pub struct OpenAiProvider {
    profile: LlmProfile,
    base: LoopbackUrl,
    prefix: String,
    agent: ureq::Agent,
    observed: Option<Device>,
}

impl OpenAiProvider {
    /// # Errors
    ///
    /// Returns [`ProviderError::Endpoint`] when the profile's base URL is not
    /// a loopback address.
    pub fn new(profile: LlmProfile) -> Result<Self, ProviderError> {
        Self::with_prefix(profile, DEFAULT_PATH_PREFIX)
    }

    /// # Errors
    ///
    /// As [`OpenAiProvider::new`].
    pub fn with_prefix(profile: LlmProfile, prefix: &str) -> Result<Self, ProviderError> {
        let base = LoopbackUrl::parse(&profile.base_url)?;
        let timeout = Duration::from_secs(u64::from(profile.timeout_seconds.max(1)));
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .timeout_connect(Some(Duration::from_secs(3)))
            .build()
            .into();
        Ok(Self {
            profile,
            base,
            prefix: prefix.trim_end_matches('/').to_owned(),
            agent,
            observed: None,
        })
    }

    /// Record a device a runtime actually reported.
    ///
    /// The only sanctioned way to set [`Placement::observed`]. Call it with
    /// what the runtime said, never with what was configured.
    #[must_use]
    pub fn with_observed_device(mut self, observed: Device) -> Self {
        self.observed = Some(observed);
        self
    }

    fn placement(&self) -> Placement {
        self.observed.map_or_else(
            || Placement::declared(self.profile.device),
            |observed| Placement::observed(self.profile.device, observed),
        )
    }

    fn post<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<(T, u64), ProviderError> {
        let url = self.base.join(&format!("{}{path}", self.prefix));
        let started = Instant::now();
        let mut response = self
            .agent
            .post(&url)
            .header("content-type", "application/json")
            .send(body.to_string())
            .map_err(|error| classify_transport(&error, started.elapsed()))?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|error| ProviderError::Transport(error.to_string()))?;
        let parsed = serde_json::from_str::<T>(&text)
            .map_err(|error| ProviderError::Schema(format!("{error}: {}", preview(&text))))?;
        Ok((parsed, elapsed_ms(started)))
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// A timeout and a refused connection are different problems for an operator:
/// one means the model is too slow for its budget, the other means nothing is
/// listening. Collapsing them into "transport failure" hides which.
fn classify_transport(error: &ureq::Error, elapsed: Duration) -> ProviderError {
    if matches!(error, ureq::Error::Timeout(_)) {
        return ProviderError::Timeout {
            after_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        };
    }
    ProviderError::Transport(error.to_string())
}

/// Keep an unparseable body short enough to read in a log line.
fn preview(text: &str) -> String {
    let cleaned = text.trim().replace('\n', " ");
    if cleaned.chars().count() <= 200 {
        return cleaned;
    }
    cleaned.chars().take(200).collect::<String>() + "…"
}

#[derive(Debug, Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingRow>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingRow {
    index: usize,
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

impl LlmProvider for OpenAiProvider {
    fn profile(&self) -> &LlmProfile {
        &self.profile
    }

    fn embed(
        &self,
        request: &EmbedRequest,
    ) -> Result<ProviderResponse<Vec<Vec<f32>>>, ProviderError> {
        if request.inputs.is_empty() {
            return Err(ProviderError::Schema("no inputs to embed".to_owned()));
        }
        let body = serde_json::json!({
            "model": self.profile.model,
            "input": request.inputs,
        });
        let (response, latency_ms): (EmbeddingsResponse, u64) = self.post("/embeddings", &body)?;
        if response.data.len() != request.inputs.len() {
            return Err(ProviderError::Schema(format!(
                "asked for {} vectors, received {}",
                request.inputs.len(),
                response.data.len()
            )));
        }
        // The OpenAI shape does not promise ordered rows, and a silently
        // permuted batch would corrupt retrieval in a way no test notices.
        let mut rows = response.data;
        rows.sort_by_key(|row| row.index);
        if rows.iter().enumerate().any(|(at, row)| row.index != at) {
            return Err(ProviderError::Schema(
                "embedding indices are not a contiguous 0..n".to_owned(),
            ));
        }
        Ok(ProviderResponse {
            value: rows.into_iter().map(|row| row.embedding).collect(),
            placement: self.placement(),
            latency_ms,
        })
    }

    fn classify(
        &self,
        request: &ClassifyRequest,
    ) -> Result<ProviderResponse<String>, ProviderError> {
        if request.labels.is_empty() {
            return Err(ProviderError::Schema(
                "classification needs a closed label set".to_owned(),
            ));
        }
        let prompt = format!(
            "{}\n\nAnswer with exactly one of these labels and nothing else: {}.\n\n{}",
            request.instruction.trim(),
            request.labels.join(", "),
            request.input.trim()
        );
        let body = serde_json::json!({
            "model": self.profile.model,
            "max_tokens": CLASSIFY_MAX_TOKENS,
            "temperature": 0,
            "messages": [{"role": "user", "content": prompt}],
        });
        let (response, latency_ms): (ChatResponse, u64) = self.post("/chat/completions", &body)?;
        let answer = response
            .choices
            .first()
            .ok_or_else(|| ProviderError::Schema("no choices in the reply".to_owned()))?
            .message
            .content
            .clone();
        Ok(ProviderResponse {
            value: resolve_label(&answer, &request.labels)?,
            placement: self.placement(),
            latency_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenAiProvider, preview};
    use crate::device::Device;
    use crate::profile::{LlmProfile, Role, Runtime};
    use crate::{LlmProvider, ProviderError};

    fn profile(base_url: &str) -> LlmProfile {
        LlmProfile {
            id: "test".to_owned(),
            role: Role::Embedding,
            model: "m".to_owned(),
            device: Device::Npu,
            runtime: Runtime::OpenAiCompatible,
            base_url: base_url.to_owned(),
            timeout_seconds: 5,
            gate_passed: true,
            note: None,
        }
    }

    #[test]
    fn a_remote_endpoint_cannot_be_constructed() {
        assert!(matches!(
            OpenAiProvider::new(profile("http://api.openai.com/v1")),
            Err(ProviderError::Endpoint(_))
        ));
        assert!(OpenAiProvider::new(profile("http://127.0.0.1:8001")).is_ok());
    }

    #[test]
    fn placement_is_unconfirmed_until_a_runtime_says_otherwise() {
        // OVMS reports no device, so this is the normal case, not an edge one.
        let provider = OpenAiProvider::new(profile("http://127.0.0.1:8001")).unwrap();
        let placement = provider.placement();
        assert!(!placement.is_confirmed());
        assert_eq!(placement.describe(), "npu (unconfirmed)");

        let confirmed = OpenAiProvider::new(profile("http://127.0.0.1:8001"))
            .unwrap()
            .with_observed_device(Device::Npu);
        assert!(confirmed.placement().is_confirmed());

        let fell_back = OpenAiProvider::new(profile("http://127.0.0.1:8001"))
            .unwrap()
            .with_observed_device(Device::Cpu);
        assert_eq!(fell_back.placement().describe(), "npu requested, cpu used");
    }

    #[test]
    fn empty_work_is_refused_before_a_request_is_made() {
        let provider = OpenAiProvider::new(profile("http://127.0.0.1:1")).unwrap();
        assert!(matches!(
            provider.embed(&crate::EmbedRequest { inputs: Vec::new() }),
            Err(ProviderError::Schema(_))
        ));
        assert!(matches!(
            provider.classify(&crate::ClassifyRequest {
                instruction: "pick".to_owned(),
                input: "x".to_owned(),
                labels: Vec::new(),
            }),
            Err(ProviderError::Schema(_))
        ));
    }

    #[test]
    fn an_unparseable_body_is_previewed_not_dumped() {
        let long = "x".repeat(1_000);
        let shown = preview(&long);
        assert_eq!(
            shown.chars().count(),
            201,
            "200 characters plus an ellipsis"
        );
        assert_eq!(preview("  a\nb  "), "a b");
    }
}
