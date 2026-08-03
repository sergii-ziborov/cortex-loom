//! Pure comparison functions shared by the benchmark harness and shadow mode.

use cortex_context::estimate_tokens;
use cortex_router::{ModelTier, TaskClass};
use serde::Serialize;

use crate::metrics::ratio;

/// Map a deterministic task class to the tier the routing policy grants it.
#[must_use]
pub const fn policy_tier(class: TaskClass) -> ModelTier {
    match class {
        TaskClass::Deterministic | TaskClass::RepositoryAnalysis => ModelTier::None,
        TaskClass::StructuredExtraction => ModelTier::LocalSmall,
        TaskClass::ContextCompression | TaskClass::AdvisoryDraft => ModelTier::LocalMedium,
        TaskClass::Implementation
        | TaskClass::Security
        | TaskClass::Authentication
        | TaskClass::Concurrency
        | TaskClass::Migration
        | TaskClass::Release
        | TaskClass::Deployment
        | TaskClass::Publication
        | TaskClass::Ambiguous => ModelTier::UpstreamStrong,
    }
}

#[must_use]
pub const fn tier_rank(tier: ModelTier) -> u8 {
    match tier {
        ModelTier::None => 0,
        ModelTier::LocalSmall => 1,
        ModelTier::LocalMedium => 2,
        ModelTier::UpstreamStrong => 3,
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationOutcome {
    pub agreement: bool,
    pub under_called: bool,
    /// The safety metric: the model claimed a lower tier where the gold
    /// decision is upstream. Shadow promotion requires zero of these.
    pub missed_escalation: bool,
}

/// Compare an observed tier against the gold tier. `None` means the model
/// failed to produce a schema-valid tier at all, which is counted as a schema
/// failure rather than a missed escalation because fail-closed routing already
/// rejects it.
#[must_use]
pub fn classification_outcome(
    gold: ModelTier,
    observed: Option<ModelTier>,
) -> ClassificationOutcome {
    observed.map_or(
        ClassificationOutcome {
            agreement: false,
            under_called: false,
            missed_escalation: false,
        },
        |tier| {
            let under_called = tier_rank(tier) < tier_rank(gold);
            ClassificationOutcome {
                agreement: tier == gold,
                under_called,
                missed_escalation: under_called && matches!(gold, ModelTier::UpstreamStrong),
            }
        },
    )
}

/// True when the value is usable as a stable citation ID such as `WX-GRAPH`.
#[must_use]
pub fn is_citation_id(value: &str) -> bool {
    let mut chars = value.chars();
    let starts_with_letter = chars.next().is_some_and(|first| first.is_ascii_uppercase());
    starts_with_letter
        && (2..=32).contains(&value.len())
        && value
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '-')
}

/// Extract unique bracketed citation IDs, in order of first appearance.
#[must_use]
pub fn bracketed_ids(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut current: Option<String> = None;
    for ch in text.chars() {
        match ch {
            '[' => current = Some(String::new()),
            ']' => {
                if let Some(candidate) = current.take()
                    && is_citation_id(&candidate)
                    && !found.contains(&candidate)
                {
                    found.push(candidate);
                }
            }
            _ => {
                if let Some(candidate) = &mut current {
                    candidate.push(ch);
                }
            }
        }
    }
    found
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CitationMetrics {
    pub cited: Vec<String>,
    pub missing: Vec<String>,
    pub hallucinated: Vec<String>,
    pub preserved_ratio: f64,
}

/// Compare cited IDs (claimed list plus inline brackets) against the supplied
/// evidence set and the IDs a draft is required to preserve.
#[must_use]
pub fn citation_metrics(
    supplied: &[String],
    must_cite: &[String],
    summary: &str,
    claimed: &[String],
) -> CitationMetrics {
    let mut cited = bracketed_ids(summary);
    for id in claimed {
        if !cited.contains(id) {
            cited.push(id.clone());
        }
    }
    let missing: Vec<String> = must_cite
        .iter()
        .filter(|id| !cited.contains(id))
        .cloned()
        .collect();
    let hallucinated: Vec<String> = cited
        .iter()
        .filter(|id| !supplied.contains(id))
        .cloned()
        .collect();
    let preserved_ratio = if must_cite.is_empty() {
        1.0
    } else {
        ratio(must_cite.len() - missing.len(), must_cite.len())
    };
    CitationMetrics {
        cited,
        missing,
        hallucinated,
        preserved_ratio,
    }
}

/// Estimated token change from raw evidence to a draft; negative means the
/// draft actually compresses.
#[must_use]
pub fn token_delta(evidence_contents: &[&str], summary: &str) -> i64 {
    let raw = evidence_contents
        .iter()
        .map(|content| estimate_tokens(content))
        .fold(0_u32, u32::saturating_add);
    i64::from(estimate_tokens(summary)) - i64::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_rank_strictly_and_policy_covers_every_class() {
        assert!(tier_rank(ModelTier::None) < tier_rank(ModelTier::LocalSmall));
        assert!(tier_rank(ModelTier::LocalSmall) < tier_rank(ModelTier::LocalMedium));
        assert!(tier_rank(ModelTier::LocalMedium) < tier_rank(ModelTier::UpstreamStrong));
        assert_eq!(policy_tier(TaskClass::Security), ModelTier::UpstreamStrong);
        assert_eq!(policy_tier(TaskClass::Deterministic), ModelTier::None);
    }

    #[test]
    fn missed_escalation_requires_an_upstream_gold_decision() {
        let missed = classification_outcome(ModelTier::UpstreamStrong, Some(ModelTier::LocalSmall));
        assert!(missed.missed_escalation && missed.under_called && !missed.agreement);

        let under_only =
            classification_outcome(ModelTier::LocalMedium, Some(ModelTier::LocalSmall));
        assert!(under_only.under_called && !under_only.missed_escalation);

        let schema_failure = classification_outcome(ModelTier::UpstreamStrong, None);
        assert!(!schema_failure.agreement && !schema_failure.missed_escalation);

        let over_called =
            classification_outcome(ModelTier::LocalSmall, Some(ModelTier::UpstreamStrong));
        assert!(!over_called.under_called && !over_called.missed_escalation);
    }

    #[test]
    fn bracketed_ids_are_validated_and_deduplicated() {
        let text = "Uses [WX-GRAPH] and [WX-GRAPH] plus [lowercase], [X], [WX-1] and [not an id].";
        assert_eq!(bracketed_ids(text), ["WX-GRAPH", "WX-1"]);
    }

    #[test]
    fn citation_metrics_report_missing_and_hallucinated_ids() {
        let supplied = vec!["WX-GRAPH".to_owned(), "WX-MODULES".to_owned()];
        let must_cite = supplied.clone();
        let metrics = citation_metrics(
            &supplied,
            &must_cite,
            "Grounded in [WX-GRAPH] and [WX-FAKE].",
            &["WX-MODULES".to_owned()],
        );
        assert_eq!(metrics.missing, Vec::<String>::new());
        assert_eq!(metrics.hallucinated, ["WX-FAKE"]);
        assert!((metrics.preserved_ratio - 1.0).abs() < 1e-9);

        let partial = citation_metrics(&supplied, &must_cite, "Only [WX-GRAPH].", &[]);
        assert_eq!(partial.missing, ["WX-MODULES"]);
        assert!((partial.preserved_ratio - 0.5).abs() < 1e-9);
    }

    #[test]
    fn token_delta_is_negative_for_a_real_compression() {
        let evidence = "long evidence ".repeat(50);
        assert!(token_delta(&[evidence.as_str()], "short summary") < 0);
        assert!(token_delta(&["tiny"], &"expanded ".repeat(80)) > 0);
    }
}
