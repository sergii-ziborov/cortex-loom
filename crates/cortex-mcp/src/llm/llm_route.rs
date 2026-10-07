//! Opt-in gated local classifier for `route_work`.
//!
//! Off by default. Enabled with `CORTEX_LLM=1` and a profiles file that has a
//! `gatePassed` classification profile (default `config/llm-profiles.json`).
//! The model may only escalate relative to the lexical floor; on any failure
//! or under-call the host keeps the lexical decision. `cortex-router` stays
//! model-free — this module lives in the MCP host only.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cortex_llm::{
    ClassifyRequest, CodingDraftRequest, LlmProvider, OpenAiProvider, ProfileRegistry, Role,
    Runtime, TokenUsage,
};
use serde_json::{Value, json};

use crate::composer_llm;
use cortex_router::{
    Classification, ModelTier, RoutingDecision, RoutingRequest, classify, parse_model_tier,
    policy_tier, route_with_classification, tier_rank,
};

/// Instruction aligned with the calibrated eval prompt: closed tiers, hard
/// escalation for release/security/mutation work, fail closed when unsure.
const CLASSIFICATION_INSTRUCTION: &str = "You classify one engineering task for a routing policy. Reply with exactly one label. Labels: none = deterministic tooling or repository graph analysis; local_small = extracting fields from supplied text only; local_medium = summarizing, compressing, or drafting advice from supplied evidence only; upstream_strong = everything else. Any task that creates, fixes, implements, changes, or updates code or state is upstream_strong. Any task touching security, authentication, concurrency, migration, release, version bump, git tag, deployment, or publication is upstream_strong. When uncertain choose upstream_strong.";

const TIER_LABELS: &[&str] = &["none", "local_small", "local_medium", "upstream_strong"];
static COMPOSER_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmBackend {
    Off,
    Local,
    Composer,
}

impl LlmBackend {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Local => "local",
            Self::Composer => "composer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmRouteConfig {
    pub enabled: bool,
    pub backend: Option<String>,
    pub profiles_path: PathBuf,
}

impl LlmRouteConfig {
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let non_empty = |key: &str| {
            lookup(key).and_then(|value| {
                let trimmed = value.trim().to_owned();
                (!trimmed.is_empty()).then_some(trimmed)
            })
        };
        Self {
            enabled: non_empty("CORTEX_LLM").is_some_and(|value| value == "1"),
            backend: non_empty("CORTEX_LLM_BACKEND"),
            profiles_path: non_empty("CORTEX_LLM_PROFILES")
                .map_or_else(|| PathBuf::from("config/llm-profiles.json"), PathBuf::from),
        }
    }

    /// `CORTEX_LLM_BACKEND` wins. Unset + `CORTEX_LLM=1` is local. Else off.
    pub fn resolve_backend(&self) -> Result<LlmBackend, String> {
        match self.backend.as_deref().map(str::trim) {
            Some("off") => Ok(LlmBackend::Off),
            Some("local") => Ok(LlmBackend::Local),
            Some("composer") => Ok(LlmBackend::Composer),
            Some(other) => Err(format!(
                "CORTEX_LLM_BACKEND must be off, local, or composer; got {other}"
            )),
            None if self.enabled => Ok(LlmBackend::Local),
            None => Ok(LlmBackend::Off),
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        !matches!(self.resolve_backend(), Ok(LlmBackend::Off) | Err(_))
    }
}

/// Result of one `route_work` decision, with optional classifier latency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedWork {
    pub decision: RoutingDecision,
    pub latency_ms: Option<u64>,
    pub classifier_profile: Option<String>,
    pub usage: Option<TokenUsage>,
    pub backend: LlmBackend,
    pub attempted: bool,
    pub succeeded: bool,
    pub warning: Option<String>,
    pub classifier_model: Option<String>,
    pub agent_model: Option<String>,
}

impl RoutedWork {
    #[must_use]
    pub fn lexical(decision: RoutingDecision) -> Self {
        Self {
            decision,
            latency_ms: None,
            classifier_profile: None,
            usage: None,
            backend: LlmBackend::Off,
            attempted: false,
            succeeded: false,
            warning: None,
            classifier_model: None,
            agent_model: None,
        }
    }

    #[must_use]
    pub fn internal_model_json(&self) -> Value {
        let total = self.usage.map(TokenUsage::total);
        json!({
            "mode": self.backend.as_str(),
            "role": "routing_classifier",
            "affectsEvidence": false,
            "called": self.attempted,
            "succeeded": self.succeeded,
            "fallbackUsed": self.attempted && !self.succeeded,
            "usageKind": if self.usage.is_some() { "provider_reported" } else if self.attempted { "unknown" } else { "not_called" },
            "skipReason": if !self.attempted && self.backend != LlmBackend::Off { Some("lexical_floor_upstream_strong") } else { None },
            "profile": self.classifier_profile,
            "classifierModel": self.classifier_model,
            "agentModel": self.agent_model,
            "promptTokens": self.usage.map(|usage| usage.prompt_tokens),
            "completionTokens": self.usage.map(|usage| usage.completion_tokens),
            "totalTokens": total,
            "composerTokens": if matches!(self.backend, LlmBackend::Composer) { total } else { None },
            "localTokens": if matches!(self.backend, LlmBackend::Local) { total } else { None },
            "warning": self.warning,
        })
    }
}

/// Gated OpenAI-compatible classifier used by the MCP host.
pub struct LlmRouter {
    provider: OpenAiProvider,
    profile_id: String,
    backend: LlmBackend,
    classifier_model: Option<String>,
    agent_model: Option<String>,
    /// Serializes hot-path calls so one OVMS worker is not flooded by the host.
    lock: Mutex<()>,
}

pub struct CodingModelResponse {
    pub content: String,
    pub usage: Option<TokenUsage>,
    pub latency_ms: u64,
    pub profile: String,
    pub model: Option<String>,
}

impl LlmRouter {
    /// The model drafts source-anchored suggestions; it never changes the
    /// upstream execution route or obtains mutation authority.
    pub fn draft_coding(
        &self,
        request: &CodingDraftRequest,
    ) -> Result<CodingModelResponse, String> {
        let _shared = if self.backend == LlmBackend::Composer {
            Some(
                COMPOSER_LOCK
                    .lock()
                    .map_err(|_| "composer draft lock poisoned".to_owned())?,
            )
        } else {
            None
        };
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "coding draft lock poisoned".to_owned())?;
        let (content, latency_ms, usage) = if self.provider.profile().runtime == Runtime::Ollama {
            super::ollama_coding::draft(self.provider.profile(), request)?
        } else {
            let result = self
                .provider
                .coding_draft(request)
                .map_err(|error| error.to_string())?;
            (result.value, result.latency_ms, result.usage)
        };
        Ok(CodingModelResponse {
            content,
            usage,
            latency_ms,
            profile: self.profile_id.clone(),
            model: self.agent_model.clone(),
        })
    }
    /// Build when explicitly configured; `Ok(None)` when inactive.
    ///
    /// # Errors
    ///
    /// Returns a human-readable reason when profiles cannot be loaded or no
    /// calibrated classification profile exists.
    pub fn from_config(config: &LlmRouteConfig) -> Result<Option<Self>, String> {
        match config.resolve_backend()? {
            LlmBackend::Off => Ok(None),
            LlmBackend::Local => Self::from_local(config).map(Some),
            LlmBackend::Composer => Ok(Some(composer_llm::router(|key| std::env::var(key).ok())?)),
        }
    }

    #[must_use]
    pub(crate) fn new(
        provider: OpenAiProvider,
        profile_id: String,
        backend: LlmBackend,
        classifier_model: Option<String>,
        agent_model: Option<String>,
    ) -> Self {
        Self {
            provider,
            profile_id,
            backend,
            classifier_model,
            agent_model,
            lock: Mutex::new(()),
        }
    }

    fn from_local(config: &LlmRouteConfig) -> Result<Self, String> {
        let registry = load_registry(&config.profiles_path)?;
        let profile = registry
            .select(Role::Classification)
            .map_err(|error| error.to_string())?
            .clone();
        let prefix = match profile.runtime {
            Runtime::OpenAiCompatible => cortex_llm::openai::DEFAULT_PATH_PREFIX,
            Runtime::Ollama => "/v1",
        };
        let provider = OpenAiProvider::with_prefix(profile.clone(), prefix)
            .map_err(|error| error.to_string())?;
        Ok(Self::new(
            provider,
            profile.id,
            LlmBackend::Local,
            None,
            Some(profile.model.clone()),
        ))
    }

    /// Lexical floor first; model may only escalate. Failures keep lexical.
    /// The classifier is skipped whenever the lexical floor has reached
    /// `upstream_strong`: escalation cannot change the route at that ceiling.
    #[must_use]
    pub fn decide(&self, request: &RoutingRequest) -> RoutedWork {
        let lexical = classify(&request.task);
        if !classifier_worth_calling(request, lexical) {
            return self.skipped(request, lexical);
        }
        self.finish(request, lexical, self.ask_tier(&request.task))
    }

    /// Prepare follows the same routing policy. Calling a model only to
    /// produce usage telemetry wastes tokens without changing the decision.
    #[must_use]
    pub fn decide_prepare(&self, request: &RoutingRequest) -> RoutedWork {
        self.decide(request)
    }

    fn skipped(&self, request: &RoutingRequest, lexical: Classification) -> RoutedWork {
        RoutedWork {
            decision: route_with_classification(request, lexical),
            latency_ms: None,
            classifier_profile: None,
            usage: None,
            backend: self.backend,
            attempted: false,
            succeeded: false,
            warning: None,
            classifier_model: self.classifier_model.clone(),
            agent_model: self.agent_model.clone(),
        }
    }

    fn finish(
        &self,
        request: &RoutingRequest,
        lexical: Classification,
        asked: Result<(ModelTier, u64, Option<TokenUsage>), String>,
    ) -> RoutedWork {
        match asked {
            Ok((llm_tier, latency_ms, usage)) => {
                let lexical_tier = policy_tier(lexical.class);
                let accepted = tier_rank(llm_tier) >= tier_rank(lexical_tier);
                let classification = merge_tiers(lexical, Some(llm_tier));
                RoutedWork {
                    decision: route_with_classification(request, classification),
                    latency_ms: accepted.then_some(latency_ms),
                    classifier_profile: accepted.then(|| self.profile_id.clone()),
                    usage,
                    backend: self.backend,
                    attempted: true,
                    succeeded: true,
                    warning: None,
                    classifier_model: self.classifier_model.clone(),
                    agent_model: self.agent_model.clone(),
                }
            }
            Err(error) => RoutedWork {
                decision: route_with_classification(request, lexical),
                latency_ms: None,
                classifier_profile: None,
                usage: None,
                backend: self.backend,
                attempted: true,
                succeeded: false,
                warning: Some(error),
                classifier_model: self.classifier_model.clone(),
                agent_model: self.agent_model.clone(),
            },
        }
    }

    fn ask_tier(&self, task: &str) -> Result<(ModelTier, u64, Option<TokenUsage>), String> {
        // Per-request alias routers have separate Mutex values; serialize all
        // of them against the same loopback Composer device/service.
        let _shared = if self.backend == LlmBackend::Composer {
            Some(
                COMPOSER_LOCK
                    .lock()
                    .map_err(|_| "composer classifier lock poisoned".to_owned())?,
            )
        } else {
            None
        };
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "classifier lock poisoned".to_owned())?;
        let labels = TIER_LABELS
            .iter()
            .map(|label| (*label).to_owned())
            .collect::<Vec<_>>();
        let response = self
            .provider
            .classify(&ClassifyRequest {
                instruction: CLASSIFICATION_INSTRUCTION.to_owned(),
                input: task.to_owned(),
                labels,
            })
            .map_err(|error| error.to_string())?;
        let tier = parse_model_tier(&response.value)
            .ok_or_else(|| format!("unrecognised tier label {}", response.value))?;
        Ok((tier, response.latency_ms, response.usage))
    }
}

fn load_registry(path: &Path) -> Result<ProfileRegistry, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("invalid {}: {error}", path.display()))
}

/// When the lexical policy is already at the ceiling, a classifier cannot
/// change the route; calling it can only add token cost and latency.
#[must_use]
pub fn classifier_worth_calling(request: &RoutingRequest, lexical: Classification) -> bool {
    route_with_classification(request, lexical).model_tier != ModelTier::UpstreamStrong
}

/// Pure merge used by tests and documentation of the under-call floor.
#[must_use]
pub fn merge_tiers(lexical: Classification, llm: Option<ModelTier>) -> Classification {
    let Some(llm_tier) = llm else {
        return lexical;
    };
    if tier_rank(llm_tier) < tier_rank(policy_tier(lexical.class)) {
        return lexical;
    }
    cortex_router::classification_for_tier(llm_tier, lexical)
}

#[cfg(test)]
mod tests {
    use super::{LlmBackend, LlmRouteConfig, merge_tiers};
    use cortex_domain::RiskLevel;
    use cortex_router::{Classification, ModelTier, TaskClass, classify, policy_tier};

    #[test]
    fn inactive_without_the_env_flag() {
        let config = LlmRouteConfig::from_lookup(|_| None);
        assert!(!config.is_active());
        assert_eq!(config.resolve_backend().unwrap(), LlmBackend::Off);
    }

    #[test]
    fn backend_flag_overrides_the_legacy_llm_switch() {
        let off = LlmRouteConfig::from_lookup(|key| match key {
            "CORTEX_LLM" => Some("1".to_owned()),
            "CORTEX_LLM_BACKEND" => Some("off".to_owned()),
            _ => None,
        });
        assert_eq!(off.resolve_backend().unwrap(), LlmBackend::Off);
        let composer = LlmRouteConfig::from_lookup(|key| match key {
            "CORTEX_LLM_BACKEND" => Some("composer".to_owned()),
            _ => None,
        });
        assert_eq!(composer.resolve_backend().unwrap(), LlmBackend::Composer);
        assert!(composer.is_active());
        let bad = LlmRouteConfig::from_lookup(|key| match key {
            "CORTEX_LLM_BACKEND" => Some("spark".to_owned()),
            _ => None,
        });
        assert!(bad.resolve_backend().is_err());
    }

    #[test]
    fn obvious_upstream_does_not_call_the_classifier() {
        let release = cortex_router::RoutingRequest::new("Tag the version bump for the milestone");
        let lexical = classify(&release.task);
        assert!(!super::classifier_worth_calling(&release, lexical));
        let mixed = cortex_router::RoutingRequest::new("Переименуй ArchiveOptions и bump the tag");
        assert!(!super::classifier_worth_calling(
            &mixed,
            classify(&mixed.task)
        ));
        let mixed_detectors = "summarize the repository graph and extract fields";
        assert!(cortex_router::detector_disagreement(mixed_detectors));
        let analysis = cortex_router::RoutingRequest::new(mixed_detectors);
        assert!(super::classifier_worth_calling(
            &analysis,
            classify(&analysis.task)
        ));
        let mutation =
            cortex_router::RoutingRequest::new("Analyze repository graph and fix the code");
        assert!(!super::classifier_worth_calling(
            &mutation,
            classify(&mutation.task)
        ));
    }

    #[test]
    fn prepare_skips_a_paid_classifier_when_coding_already_routes_upstream() {
        let router = crate::composer_llm::router(|_| None).unwrap();
        let request = cortex_router::RoutingRequest::new(
            "Fix the repository graph code and update its tests",
        );
        let prepared = router.decide_prepare(&request);
        assert!(!prepared.attempted);
        assert_eq!(prepared.decision, cortex_router::route(&request));
        let model = prepared.internal_model_json();
        assert_eq!(model["mode"], "composer");
        assert_eq!(model["usageKind"], "not_called");
        assert_eq!(model["skipReason"], "lexical_floor_upstream_strong");
    }

    #[test]
    fn under_call_keeps_the_lexical_release_floor() {
        let lexical = classify("Tag the version bump for the milestone");
        assert_eq!(policy_tier(lexical.class), ModelTier::UpstreamStrong);
        let merged = merge_tiers(lexical, Some(ModelTier::LocalMedium));
        assert_eq!(merged.class, lexical.class);
    }

    #[test]
    fn escalation_above_lexical_is_allowed() {
        let lexical = Classification {
            class: TaskClass::AdvisoryDraft,
            risk: RiskLevel::Low,
            mutation_likely: false,
        };
        let merged = merge_tiers(lexical, Some(ModelTier::UpstreamStrong));
        assert_eq!(merged.class, TaskClass::Ambiguous);
    }
}
