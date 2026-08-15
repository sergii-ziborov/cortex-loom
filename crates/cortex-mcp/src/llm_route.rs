//! Opt-in gated local classifier for `route_work`.
//!
//! Off by default. Enabled with `CORTEX_LLM=1` and a profiles file that has a
//! `gatePassed` classification profile (default `config/llm-profiles.json`).
//! The model may only escalate relative to the lexical floor; on any failure
//! or under-call the host keeps the lexical decision. `cortex-router` stays
//! model-free — this module lives in the MCP host only.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cortex_llm::{ClassifyRequest, LlmProvider, OpenAiProvider, ProfileRegistry, Role, Runtime};
use cortex_router::{
    Classification, ModelTier, RoutingDecision, RoutingRequest, TaskClass, classify,
    detector_disagreement, mixed_script, parse_model_tier, policy_tier, route_with_classification,
    tier_rank,
};

/// Instruction aligned with the calibrated eval prompt: closed tiers, hard
/// escalation for release/security/mutation work, fail closed when unsure.
const CLASSIFICATION_INSTRUCTION: &str = "You classify one engineering task for a routing policy. Reply with exactly one label. Labels: none = deterministic tooling or repository graph analysis; local_small = extracting fields from supplied text only; local_medium = summarizing, compressing, or drafting advice from supplied evidence only; upstream_strong = everything else. Any task that creates, fixes, implements, changes, or updates code or state is upstream_strong. Any task touching security, authentication, concurrency, migration, release, version bump, git tag, deployment, or publication is upstream_strong. When uncertain choose upstream_strong.";

const TIER_LABELS: &[&str] = &["none", "local_small", "local_medium", "upstream_strong"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmRouteConfig {
    pub enabled: bool,
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
            profiles_path: non_empty("CORTEX_LLM_PROFILES")
                .map_or_else(|| PathBuf::from("config/llm-profiles.json"), PathBuf::from),
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled
    }
}

/// Result of one `route_work` decision, with optional classifier latency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedWork {
    pub decision: RoutingDecision,
    pub latency_ms: Option<u64>,
    pub classifier_profile: Option<String>,
}

/// Gated OpenAI-compatible classifier used by the MCP host.
pub struct LlmRouter {
    provider: OpenAiProvider,
    profile_id: String,
    /// Serializes hot-path calls so one OVMS worker is not flooded by the host.
    lock: Mutex<()>,
}

impl LlmRouter {
    /// Build when explicitly configured; `Ok(None)` when inactive.
    ///
    /// # Errors
    ///
    /// Returns a human-readable reason when profiles cannot be loaded or no
    /// calibrated classification profile exists.
    pub fn from_config(config: &LlmRouteConfig) -> Result<Option<Self>, String> {
        if !config.is_active() {
            return Ok(None);
        }
        let registry = load_registry(&config.profiles_path)?;
        let profile = registry
            .select(Role::Classification)
            .map_err(|error| error.to_string())?
            .clone();
        if !matches!(profile.runtime, Runtime::OpenAiCompatible) {
            return Err(format!(
                "classification profile {} uses {:?}; only open_ai_compatible is wired",
                profile.id, profile.runtime
            ));
        }
        let provider = OpenAiProvider::new(profile.clone()).map_err(|error| error.to_string())?;
        Ok(Some(Self {
            profile_id: profile.id,
            provider,
            lock: Mutex::new(()),
        }))
    }

    /// Lexical floor first; model may only escalate. Failures keep lexical.
    /// The classifier is skipped when the lexical floor is already
    /// `upstream_strong` and the request is not mixed-script or ambiguous.
    #[must_use]
    pub fn decide(&self, request: &RoutingRequest) -> RoutedWork {
        let lexical = classify(&request.task);
        if !classifier_worth_calling(&request.task, lexical) {
            return RoutedWork {
                decision: route_with_classification(request, lexical),
                latency_ms: None,
                classifier_profile: None,
            };
        }
        match self.ask_tier(&request.task) {
            Ok((llm_tier, latency_ms)) => {
                let lexical_tier = policy_tier(lexical.class);
                let accepted = tier_rank(llm_tier) >= tier_rank(lexical_tier);
                let classification = merge_tiers(lexical, Some(llm_tier));
                RoutedWork {
                    decision: route_with_classification(request, classification),
                    latency_ms: accepted.then_some(latency_ms),
                    classifier_profile: accepted.then(|| self.profile_id.clone()),
                }
            }
            Err(_) => RoutedWork {
                decision: route_with_classification(request, lexical),
                latency_ms: None,
                classifier_profile: None,
            },
        }
    }

    fn ask_tier(&self, task: &str) -> Result<(ModelTier, u64), String> {
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
        Ok((tier, response.latency_ms))
    }
}

fn load_registry(path: &Path) -> Result<ProfileRegistry, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("invalid {}: {error}", path.display()))
}

/// When the lexical policy is already at the ceiling, an 8B classifier
/// cannot save tokens — it can only add latency.
#[must_use]
pub fn classifier_worth_calling(task: &str, lexical: Classification) -> bool {
    if matches!(lexical.class, TaskClass::Ambiguous) {
        return true;
    }
    if mixed_script(task) {
        return true;
    }
    if detector_disagreement(task) {
        return true;
    }
    policy_tier(lexical.class) != ModelTier::UpstreamStrong
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
    use super::{LlmRouteConfig, merge_tiers};
    use cortex_domain::RiskLevel;
    use cortex_router::{Classification, ModelTier, TaskClass, classify, policy_tier};

    #[test]
    fn inactive_without_the_env_flag() {
        let config = LlmRouteConfig::from_lookup(|_| None);
        assert!(!config.is_active());
    }

    #[test]
    fn obvious_upstream_does_not_call_the_classifier() {
        let lexical = classify("Tag the version bump for the milestone");
        assert!(!super::classifier_worth_calling(
            "Tag the version bump for the milestone",
            lexical
        ));
        let mixed = classify("Переименуй ArchiveOptions и bump the tag");
        assert!(super::classifier_worth_calling(
            "Переименуй ArchiveOptions и bump the tag",
            mixed
        ));
        let mixed_detectors = "summarize the repository graph and extract fields";
        assert!(cortex_router::detector_disagreement(mixed_detectors));
        assert!(super::classifier_worth_calling(
            mixed_detectors,
            classify(mixed_detectors)
        ));
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
