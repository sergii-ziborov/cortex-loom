//! Fail-closed, role-aware calibration verdicts.
//!
//! A verdict is measurement data for humans and for shadow-mode configuration
//! gates. It never changes routing by itself. A profile is calibrated for the
//! role its tier grants: `local_small` is gated on classification and
//! extraction, `local_medium` on citation-preserving compression. Suites
//! outside the role are still measured and reported, but they do not gate the
//! verdict, because routing never assigns that work to the profile.

use cortex_router::ModelTier;
use serde::Serialize;

use crate::SuiteKind;
use crate::metrics::{ClassificationAggregate, CompressionAggregate, ExtractionAggregate};

pub const MIN_SCHEMA_VALID_RATE: f64 = 0.95;
pub const MIN_CLASSIFICATION_ACCURACY: f64 = 0.8;
pub const MIN_EXTRACTION_ACTION_ACCURACY: f64 = 0.8;
pub const MIN_EXTRACTION_EXACT_RATE: f64 = 0.6;
pub const MIN_PRESERVED_RATIO: f64 = 0.9;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "code", content = "detail")]
pub enum VerdictReason {
    SuiteNotRun(SuiteKind),
    SchemaValidityBelowThreshold { suite: SuiteKind, rate: f64 },
    MissedEscalations { count: u32 },
    ClassificationAccuracyBelowThreshold { accuracy: f64 },
    ActionAccuracyBelowThreshold { accuracy: f64 },
    ExactMatchBelowThreshold { rate: f64 },
    CitationPreservationBelowThreshold { min_ratio: f64 },
    HallucinatedCitations { count: u32 },
    DraftDoesNotCompress { mean_token_delta: i64 },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationVerdict {
    pub pass: bool,
    pub reasons: Vec<VerdictReason>,
}

/// Judge one profile for the role its tier grants. Every gated suite must
/// have run: a partial run of the role matrix fails explicitly. A tier that
/// maps to no local role (`none`, `upstream_strong`) is gated on everything.
#[must_use]
pub fn judge(
    tier: ModelTier,
    classification: Option<&ClassificationAggregate>,
    extraction: Option<&ExtractionAggregate>,
    compression: Option<&CompressionAggregate>,
) -> CalibrationVerdict {
    let gate_small_role = !matches!(tier, ModelTier::LocalMedium);
    let gate_medium_role = !matches!(tier, ModelTier::LocalSmall);
    let mut reasons = Vec::new();

    match (gate_small_role, classification) {
        (false, _) => {}
        (true, None) => reasons.push(VerdictReason::SuiteNotRun(SuiteKind::Classification)),
        (true, Some(aggregate)) => {
            if aggregate.schema_valid_rate < MIN_SCHEMA_VALID_RATE {
                reasons.push(VerdictReason::SchemaValidityBelowThreshold {
                    suite: SuiteKind::Classification,
                    rate: aggregate.schema_valid_rate,
                });
            }
            if aggregate.missed_escalations > 0 {
                reasons.push(VerdictReason::MissedEscalations {
                    count: aggregate.missed_escalations,
                });
            }
            if aggregate.accuracy < MIN_CLASSIFICATION_ACCURACY {
                reasons.push(VerdictReason::ClassificationAccuracyBelowThreshold {
                    accuracy: aggregate.accuracy,
                });
            }
        }
    }

    match (gate_small_role, extraction) {
        (false, _) => {}
        (true, None) => reasons.push(VerdictReason::SuiteNotRun(SuiteKind::Extraction)),
        (true, Some(aggregate)) => {
            if aggregate.schema_valid_rate < MIN_SCHEMA_VALID_RATE {
                reasons.push(VerdictReason::SchemaValidityBelowThreshold {
                    suite: SuiteKind::Extraction,
                    rate: aggregate.schema_valid_rate,
                });
            }
            if aggregate.action_accuracy < MIN_EXTRACTION_ACTION_ACCURACY {
                reasons.push(VerdictReason::ActionAccuracyBelowThreshold {
                    accuracy: aggregate.action_accuracy,
                });
            }
            if aggregate.exact_match_rate < MIN_EXTRACTION_EXACT_RATE {
                reasons.push(VerdictReason::ExactMatchBelowThreshold {
                    rate: aggregate.exact_match_rate,
                });
            }
        }
    }

    match (gate_medium_role, compression) {
        (false, _) => {}
        (true, None) => reasons.push(VerdictReason::SuiteNotRun(SuiteKind::Compression)),
        (true, Some(aggregate)) => {
            if aggregate.schema_valid_rate < MIN_SCHEMA_VALID_RATE {
                reasons.push(VerdictReason::SchemaValidityBelowThreshold {
                    suite: SuiteKind::Compression,
                    rate: aggregate.schema_valid_rate,
                });
            }
            if aggregate.min_preserved_ratio < MIN_PRESERVED_RATIO {
                reasons.push(VerdictReason::CitationPreservationBelowThreshold {
                    min_ratio: aggregate.min_preserved_ratio,
                });
            }
            if aggregate.hallucinated_total > 0 {
                reasons.push(VerdictReason::HallucinatedCitations {
                    count: aggregate.hallucinated_total,
                });
            }
            if aggregate.mean_token_delta >= 0 {
                reasons.push(VerdictReason::DraftDoesNotCompress {
                    mean_token_delta: aggregate.mean_token_delta,
                });
            }
        }
    }

    CalibrationVerdict {
        pass: reasons.is_empty(),
        reasons,
    }
}
