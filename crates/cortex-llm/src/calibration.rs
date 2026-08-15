//! Calibration artifacts. `gatePassed` in a profile JSON is not authority.
//!
//! A passing verdict is bound to the exact serving identity that was
//! measured. Startup attests the live profile against this record; any
//! field mismatch disables the feature.

use serde::{Deserialize, Serialize};

use crate::device::Device;
use crate::profile::{Role, Runtime};

pub const CALIBRATION_SCHEMA: &str = "cortex-loom.calibration.v1";
/// Graph features the production scorer extracts from evidence fragments.
pub const ADJACENCY_EVIDENCE_SPANS: &str = "evidence_spans";
/// Graph features the historical retrieval gate used: declared fixture pairs.
pub const ADJACENCY_FIXTURE_RELATED: &str = "fixture_related";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationArtifact {
    pub schema_version: String,
    pub role: Role,
    pub profile_id: String,
    pub model: String,
    pub model_digest: String,
    pub runtime: Runtime,
    pub device: Device,
    pub quantization: String,
    pub embedding_pooling: String,
    pub tokenizer: String,
    pub prompt_version: String,
    pub ranking_version: String,
    pub fixture_set_hash: String,
    pub adjacency_kind: String,
    pub date: String,
    pub suite: String,
    /// Whether that exact identity passed. Never inferred from a profile
    /// `gatePassed` flag.
    pub verdict_pass: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAttestation {
    pub model: String,
    pub model_digest: String,
    pub runtime: Runtime,
    pub device: Device,
    pub quantization: String,
    pub embedding_pooling: String,
    pub tokenizer: String,
    pub prompt_version: String,
    pub ranking_version: String,
    pub fixture_set_hash: String,
    pub adjacency_kind: String,
}

/// Why a live profile cannot use an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttestationMismatch {
    Field {
        field: &'static str,
        expected: String,
        actual: String,
    },
    VerdictFailed,
    Schema(String),
}

impl std::fmt::Display for AttestationMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VerdictFailed => formatter.write_str("calibration verdict is not a pass"),
            Self::Schema(reason) => write!(formatter, "calibration schema: {reason}"),
            Self::Field {
                field,
                expected,
                actual,
            } => write!(formatter, "{field}: expected {expected}, live {actual}"),
        }
    }
}

impl CalibrationArtifact {
    /// # Errors
    ///
    /// Rejects an unknown schema or a pass with empty identity fields.
    pub fn validate(&self) -> Result<(), AttestationMismatch> {
        if self.schema_version != CALIBRATION_SCHEMA {
            return Err(AttestationMismatch::Schema(format!(
                "unsupported {}",
                self.schema_version
            )));
        }
        if self.verdict_pass
            && (self.model.is_empty()
                || self.ranking_version.is_empty()
                || self.fixture_set_hash.is_empty()
                || self.adjacency_kind.is_empty())
        {
            return Err(AttestationMismatch::Schema(
                "a passing artifact must name model, ranking, fixture, and adjacency".to_owned(),
            ));
        }
        Ok(())
    }

    /// # Errors
    ///
    /// Any identity field that differs, or a non-passing verdict.
    pub fn authorize(&self, live: &RuntimeAttestation) -> Result<(), AttestationMismatch> {
        self.validate()?;
        if !self.verdict_pass {
            return Err(AttestationMismatch::VerdictFailed);
        }
        check("model", &self.model, &live.model)?;
        check("modelDigest", &self.model_digest, &live.model_digest)?;
        if self.runtime != live.runtime {
            return Err(AttestationMismatch::Field {
                field: "runtime",
                expected: format!("{:?}", self.runtime),
                actual: format!("{:?}", live.runtime),
            });
        }
        if self.device != live.device {
            return Err(AttestationMismatch::Field {
                field: "device",
                expected: self.device.as_str().to_owned(),
                actual: live.device.as_str().to_owned(),
            });
        }
        check("quantization", &self.quantization, &live.quantization)?;
        check(
            "embeddingPooling",
            &self.embedding_pooling,
            &live.embedding_pooling,
        )?;
        check("tokenizer", &self.tokenizer, &live.tokenizer)?;
        check("promptVersion", &self.prompt_version, &live.prompt_version)?;
        check(
            "rankingVersion",
            &self.ranking_version,
            &live.ranking_version,
        )?;
        check(
            "fixtureSetHash",
            &self.fixture_set_hash,
            &live.fixture_set_hash,
        )?;
        check("adjacencyKind", &self.adjacency_kind, &live.adjacency_kind)?;
        Ok(())
    }
}

fn check(field: &'static str, expected: &str, actual: &str) -> Result<(), AttestationMismatch> {
    if expected == actual {
        Ok(())
    } else {
        Err(AttestationMismatch::Field {
            field,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact() -> CalibrationArtifact {
        CalibrationArtifact {
            schema_version: CALIBRATION_SCHEMA.to_owned(),
            role: Role::Embedding,
            profile_id: "gpu-embedding".to_owned(),
            model: "qwen3-embed".to_owned(),
            model_digest: "ovms:qwen3-embed:pooling=last".to_owned(),
            runtime: Runtime::OpenAiCompatible,
            device: Device::Gpu,
            quantization: "int8-ov".to_owned(),
            embedding_pooling: "last".to_owned(),
            tokenizer: "qwen3".to_owned(),
            prompt_version: "none".to_owned(),
            ranking_version: "retrieval-ranking-v1".to_owned(),
            fixture_set_hash: "retrieval-fixtures-v1".to_owned(),
            adjacency_kind: ADJACENCY_EVIDENCE_SPANS.to_owned(),
            date: "2026-08-05".to_owned(),
            suite: "hybrid_graph".to_owned(),
            verdict_pass: true,
        }
    }

    fn live() -> RuntimeAttestation {
        RuntimeAttestation {
            model: "qwen3-embed".to_owned(),
            model_digest: "ovms:qwen3-embed:pooling=last".to_owned(),
            runtime: Runtime::OpenAiCompatible,
            device: Device::Gpu,
            quantization: "int8-ov".to_owned(),
            embedding_pooling: "last".to_owned(),
            tokenizer: "qwen3".to_owned(),
            prompt_version: "none".to_owned(),
            ranking_version: "retrieval-ranking-v1".to_owned(),
            fixture_set_hash: "retrieval-fixtures-v1".to_owned(),
            adjacency_kind: ADJACENCY_EVIDENCE_SPANS.to_owned(),
        }
    }

    #[test]
    fn a_passing_artifact_authorizes_an_identical_attestation() {
        artifact().authorize(&live()).unwrap();
    }

    #[test]
    fn any_field_mismatch_disables_the_feature() {
        let mut live = live();
        live.embedding_pooling = "cls".to_owned();
        assert!(matches!(
            artifact().authorize(&live),
            Err(AttestationMismatch::Field {
                field: "embeddingPooling",
                ..
            })
        ));
    }

    #[test]
    fn a_failed_verdict_never_authorizes() {
        let mut artifact = artifact();
        artifact.verdict_pass = false;
        assert_eq!(
            artifact.authorize(&live()),
            Err(AttestationMismatch::VerdictFailed)
        );
    }
}
