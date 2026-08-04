//! Opt-in semantic ordering for evidence compilation.
//!
//! Off by default; enabled only by `CORTEX_SEMANTIC=1` plus an exact model
//! tag that passed the retrieval gate for the `hybrid_graph` mode. Scores
//! only reorder fragments within a priority band inside the deterministic
//! compiler; on any failure the packet falls back to deterministic order
//! with a recorded warning. Semantic ordering never gains workflow
//! authority.

use std::collections::HashMap;
use std::time::Duration;

use cortex_context::ranking::{
    Bm25Index, RANKING_VERSION, build_adjacency, graph_boost, rank_by_similarity, rrf_fuse,
};
use cortex_ollama::{EmbedRequest, MAX_EMBED_INPUTS, ModelProfile, OllamaClient, OllamaConfig};

const SEMANTIC_PROFILE: &str = "semantic";
const MAX_EMBED_CHARS: usize = 6_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticConfig {
    pub enabled: bool,
    /// Exact tag; must hold a passing `hybrid_graph` retrieval verdict.
    pub model: Option<String>,
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
            timeout_ms: non_empty("CORTEX_SEMANTIC_TIMEOUT_MS")
                .and_then(|value| value.parse().ok())
                .unwrap_or(30_000),
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && self.model.is_some()
    }
}

pub struct SemanticScorer {
    client: OllamaClient,
    model: String,
}

impl SemanticScorer {
    /// Build the scorer when explicitly configured; `Ok(None)` when inactive.
    pub fn from_config(config: SemanticConfig) -> Result<Option<Self>, String> {
        if !config.is_active() {
            return Ok(None);
        }
        let model = config.model.unwrap_or_default();
        let timeout = Duration::from_millis(config.timeout_ms.max(1));
        let ollama = OllamaConfig {
            request_timeout: timeout,
            read_timeout: timeout,
            write_timeout: timeout,
            ..OllamaConfig::default()
        }
        .with_profile(
            SEMANTIC_PROFILE,
            ModelProfile::new(model.clone(), 2_048, 1, 4_096),
        );
        let client = OllamaClient::new(ollama).map_err(|error| error.to_string())?;
        Ok(Some(Self { client, model }))
    }

    /// Provenance string recorded on packets whose ordering this influenced.
    #[must_use]
    pub fn provenance(&self) -> String {
        format!("{}/hybrid_graph/{RANKING_VERSION}", self.model)
    }

    /// Score fragments for one task with the gated `hybrid_graph` ranking.
    /// Scores are in (0, 1); the caller assigns the synthetic TASK item 1.0.
    pub fn score(
        &self,
        task: &str,
        fragments: &[(String, String)],
    ) -> Result<HashMap<String, f64>, String> {
        if fragments.is_empty() {
            return Ok(HashMap::new());
        }
        if fragments.len() + 1 > MAX_EMBED_INPUTS {
            return Err(format!(
                "{} fragments exceed the embed batch bound",
                fragments.len()
            ));
        }
        let truncated: Vec<String> = fragments
            .iter()
            .map(|(_, content)| content.chars().take(MAX_EMBED_CHARS).collect())
            .collect();
        let mut inputs = Vec::with_capacity(truncated.len() + 1);
        inputs.push(task.chars().take(MAX_EMBED_CHARS).collect::<String>());
        inputs.extend(truncated.iter().cloned());
        let vectors = self
            .client
            .embed(&EmbedRequest {
                profile: SEMANTIC_PROFILE.to_owned(),
                inputs,
            })
            .map_err(|error| error.to_string())?;
        let (query, corpus) = vectors
            .split_first()
            .ok_or_else(|| "embed returned no vectors".to_owned())?;

        let embedding_ranking = rank_by_similarity(query, corpus);
        let bm25 = Bm25Index::build(&truncated);
        let lexical_ranking = bm25.rank(task);
        let fused = rrf_fuse(
            &[embedding_ranking.as_slice(), lexical_ranking.as_slice()],
            fragments.len(),
        );
        let ids: Vec<&str> = fragments.iter().map(|(id, _)| id.as_str()).collect();
        let adjacency = build_adjacency(&ids, &fragment_adjacency(&ids));
        let boosted = graph_boost(&fused, &adjacency);
        Ok(scores_from_ranking(&ids, &boosted))
    }
}

/// Structural adjacency between fragments: split parts of the same tool
/// result (shared parent id such as `WX-VERIFY-1`/`WX-VERIFY-2`) are related.
pub(crate) fn fragment_adjacency(ids: &[&str]) -> Vec<Vec<String>> {
    let mut pairs = Vec::new();
    for (left_index, left) in ids.iter().enumerate() {
        for right in ids.iter().skip(left_index + 1) {
            if parent_id(left) == parent_id(right) {
                pairs.push(vec![(*left).to_owned(), (*right).to_owned()]);
            }
        }
    }
    pairs
}

/// `WX-VERIFY-2` → `WX-VERIFY`; ids without a numeric suffix are their own
/// parent.
pub(crate) fn parent_id(id: &str) -> &str {
    if let Some(position) = id.rfind('-') {
        let suffix = &id[position + 1..];
        if !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return &id[..position];
        }
    }
    id
}

/// Monotone map from rank to a score in (0, 1): rank 0 → 0.5.
pub(crate) fn scores_from_ranking(ids: &[&str], ranking: &[usize]) -> HashMap<String, f64> {
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

    #[test]
    fn parent_ids_group_split_fragments_only() {
        assert_eq!(parent_id("WX-VERIFY-2"), "WX-VERIFY");
        assert_eq!(parent_id("WX-VERIFY-12"), "WX-VERIFY");
        assert_eq!(parent_id("WX-GRAPH"), "WX-GRAPH");
        assert_eq!(parent_id("TASK"), "TASK");

        let ids = ["WX-VERIFY-1", "WX-VERIFY-2", "WX-GRAPH", "WX-MODULES"];
        let pairs = fragment_adjacency(&ids);
        assert_eq!(
            pairs,
            vec![vec!["WX-VERIFY-1".to_owned(), "WX-VERIFY-2".to_owned()]]
        );
    }

    #[test]
    fn ranking_scores_are_monotone_and_bounded() {
        let ids = ["a", "b", "c"];
        let scores = scores_from_ranking(&ids, &[2, 0, 1]);
        assert!(scores["c"] > scores["a"]);
        assert!(scores["a"] > scores["b"]);
        assert!(scores.values().all(|score| (0.0..1.0).contains(score)));
    }

    #[test]
    fn semantic_is_off_by_default() {
        let config = SemanticConfig::from_lookup(|_| None);
        assert!(!config.is_active());
        assert!(SemanticScorer::from_config(config).unwrap().is_none());

        let enabled_only =
            SemanticConfig::from_lookup(|key| (key == "CORTEX_SEMANTIC").then(|| "1".to_owned()));
        assert!(!enabled_only.is_active(), "a model tag is also required");
    }
}
