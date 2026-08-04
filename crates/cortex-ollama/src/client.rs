use serde::{Deserialize, Serialize, de::DeserializeOwned};

use cortex_router::RoutingDecision;

use crate::quality::fallback;
use crate::{
    DevicePlacement, DraftAssessment, DraftRequest, ModelInfo, ModelProfile, OllamaConfig,
    OllamaError, QualityFailure, RunningModel, VersionInfo, assess_local_draft,
};

pub struct OllamaClient {
    config: OllamaConfig,
    agent: ureq::Agent,
}

impl OllamaClient {
    pub fn new(mut config: OllamaConfig) -> Result<Self, OllamaError> {
        validate_config(&config)?;
        config.base_url = config.base_url.trim_end_matches('/').to_owned();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(config.request_timeout))
            .timeout_connect(Some(config.connect_timeout))
            .timeout_send_body(Some(config.write_timeout))
            .timeout_recv_body(Some(config.read_timeout))
            .build()
            .into();
        Ok(Self { config, agent })
    }

    /// Discover locally installed models. This client never invokes `/api/pull`.
    pub fn tags(&self) -> Result<Vec<ModelInfo>, OllamaError> {
        let response: TagsResponse = self.get_json("/api/tags")?;
        Ok(response.models)
    }

    pub fn version(&self) -> Result<VersionInfo, OllamaError> {
        self.get_json("/api/version")
    }

    pub fn running_models(&self) -> Result<Vec<RunningModel>, OllamaError> {
        let response: PsResponse = self.get_json("/api/ps")?;
        Ok(response
            .models
            .into_iter()
            .map(|model| RunningModel {
                placement: if model.size_vram > 0 {
                    DevicePlacement::Gpu
                } else {
                    DevicePlacement::Cpu
                },
                name: model.name,
                model: model.model,
                size: model.size,
                size_vram: model.size_vram,
                digest: model.digest,
            })
            .collect())
    }

    /// Request one exact-profile, non-streaming structured draft.
    pub fn draft(
        &self,
        request: &DraftRequest,
        routing: &RoutingDecision,
    ) -> Result<DraftAssessment, OllamaError> {
        if !routing.approves_local_model() {
            return Ok(fallback(None, vec![QualityFailure::RouterRejected]));
        }
        let profile = self
            .config
            .profiles
            .get(&request.profile)
            .ok_or_else(|| OllamaError::UnknownProfile(request.profile.clone()))?;
        validate_budget(
            request.estimated_input_tokens,
            request.requested_output_tokens,
            profile,
        )?;
        let content = self.chat_content(
            &profile.model,
            &request.messages,
            local_draft_schema(),
            request.requested_output_tokens,
            profile.context_tokens,
        )?;
        Ok(assess_local_draft(&content, &request.evidence_ids, routing))
    }

    /// Request one exact-profile completion constrained by a caller schema.
    ///
    /// This deliberately bypasses the draft quality gate: it serves offline
    /// evaluation and shadow observation, where the raw structured text is the
    /// measurement itself. Callers must never grant it workflow authority.
    pub fn structured_chat(
        &self,
        request: &crate::StructuredChatRequest,
    ) -> Result<String, OllamaError> {
        let profile = self
            .config
            .profiles
            .get(&request.profile)
            .ok_or_else(|| OllamaError::UnknownProfile(request.profile.clone()))?;
        validate_budget(
            request.estimated_input_tokens,
            request.requested_output_tokens,
            profile,
        )?;
        self.chat_content(
            &profile.model,
            &request.messages,
            request.schema.clone(),
            request.requested_output_tokens,
            profile.context_tokens,
        )
    }

    /// Embed a bounded batch of inputs with an exact profile. Returns one
    /// vector per input, in order; the count is verified.
    pub fn embed(&self, request: &crate::EmbedRequest) -> Result<Vec<Vec<f32>>, OllamaError> {
        let profile = self
            .config
            .profiles
            .get(&request.profile)
            .ok_or_else(|| OllamaError::UnknownProfile(request.profile.clone()))?;
        if request.inputs.is_empty() || request.inputs.len() > crate::MAX_EMBED_INPUTS {
            return Err(OllamaError::InvalidConfiguration(format!(
                "embed batch must contain 1..={} inputs",
                crate::MAX_EMBED_INPUTS
            )));
        }
        for input in &request.inputs {
            if input.trim().is_empty() {
                return Err(OllamaError::InvalidConfiguration(
                    "embed inputs must not be empty".to_owned(),
                ));
            }
            let estimated = u32::try_from(input.chars().count().div_ceil(4)).unwrap_or(u32::MAX);
            if estimated > profile.max_input_tokens {
                return Err(OllamaError::InputBudgetExceeded {
                    estimated,
                    limit: profile.max_input_tokens,
                });
            }
        }
        let body = EmbedApiRequest {
            model: &profile.model,
            input: &request.inputs,
        };
        let serialized =
            serde_json::to_string(&body).map_err(|error| OllamaError::Json(error.to_string()))?;
        if serialized.len() > self.config.max_request_bytes {
            return Err(OllamaError::RequestTooLarge {
                bytes: serialized.len(),
                limit: self.config.max_request_bytes,
            });
        }
        let response: EmbedApiResponse = self.post_json("/api/embed", &serialized)?;
        if response.embeddings.len() != request.inputs.len() {
            return Err(OllamaError::Json(format!(
                "embed returned {} vectors for {} inputs",
                response.embeddings.len(),
                request.inputs.len()
            )));
        }
        Ok(response.embeddings)
    }

    fn chat_content(
        &self,
        model: &str,
        messages: &[crate::ChatMessage],
        format: serde_json::Value,
        num_predict: u32,
        num_ctx: u32,
    ) -> Result<String, OllamaError> {
        let body = ChatApiRequest {
            model,
            messages,
            stream: false,
            // Structured output must land in `content` deterministically, so
            // hidden thinking is disabled for every profile.
            think: false,
            format,
            options: ChatOptions {
                temperature: 0,
                num_predict,
                num_ctx,
            },
        };
        let serialized =
            serde_json::to_string(&body).map_err(|error| OllamaError::Json(error.to_string()))?;
        if serialized.len() > self.config.max_request_bytes {
            return Err(OllamaError::RequestTooLarge {
                bytes: serialized.len(),
                limit: self.config.max_request_bytes,
            });
        }
        let response: ChatApiResponse = self.post_json("/api/chat", &serialized)?;
        Ok(response.message.content)
    }

    fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, OllamaError> {
        let response = self
            .agent
            .get(&self.endpoint(path))
            .call()
            .map_err(|error| OllamaError::Http(error.to_string()))?;
        self.decode(response)
    }

    fn post_json<T: DeserializeOwned>(&self, path: &str, body: &str) -> Result<T, OllamaError> {
        let response = self
            .agent
            .post(&self.endpoint(path))
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|error| OllamaError::Http(error.to_string()))?;
        self.decode(response)
    }

    fn decode<T: DeserializeOwned>(
        &self,
        mut response: ureq::http::Response<ureq::Body>,
    ) -> Result<T, OllamaError> {
        let body = response
            .body_mut()
            .with_config()
            .limit(self.config.max_response_bytes)
            .read_to_string()
            .map_err(|error| OllamaError::Http(error.to_string()))?;
        serde_json::from_str(&body).map_err(|error| OllamaError::Json(error.to_string()))
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.config.base_url)
    }
}

#[derive(Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct PsResponse {
    #[serde(default)]
    models: Vec<RawRunningModel>,
}

#[derive(Deserialize)]
struct RawRunningModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    size_vram: u64,
    #[serde(default)]
    digest: String,
}

#[derive(Serialize)]
struct ChatApiRequest<'a> {
    model: &'a str,
    messages: &'a [crate::ChatMessage],
    stream: bool,
    think: bool,
    format: serde_json::Value,
    options: ChatOptions,
}

#[derive(Serialize)]
struct ChatOptions {
    temperature: u8,
    num_predict: u32,
    num_ctx: u32,
}

#[derive(Deserialize)]
struct ChatApiResponse {
    message: ChatApiMessage,
}

#[derive(Serialize)]
struct EmbedApiRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct EmbedApiResponse {
    #[serde(default)]
    embeddings: Vec<Vec<f32>>,
}

#[derive(Deserialize)]
struct ChatApiMessage {
    content: String,
}

fn local_draft_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "summary": {"type": "string"},
            "evidenceIds": {"type": "array", "items": {"type": "string"}},
            "unresolved": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["summary", "evidenceIds", "unresolved"],
        "additionalProperties": false
    })
}

fn validate_config(config: &OllamaConfig) -> Result<(), OllamaError> {
    validate_loopback_url(&config.base_url)?;
    if config.max_request_bytes == 0 || config.max_response_bytes == 0 {
        return Err(OllamaError::InvalidConfiguration(
            "body limits must be greater than zero".to_owned(),
        ));
    }
    for (name, profile) in &config.profiles {
        if name.trim().is_empty() || profile.model.trim().is_empty() {
            return Err(OllamaError::InvalidConfiguration(
                "profile names and exact model tags must not be empty".to_owned(),
            ));
        }
        if profile.max_input_tokens == 0
            || profile.max_output_tokens == 0
            || profile.context_tokens == 0
            || profile.max_input_tokens > profile.context_tokens
            || profile.max_output_tokens > profile.context_tokens
        {
            return Err(OllamaError::InvalidConfiguration(format!(
                "profile {name} has invalid token bounds"
            )));
        }
    }
    Ok(())
}

fn validate_loopback_url(base_url: &str) -> Result<(), OllamaError> {
    let rest = base_url
        .strip_prefix("http://")
        .ok_or_else(|| OllamaError::InvalidConfiguration("base URL must use http".to_owned()))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.contains('@') || !path.trim_matches('/').is_empty() {
        return Err(OllamaError::InvalidConfiguration(
            "base URL must not contain credentials or a path".to_owned(),
        ));
    }
    let host = if authority.starts_with('[') {
        authority
            .split_once(']')
            .map(|(host, _)| format!("{host}]"))
    } else {
        Some(authority.split(':').next().unwrap_or_default().to_owned())
    };
    if !matches!(host.as_deref(), Some("localhost" | "127.0.0.1" | "[::1]")) {
        return Err(OllamaError::InvalidConfiguration(
            "base URL host must be localhost or a loopback IP".to_owned(),
        ));
    }
    Ok(())
}

fn validate_budget(
    estimated_input_tokens: u32,
    requested_output_tokens: u32,
    profile: &ModelProfile,
) -> Result<(), OllamaError> {
    if estimated_input_tokens > profile.max_input_tokens {
        return Err(OllamaError::InputBudgetExceeded {
            estimated: estimated_input_tokens,
            limit: profile.max_input_tokens,
        });
    }
    if requested_output_tokens == 0 || requested_output_tokens > profile.max_output_tokens {
        return Err(OllamaError::OutputBudgetExceeded {
            requested: requested_output_tokens,
            limit: profile.max_output_tokens,
        });
    }
    let total = estimated_input_tokens.saturating_add(requested_output_tokens);
    if total > profile.context_tokens {
        return Err(OllamaError::ContextBudgetExceeded {
            requested: total,
            limit: profile.context_tokens,
        });
    }
    Ok(())
}
