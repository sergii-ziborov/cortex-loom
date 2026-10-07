//! Native Ollama structured chat for the advisory coding role.

use std::time::{Duration, Instant};

use cortex_context::estimate_tokens;
use cortex_llm::{CodingDraftRequest, LlmProfile, TokenUsage, coding_draft_schema};
use cortex_ollama::{ChatMessage, ModelProfile, OllamaClient, OllamaConfig, StructuredChatRequest};

pub(super) fn draft(
    profile: &LlmProfile,
    request: &CodingDraftRequest,
) -> Result<(String, u64, Option<TokenUsage>), String> {
    let timeout = Duration::from_secs(u64::from(profile.timeout_seconds.clamp(1, 105)));
    let config = OllamaConfig {
        base_url: profile.base_url.clone(),
        request_timeout: timeout,
        read_timeout: timeout,
        ..OllamaConfig::default()
    }
    .with_profile(
        "coding",
        ModelProfile::new(profile.model.clone(), 7_168, 1_024, 8_192),
    );
    let client = OllamaClient::new(config).map_err(|error| error.to_string())?;
    let prompt = request.prompt();
    let asked = StructuredChatRequest {
        profile: "coding".to_owned(),
        messages: vec![ChatMessage::user(prompt.clone())],
        schema: coding_draft_schema(),
        estimated_input_tokens: estimate_tokens(&prompt),
        requested_output_tokens: request.max_output_tokens,
    };
    let started = Instant::now();
    let response = client
        .structured_chat_with_usage(&asked)
        .map_err(|error| error.to_string())?;
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let usage = response.prompt_tokens.zip(response.completion_tokens).map(
        |(prompt_tokens, completion_tokens)| TokenUsage {
            prompt_tokens,
            completion_tokens,
        },
    );
    Ok((response.content, latency_ms, usage))
}
