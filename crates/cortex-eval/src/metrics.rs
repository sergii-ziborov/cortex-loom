//! Per-sample records and deterministic aggregation.

use cortex_router::ModelTier;
use serde::Serialize;

use crate::comparators::{CitationMetrics, ClassificationOutcome};

#[must_use]
pub fn count_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

#[must_use]
pub fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        count_f64(numerator) / count_f64(denominator)
    }
}

/// Nearest-rank percentile over unsorted samples; zero when empty.
#[must_use]
pub fn percentile(samples: &[u64], percentile: u8) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = (sorted.len() * usize::from(percentile))
        .div_ceil(100)
        .max(1);
    sorted[rank - 1]
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LatencyStats {
    pub samples: u32,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub max_ms: u64,
}

#[must_use]
pub fn latency_stats(samples: &[u64]) -> LatencyStats {
    LatencyStats {
        samples: u32::try_from(samples.len()).unwrap_or(u32::MAX),
        p50_ms: percentile(samples, 50),
        p95_ms: percentile(samples, 95),
        max_ms: samples.iter().copied().max().unwrap_or(0),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationSample {
    pub fixture_id: String,
    pub gold_tier: ModelTier,
    pub observed_tier: Option<ModelTier>,
    pub schema_valid: bool,
    pub outcome: ClassificationOutcome,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionMatches {
    pub action: bool,
    pub files: bool,
    pub symbols: bool,
}

impl ExtractionMatches {
    #[must_use]
    pub const fn is_exact(self) -> bool {
        self.action && self.files && self.symbols
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionSample {
    pub fixture_id: String,
    pub schema_valid: bool,
    pub matches: ExtractionMatches,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompressionSample {
    pub fixture_id: String,
    pub schema_valid: bool,
    pub citations: Option<CitationMetrics>,
    pub token_delta: Option<i64>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationAggregate {
    pub samples: u32,
    pub schema_valid: u32,
    pub schema_valid_rate: f64,
    pub agreements: u32,
    pub accuracy: f64,
    pub under_called: u32,
    pub missed_escalations: u32,
}

#[must_use]
pub fn aggregate_classification(samples: &[ClassificationSample]) -> ClassificationAggregate {
    let schema_valid = samples.iter().filter(|s| s.schema_valid).count();
    let agreements = samples.iter().filter(|s| s.outcome.agreement).count();
    ClassificationAggregate {
        samples: u32::try_from(samples.len()).unwrap_or(u32::MAX),
        schema_valid: u32::try_from(schema_valid).unwrap_or(u32::MAX),
        schema_valid_rate: ratio(schema_valid, samples.len()),
        agreements: u32::try_from(agreements).unwrap_or(u32::MAX),
        accuracy: ratio(agreements, samples.len()),
        under_called: u32::try_from(samples.iter().filter(|s| s.outcome.under_called).count())
            .unwrap_or(u32::MAX),
        missed_escalations: u32::try_from(
            samples
                .iter()
                .filter(|s| s.outcome.missed_escalation)
                .count(),
        )
        .unwrap_or(u32::MAX),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionAggregate {
    pub samples: u32,
    pub schema_valid: u32,
    pub schema_valid_rate: f64,
    pub action_matches: u32,
    pub action_accuracy: f64,
    pub exact_matches: u32,
    pub exact_match_rate: f64,
}

#[must_use]
pub fn aggregate_extraction(samples: &[ExtractionSample]) -> ExtractionAggregate {
    let schema_valid = samples.iter().filter(|s| s.schema_valid).count();
    let action_matches = samples.iter().filter(|s| s.matches.action).count();
    let exact_matches = samples.iter().filter(|s| s.matches.is_exact()).count();
    ExtractionAggregate {
        samples: u32::try_from(samples.len()).unwrap_or(u32::MAX),
        schema_valid: u32::try_from(schema_valid).unwrap_or(u32::MAX),
        schema_valid_rate: ratio(schema_valid, samples.len()),
        action_matches: u32::try_from(action_matches).unwrap_or(u32::MAX),
        action_accuracy: ratio(action_matches, samples.len()),
        exact_matches: u32::try_from(exact_matches).unwrap_or(u32::MAX),
        exact_match_rate: ratio(exact_matches, samples.len()),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompressionAggregate {
    pub samples: u32,
    pub schema_valid: u32,
    pub schema_valid_rate: f64,
    pub mean_preserved_ratio: f64,
    pub min_preserved_ratio: f64,
    pub hallucinated_total: u32,
    pub missing_total: u32,
    pub compressed_count: u32,
    pub mean_token_delta: i64,
}

#[must_use]
pub fn aggregate_compression(samples: &[CompressionSample]) -> CompressionAggregate {
    let valid: Vec<&CompressionSample> = samples.iter().filter(|s| s.schema_valid).collect();
    let ratios: Vec<f64> = valid
        .iter()
        .filter_map(|s| s.citations.as_ref().map(|c| c.preserved_ratio))
        .collect();
    let deltas: Vec<i64> = valid.iter().filter_map(|s| s.token_delta).collect();
    let hallucinated_total = valid
        .iter()
        .filter_map(|s| s.citations.as_ref())
        .map(|c| c.hallucinated.len())
        .sum::<usize>();
    let missing_total = valid
        .iter()
        .filter_map(|s| s.citations.as_ref())
        .map(|c| c.missing.len())
        .sum::<usize>();
    CompressionAggregate {
        samples: u32::try_from(samples.len()).unwrap_or(u32::MAX),
        schema_valid: u32::try_from(valid.len()).unwrap_or(u32::MAX),
        schema_valid_rate: ratio(valid.len(), samples.len()),
        mean_preserved_ratio: if ratios.is_empty() {
            0.0
        } else {
            ratios.iter().sum::<f64>() / count_f64(ratios.len())
        },
        min_preserved_ratio: if ratios.is_empty() {
            0.0
        } else {
            ratios.iter().copied().fold(f64::INFINITY, f64::min)
        },
        hallucinated_total: u32::try_from(hallucinated_total).unwrap_or(u32::MAX),
        missing_total: u32::try_from(missing_total).unwrap_or(u32::MAX),
        compressed_count: u32::try_from(deltas.iter().filter(|delta| **delta < 0).count())
            .unwrap_or(u32::MAX),
        mean_token_delta: if deltas.is_empty() {
            0
        } else {
            deltas.iter().sum::<i64>() / i64::try_from(deltas.len()).unwrap_or(1).max(1)
        },
    }
}
