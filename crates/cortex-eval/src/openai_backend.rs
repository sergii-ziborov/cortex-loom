//! Calibration against an OpenAI-compatible local runtime.
//!
//! The gates were written against Ollama, which is the runtime that happened
//! to be installed. They are not *about* Ollama: a gate verdict belongs to a
//! *(model, device, runtime)* triple, so a profile that passed on Ollama/CPU
//! has proved nothing about the same weights compiled for an accelerator by
//! `OpenVINO` Model Server. This backend exists so the same fixtures, the same
//! comparators and the same pinned prompts can be pointed at the accelerator
//! and produce a verdict that means something.
//!
//! Loopback is enforced through `cortex_llm::LoopbackUrl` rather than
//! re-checked here, so there is exactly one implementation of that rule in
//! the workspace.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use cortex_llm::LoopbackUrl;
use cortex_ollama::{EmbedRequest, ModelInfo, RunningModel, StructuredChatRequest};
use serde::Deserialize;

use crate::backend::{EvalBackend, TimedContent, TimedEmbeddings};

/// OVMS serves the `OpenAI` surface under `/v3`; llama.cpp and LM Studio use
/// `/v1`.
pub const DEFAULT_PREFIX: &str = "/v3";

pub struct OpenAiEvalBackend {
    base: LoopbackUrl,
    prefix: String,
    agent: ureq::Agent,
    /// Model id to send, overriding whatever the profile resolves to.
    ///
    /// A servable is named at deployment (`--model_name`), which need not
    /// match the weights' upstream name. Keeping them separate stops a
    /// mismatch from being silently read as "model absent".
    model: Option<String>,
    /// Profile id to model tag, mirroring `OllamaConfig::profiles`.
    ///
    /// The runner addresses profiles by **id**, not by model — the Ollama
    /// client resolves that internally, so a backend that skips the step
    /// sends the profile id as a model name and every call 404s.
    profiles: BTreeMap<String, String>,
}

impl OpenAiEvalBackend {
    /// # Errors
    ///
    /// Returns a message when the base URL is not a loopback address.
    pub fn new(base_url: &str, timeout: Duration, model: Option<String>) -> Result<Self, String> {
        let base = LoopbackUrl::parse(base_url).map_err(|error| error.to_string())?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .timeout_connect(Some(Duration::from_secs(3)))
            .build()
            .into();
        Ok(Self {
            base,
            prefix: DEFAULT_PREFIX.to_owned(),
            agent,
            model,
            profiles: BTreeMap::new(),
        })
    }

    #[must_use]
    pub fn with_prefix(mut self, prefix: &str) -> Self {
        prefix.trim_end_matches('/').clone_into(&mut self.prefix);
        self
    }

    /// Register the profile-id to model-tag mapping the runner will address.
    #[must_use]
    pub fn with_profile(mut self, id: impl Into<String>, model: impl Into<String>) -> Self {
        self.profiles.insert(id.into(), model.into());
        self
    }

    fn url(&self, path: &str) -> String {
        self.base.join(&format!("{}{path}", self.prefix))
    }

    /// Resolve what the runner addressed into what the runtime serves.
    ///
    /// An explicit `--servable` wins; otherwise the registered mapping; and
    /// failing both, the string as given, so a single-model deployment needs
    /// no configuration at all.
    fn model_for(&self, profile_id: &str) -> String {
        self.model
            .clone()
            .or_else(|| self.profiles.get(profile_id).cloned())
            .unwrap_or_else(|| profile_id.to_owned())
    }

    fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<(String, u64), String> {
        let started = Instant::now();
        let mut response = self
            .agent
            .post(&self.url(path))
            .header("content-type", "application/json")
            .send(body.to_string())
            .map_err(|error| error.to_string())?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())?;
        Ok((
            text,
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelRow>,
}

#[derive(Debug, Deserialize)]
struct ModelRow {
    id: String,
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

#[derive(Debug, Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingRow>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingRow {
    index: usize,
    embedding: Vec<f32>,
}

impl EvalBackend for OpenAiEvalBackend {
    fn version(&self) -> Result<String, String> {
        // The OpenAI surface has no version endpoint. Reaching the model list
        // at all is the honest liveness signal; inventing a version string
        // would put a number in the report that means nothing.
        self.installed_models()
            .map(|models| format!("openai-compatible ({} servable(s))", models.len()))
    }

    fn installed_models(&self) -> Result<Vec<ModelInfo>, String> {
        let mut response = self
            .agent
            .get(&self.url("/models"))
            .call()
            .map_err(|error| error.to_string())?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())?;
        let parsed: ModelsResponse =
            serde_json::from_str(&text).map_err(|error| format!("{error}: {text}"))?;
        Ok(parsed
            .data
            .into_iter()
            .map(|row| ModelInfo {
                name: row.id.clone(),
                model: row.id,
                size: 0,
                digest: String::new(),
            })
            .collect())
    }

    fn running_models(&self) -> Result<Vec<RunningModel>, String> {
        // Measured against OVMS 2026.3.0: neither `/v1/config` nor `/metrics`
        // reports the device a servable was compiled for. An empty list is the
        // truth — "we cannot see" — and the report shows it as such rather
        // than echoing back the device the operator asked for.
        Ok(Vec::new())
    }

    fn structured(&self, request: &StructuredChatRequest) -> Result<TimedContent, String> {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|message| {
                serde_json::json!({
                    "role": format!("{:?}", message.role).to_lowercase(),
                    "content": message.content,
                })
            })
            .collect();
        let body = serde_json::json!({
            "model": self.model_for(&request.profile),
            "messages": messages,
            "max_tokens": request.requested_output_tokens,
            "temperature": 0,
            "response_format": {
                "type": "json_schema",
                "json_schema": {"name": "eval", "strict": true, "schema": request.schema},
            },
        });
        let (text, latency_ms) = self.post_json("/chat/completions", &body)?;
        let parsed: ChatResponse =
            serde_json::from_str(&text).map_err(|error| format!("{error}: {}", clip(&text)))?;
        let content = parsed
            .choices
            .first()
            .ok_or_else(|| "no choices in the reply".to_owned())?
            .message
            .content
            .clone();
        Ok(TimedContent {
            content,
            latency_ms,
        })
    }

    fn embed(&self, request: &EmbedRequest) -> Result<TimedEmbeddings, String> {
        if request.inputs.is_empty() {
            return Err("no inputs to embed".to_owned());
        }
        let body = serde_json::json!({
            "model": self.model_for(&request.profile),
            "input": request.inputs,
        });
        let (text, latency_ms) = self.post_json("/embeddings", &body)?;
        let parsed: EmbeddingsResponse =
            serde_json::from_str(&text).map_err(|error| format!("{error}: {}", clip(&text)))?;
        if parsed.data.len() != request.inputs.len() {
            return Err(format!(
                "asked for {} vectors, received {}",
                request.inputs.len(),
                parsed.data.len()
            ));
        }
        // Row order is not promised by the OpenAI shape, and a permuted batch
        // would silently corrupt every retrieval metric computed from it.
        let mut rows = parsed.data;
        rows.sort_by_key(|row| row.index);
        if rows.iter().enumerate().any(|(at, row)| row.index != at) {
            return Err("embedding indices are not a contiguous 0..n".to_owned());
        }
        Ok(TimedEmbeddings {
            vectors: rows.into_iter().map(|row| row.embedding).collect(),
            latency_ms,
        })
    }
}

fn clip(text: &str) -> String {
    let cleaned = text.trim().replace('\n', " ");
    if cleaned.chars().count() <= 200 {
        return cleaned;
    }
    cleaned.chars().take(200).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::OpenAiEvalBackend;
    use std::time::Duration;

    #[test]
    fn a_remote_runtime_cannot_be_calibrated_against() {
        assert!(
            OpenAiEvalBackend::new("http://api.openai.com/v1", Duration::from_secs(5), None)
                .is_err()
        );
        assert!(
            OpenAiEvalBackend::new("http://127.0.0.1:8000", Duration::from_secs(5), None).is_ok()
        );
    }

    #[test]
    fn the_deployed_servable_name_overrides_the_profile_tag() {
        // `--model_name qwen25-1.5b` need not match the weights' upstream
        // name; without the override the runtime answers "model not found"
        // and the runner would record it as absent rather than misconfigured.
        let backend =
            OpenAiEvalBackend::new("http://127.0.0.1:8000", Duration::from_secs(5), None).unwrap();
        assert_eq!(backend.model_for("unmapped"), "unmapped");
        let mapped = OpenAiEvalBackend::new("http://127.0.0.1:8000", Duration::from_secs(5), None)
            .unwrap()
            .with_profile("gpu-embedding", "qwen3-embed");
        assert_eq!(
            mapped.model_for("gpu-embedding"),
            "qwen3-embed",
            "the runner addresses profiles by id, not by model"
        );
        let pinned = OpenAiEvalBackend::new(
            "http://127.0.0.1:8000",
            Duration::from_secs(5),
            Some("qwen25-1.5b".to_owned()),
        )
        .unwrap();
        assert_eq!(pinned.model_for("anything"), "qwen25-1.5b");
    }

    #[test]
    fn paths_are_joined_under_the_configured_prefix() {
        let backend =
            OpenAiEvalBackend::new("http://127.0.0.1:8000/", Duration::from_secs(5), None).unwrap();
        assert_eq!(
            backend.url("/embeddings"),
            "http://127.0.0.1:8000/v3/embeddings"
        );
        let v1 = OpenAiEvalBackend::new("http://127.0.0.1:8080", Duration::from_secs(5), None)
            .unwrap()
            .with_prefix("/v1");
        assert_eq!(
            v1.url("/chat/completions"),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
    }
}
