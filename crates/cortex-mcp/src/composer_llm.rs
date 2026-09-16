//! Cursor-agent models as a Cortex-internal classifier, not as the coding agent.
//!
//! Talks to a loopback OpenAI-compatible proxy (`CORTEX_COMPOSER_BASE_URL`,
//! default `http://127.0.0.1:8787`). Pick the model with
//! `CORTEX_CLASSIFIER_MODEL` / `CORTEX_COMPOSER_MODEL` or `cortex_prepare`
//! `classifierModel`: `composer`, `sonnet-5`, `opus-5`, `haiku`. The model
//! may only escalate the lexical floor. High-risk work stays upstream.

use cortex_llm::{Device, LlmProfile, OpenAiProvider, Role, Runtime};

use crate::llm_route::{LlmBackend, LlmRouter};

const DEFAULT_COMPOSER_URL: &str = "http://127.0.0.1:8787";

/// Public alias → cursor-agent `--model` id.
pub const CLASSIFIER_ALIASES: &[(&str, &str)] = &[
    ("composer", "composer-2.5"),
    ("composer-2.5", "composer-2.5"),
    ("sonnet-5", "claude-sonnet-5-thinking-high"),
    ("sonnet", "claude-sonnet-5-thinking-high"),
    ("opus-5", "claude-opus-5-thinking-high"),
    ("opus", "claude-opus-5-thinking-high"),
    ("haiku", "claude-haiku-4-5"),
];

pub fn resolve_alias(raw: &str) -> Result<(String, String), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("classifier model is empty".to_owned());
    }
    let key = trimmed.to_ascii_lowercase();
    for (alias, agent) in CLASSIFIER_ALIASES {
        if *alias == key || *agent == trimmed {
            return Ok(((*alias).to_owned(), (*agent).to_owned()));
        }
    }
    if trimmed.starts_with("claude-") || trimmed.starts_with("composer-") {
        return Ok((trimmed.to_owned(), trimmed.to_owned()));
    }
    Err(format!(
        "classifier model must be composer, sonnet-5, opus-5, haiku, or a cursor-agent id; got {trimmed}"
    ))
}

pub(crate) fn router(lookup: impl Fn(&str) -> Option<String>) -> Result<LlmRouter, String> {
    let non_empty = |key: &str| {
        lookup(key).and_then(|value| {
            let trimmed = value.trim().to_owned();
            (!trimmed.is_empty()).then_some(trimmed)
        })
    };
    let requested = non_empty("CORTEX_CLASSIFIER_MODEL")
        .or_else(|| non_empty("CORTEX_COMPOSER_MODEL"))
        .unwrap_or_else(|| "composer".to_owned());
    let (alias, agent_model) = resolve_alias(&requested)?;
    let base_url =
        non_empty("CORTEX_COMPOSER_BASE_URL").unwrap_or_else(|| DEFAULT_COMPOSER_URL.to_owned());
    let api_key = non_empty("CORTEX_COMPOSER_API_KEY").or_else(|| non_empty("CURSOR_API_KEY"));
    let timeout_seconds = non_empty("CORTEX_COMPOSER_TIMEOUT_SECS")
        .and_then(|value| value.parse().ok())
        .unwrap_or(180);
    let profile = LlmProfile {
        id: format!("cursor-{alias}-classifier"),
        role: Role::Classification,
        model: agent_model.clone(),
        device: Device::Gpu,
        runtime: Runtime::OpenAiCompatible,
        base_url,
        timeout_seconds,
        gate_passed: true,
        quantization: None,
        embedding_pooling: None,
        tokenizer: None,
        calibration_ref: None,
        note: Some(format!(
            "Classifier {alias} via loopback OpenAI-compatible proxy ({agent_model})."
        )),
    };
    let provider = OpenAiProvider::with_prefix(profile.clone(), "/v1")
        .map_err(|error| error.to_string())?
        .with_api_key(api_key);
    Ok(LlmRouter::new(
        provider,
        profile.id,
        LlmBackend::Composer,
        Some(alias),
        Some(agent_model),
    ))
}

pub(crate) fn router_for_alias(alias: &str) -> Result<LlmRouter, String> {
    let requested = alias.to_owned();
    router(|key| match key {
        "CORTEX_CLASSIFIER_MODEL" => Some(requested.clone()),
        other => std::env::var(other).ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::{resolve_alias, router};

    #[test]
    fn composer_refuses_a_remote_host() {
        let Err(error) = router(|key| match key {
            "CORTEX_COMPOSER_BASE_URL" => Some("http://api.cursor.com".to_owned()),
            _ => None,
        }) else {
            panic!("cloud Composer is not a Cortex endpoint")
        };
        assert!(error.contains("loopback"), "{error}");
    }

    #[test]
    fn composer_accepts_the_default_loopback_proxy() {
        assert!(router(|_| None).is_ok());
    }

    #[test]
    fn aliases_resolve_to_cursor_agent_ids() {
        let (alias, agent) = resolve_alias("sonnet-5").unwrap();
        assert_eq!(alias, "sonnet-5");
        assert_eq!(agent, "claude-sonnet-5-thinking-high");
        assert_eq!(
            resolve_alias("opus-5").unwrap().1,
            "claude-opus-5-thinking-high"
        );
        assert_eq!(resolve_alias("haiku").unwrap().1, "claude-haiku-4-5");
        assert_eq!(resolve_alias("composer").unwrap().1, "composer-2.5");
    }

    #[test]
    fn unknown_alias_is_rejected() {
        assert!(resolve_alias("spark").is_err());
    }

    #[test]
    fn router_for_alias_sets_the_requested_model() {
        let router = super::router_for_alias("opus-5").expect("loopback opus alias");
        let work = router.decide(&cortex_router::RoutingRequest::new(
            "Tag the version bump for the milestone",
        ));
        assert_eq!(work.classifier_model.as_deref(), Some("opus-5"));
        assert_eq!(
            work.agent_model.as_deref(),
            Some("claude-opus-5-thinking-high")
        );
        assert!(!work.attempted);
    }
}
