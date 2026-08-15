//! Opt-in semantic ordering for evidence compilation.
//!
//! Off by default. `CORTEX_SEMANTIC=1` is not enough: a matching
//! [`CalibrationArtifact`] must authorize the live embedding profile. A
//! profile JSON `gatePassed` flag is ignored. Scores only reorder fragments
//! within a priority band; any failure falls back to deterministic order.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use cortex_context::ranking::{
    ADJACENCY_KIND, EvidenceLink, RANKING_FIXTURE_SET, RANKING_VERSION, evidence_adjacency,
    rank_hybrid_graph,
};
use cortex_llm::{
    ADJACENCY_EVIDENCE_SPANS, CalibrationArtifact, EmbedRequest, LlmProvider, OpenAiProvider,
    ProfileRegistry, Role, Runtime, RuntimeAttestation,
};

const MAX_EMBED_CHARS: usize = 6_000;
const MAX_EMBED_CACHE: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticConfig {
    pub enabled: bool,
    pub model: Option<String>,
    pub profiles_path: PathBuf,
    pub timeout_ms: u64,
}

impl SemanticConfig {
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
            enabled: non_empty("CORTEX_SEMANTIC").is_some_and(|value| value == "1"),
            model: non_empty("CORTEX_SEMANTIC_MODEL"),
            profiles_path: non_empty("CORTEX_LLM_PROFILES")
                .map_or_else(|| PathBuf::from("config/llm-profiles.json"), PathBuf::from),
            timeout_ms: non_empty("CORTEX_SEMANTIC_TIMEOUT_MS")
                .and_then(|value| value.parse().ok())
                .unwrap_or(30_000),
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled
    }
}

pub struct SemanticScorer {
    provider: OpenAiProvider,
    provenance: String,
    cache: Mutex<HashMap<String, Vec<f32>>>,
}

impl SemanticScorer {
    /// Build when `CORTEX_SEMANTIC=1` and a calibration artifact authorizes
    /// the live embedding profile. `Ok(None)` when the flag is off.
    pub fn from_config(config: &SemanticConfig) -> Result<Option<Self>, String> {
        if !config.is_active() {
            return Ok(None);
        }
        let registry = load_registry(&config.profiles_path)?;
        let profile =
            authorized_embedding(&registry, config.model.as_deref(), &config.profiles_path)?;
        if !matches!(profile.runtime, Runtime::OpenAiCompatible) {
            return Err(format!(
                "semantic profile {} uses {:?}; only the OpenAI-compatible embedding path is attested",
                profile.id, profile.runtime
            ));
        }
        let mut profile = profile.clone();
        if config.timeout_ms > 0 {
            profile.timeout_seconds = u32::try_from(config.timeout_ms / 1_000)
                .unwrap_or(u32::MAX)
                .max(1);
        }
        let provider = OpenAiProvider::new(profile.clone()).map_err(|error| error.to_string())?;
        let provenance = format!(
            "{}/hybrid_graph/{RANKING_VERSION}/{ADJACENCY_KIND}",
            profile.model
        );
        Ok(Some(Self {
            provider,
            provenance,
            cache: Mutex::new(HashMap::new()),
        }))
    }

    #[must_use]
    pub fn provenance(&self) -> String {
        self.provenance.clone()
    }

    /// Score fragments with the same `hybrid_graph` pipeline the eval gate uses.
    pub fn score(
        &self,
        task: &str,
        fragments: &[EvidenceLink<'_>],
    ) -> Result<HashMap<String, f64>, String> {
        if fragments.is_empty() {
            return Ok(HashMap::new());
        }
        let texts: Vec<String> = fragments
            .iter()
            .map(|item| item.content.chars().take(MAX_EMBED_CHARS).collect())
            .collect();
        let mut inputs = Vec::with_capacity(texts.len() + 1);
        inputs.push(task.chars().take(MAX_EMBED_CHARS).collect());
        inputs.extend(texts.iter().cloned());
        let vectors = self.embed_cached(&inputs)?;
        let (query, corpus) = vectors
            .split_first()
            .ok_or_else(|| "embed returned no vectors".to_owned())?;
        let ids: Vec<&str> = fragments.iter().map(|item| item.id).collect();
        let related = evidence_adjacency(fragments);
        let ranking = rank_hybrid_graph(query, corpus, &texts, task, &ids, &related)
            .map_err(|error| error.to_string())?;
        Ok(scores_from_ranking(&ids, &ranking))
    }

    fn embed_cached(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let keys: Vec<String> = inputs.iter().map(|text| content_hash(text)).collect();
        let mut missing = Vec::new();
        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| "embedding cache lock poisoned".to_owned())?;
            for (key, text) in keys.iter().zip(inputs.iter()) {
                if !cache.contains_key(key) {
                    missing.push((key.clone(), text.clone()));
                }
            }
        }
        if !missing.is_empty() {
            let fresh = self
                .provider
                .embed(&EmbedRequest {
                    inputs: missing.iter().map(|(_, text)| text.clone()).collect(),
                })
                .map_err(|error| error.to_string())?
                .value;
            if fresh.len() != missing.len() {
                return Err("embed returned the wrong number of vectors".to_owned());
            }
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| "embedding cache lock poisoned".to_owned())?;
            for ((key, _), vector) in missing.into_iter().zip(fresh) {
                if cache.len() >= MAX_EMBED_CACHE
                    && let Some(oldest) = cache.keys().next().cloned()
                {
                    cache.remove(&oldest);
                }
                cache.insert(key, vector);
            }
        }
        let cache = self
            .cache
            .lock()
            .map_err(|_| "embedding cache lock poisoned".to_owned())?;
        keys.into_iter()
            .map(|key| {
                cache
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| "embedding cache miss after fill".to_owned())
            })
            .collect()
    }
}

fn content_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn resolve_ref(profiles_path: &Path, reference: &str) -> PathBuf {
    let raw = Path::new(reference);
    if raw.is_absolute() {
        return raw.to_path_buf();
    }
    profiles_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(raw)
}

fn load_registry(path: &Path) -> Result<ProfileRegistry, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("invalid {}: {error}", path.display()))
}

fn authorized_embedding(
    registry: &ProfileRegistry,
    wanted: Option<&str>,
    profiles_path: &Path,
) -> Result<cortex_llm::LlmProfile, String> {
    let mut last_error = "no embedding profile is configured".to_owned();
    for profile in registry.profiles().iter().filter(|profile| {
        profile.role == Role::Embedding
            && wanted.is_none_or(|want| profile.model == want || profile.id == want)
    }) {
        let Some(reference) = profile.calibration_ref.as_deref() else {
            last_error = format!("{} has no calibrationRef", profile.id);
            continue;
        };
        let artifact = resolve_ref(profiles_path, reference);
        match load_and_authorize(profile, &artifact) {
            Ok(()) => return Ok(profile.clone()),
            Err(error) => last_error = format!("{}: {error}", profile.id),
        }
    }
    Err(last_error)
}

fn load_and_authorize(profile: &cortex_llm::LlmProfile, path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let artifact: CalibrationArtifact = serde_json::from_str(&text)
        .map_err(|error| format!("invalid {}: {error}", path.display()))?;
    artifact
        .authorize(&attestation(profile))
        .map_err(|error| error.to_string())
}

fn attestation(profile: &cortex_llm::LlmProfile) -> RuntimeAttestation {
    let pooling = profile
        .embedding_pooling
        .clone()
        .unwrap_or_else(|| "none".to_owned());
    let digest = format!(
        "{}:{}:pooling={pooling}",
        runtime_tag(profile.runtime),
        profile.model
    );
    RuntimeAttestation {
        model: profile.model.clone(),
        model_digest: digest,
        runtime: profile.runtime,
        device: profile.device,
        quantization: profile.quantization.clone().unwrap_or_default(),
        embedding_pooling: pooling,
        tokenizer: profile.tokenizer.clone().unwrap_or_default(),
        prompt_version: "none".to_owned(),
        ranking_version: RANKING_VERSION.to_owned(),
        fixture_set_hash: RANKING_FIXTURE_SET.to_owned(),
        adjacency_kind: ADJACENCY_EVIDENCE_SPANS.to_owned(),
    }
}

fn runtime_tag(runtime: Runtime) -> &'static str {
    match runtime {
        Runtime::Ollama => "ollama",
        Runtime::OpenAiCompatible => "ovms",
    }
}

fn scores_from_ranking(ids: &[&str], ranking: &[usize]) -> HashMap<String, f64> {
    let mut scores = HashMap::with_capacity(ids.len());
    for (rank, index) in ranking.iter().enumerate() {
        if let Some(id) = ids.get(*index) {
            scores.insert(
                (*id).to_owned(),
                1.0 / (2.0 + f64::from(u32::try_from(rank).unwrap_or(u32::MAX))),
            );
        }
    }
    scores
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_context::ranking::parent_id;

    #[test]
    fn semantic_is_off_by_default() {
        let config = SemanticConfig::from_lookup(|_| None);
        assert!(!config.is_active());
        assert!(SemanticScorer::from_config(&config).unwrap().is_none());
    }

    #[test]
    fn a_flag_without_an_artifact_does_not_start_a_scorer() {
        let profiles = format!(
            "{}/../../config/llm-profiles.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let config = SemanticConfig::from_lookup(|key| match key {
            "CORTEX_SEMANTIC" => Some("1".to_owned()),
            "CORTEX_LLM_PROFILES" => Some(profiles.clone()),
            _ => None,
        });
        let error = match SemanticScorer::from_config(&config) {
            Ok(Some(_)) => panic!("gatePassed is not authority"),
            Ok(None) => panic!("enabled config must fail closed, not stay inactive"),
            Err(error) => error,
        };
        assert!(
            error.contains("calibration")
                || error.contains("verdict")
                || error.contains("adjacency"),
            "{error}"
        );
    }

    #[test]
    fn parent_ids_still_group_split_fragments() {
        assert_eq!(parent_id("WX-VERIFY-2"), "WX-VERIFY");
        assert_eq!(parent_id("WX-GRAPH"), "WX-GRAPH");
    }

    #[test]
    fn ranking_scores_are_monotone_and_bounded() {
        let ids = ["a", "b", "c"];
        let scores = scores_from_ranking(&ids, &[2, 0, 1]);
        assert!(scores["c"] > scores["a"]);
        assert!(scores.values().all(|score| (0.0..1.0).contains(score)));
    }
}
