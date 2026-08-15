//! Typed evidence identity, trust, and provenance.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidencePriority {
    Critical,
    High,
    Normal,
    Low,
}

impl EvidencePriority {
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Critical => 0,
            Self::High => 1,
            Self::Normal => 2,
            Self::Low => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Verified,
    Unverified,
    Contradictory,
}

/// How a fragment was derived. Shown in the packet heading so a model
/// cannot treat an unverified plan as exact source.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceDerivation {
    #[default]
    ExactSource,
    Plan,
    Graph,
    Search,
    Memory,
    Inferred,
}

/// Why this fragment is in the packet. Criticality attaches to the facet,
/// not to the Weavatrix operation that produced it.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceFacet {
    #[default]
    Unspecified,
    Definition,
    CallerSignature,
    SourceWindow,
    References,
    Plan,
    Structure,
    Memory,
}

/// Revision-stable locator. Empty fields mean the gatherer did not know.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLocator {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
}

impl EvidenceLocator {
    #[must_use]
    pub fn from_source(source: &str) -> Self {
        let mut locator = Self::default();
        let trimmed = source.trim();
        if let Some((path, rest)) = trimmed.rsplit_once(':')
            && let Ok(line) = rest.parse::<u32>()
            && (path.contains('/') || path.contains('\\') || path.contains('.'))
        {
            locator.path = Some(path.to_owned());
            locator.start_line = Some(line);
            locator.end_line = Some(line);
        }
        locator
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.path.is_none()
            && self.start_line.is_none()
            && self.end_line.is_none()
            && self.blob_hash.is_none()
            && self.snapshot_id.is_none()
    }

    /// True when both locators name the same span but a different blob or
    /// snapshot, so a packet citing `self` is stale against `current`.
    #[must_use]
    pub fn is_revision_stale(&self, current: &Self) -> bool {
        let same_span = self.path.is_some()
            && self.path == current.path
            && self.start_line == current.start_line
            && self.end_line == current.end_line;
        same_span
            && (option_mismatch(self.blob_hash.as_ref(), current.blob_hash.as_ref())
                || option_mismatch(self.snapshot_id.as_ref(), current.snapshot_id.as_ref()))
    }
}

fn option_mismatch(left: Option<&String>, right: Option<&String>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left != right)
}

/// `true` when the packet was compiled against a different tree than `current`.
/// A missing packet snapshot cannot prove staleness.
#[must_use]
pub fn snapshot_is_stale(packet_snapshot: Option<&str>, current: &str) -> bool {
    packet_snapshot.is_some_and(|id| id != current)
}

/// Content-addressed citation: `ev_<12 hex>`.
#[must_use]
pub fn evidence_id(parts: &[&str]) -> String {
    digest_id("ev_", parts, 12)
}

/// Packet handle: `pk_<12 hex>`.
#[must_use]
pub fn packet_id(parts: &[&str]) -> String {
    digest_id("pk_", parts, 12)
}

/// Span body hash: `blob_<12 hex>`.
#[must_use]
pub fn blob_id(parts: &[&str]) -> String {
    digest_id("blob_", parts, 12)
}

fn digest_id(prefix: &str, parts: &[&str], hex_chars: usize) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0xff]);
    }
    format!("{prefix}{:x}", hasher.finalize())
        .chars()
        .take(prefix.chars().count() + hex_chars)
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceItem {
    pub id: String,
    pub source: String,
    pub content: String,
    pub priority: EvidencePriority,
    pub state: EvidenceState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivation: Option<EvidenceDerivation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facet: Option<EvidenceFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contradiction_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<EvidenceLocator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
}

impl EvidenceItem {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        content: impl Into<String>,
        priority: EvidencePriority,
        state: EvidenceState,
    ) -> Self {
        let source = source.into();
        let locator = EvidenceLocator::from_source(&source);
        Self {
            id: id.into(),
            locator: (!locator.is_empty()).then_some(locator),
            source,
            content: content.into(),
            priority,
            state,
            relevance: None,
            derivation: None,
            facet: None,
            contradiction_group: None,
            group_id: None,
        }
    }

    #[must_use]
    pub fn heading_label(&self) -> String {
        match self.state {
            EvidenceState::Contradictory => self.contradiction_group.as_deref().map_or_else(
                || "CONTRADICTORY".to_owned(),
                |group| format!("CONTRADICTORY — group {group}"),
            ),
            EvidenceState::Unverified => match self.derivation {
                Some(EvidenceDerivation::Plan) => "UNVERIFIED PLAN".to_owned(),
                _ => "UNVERIFIED".to_owned(),
            },
            EvidenceState::Verified => match self.derivation {
                Some(EvidenceDerivation::ExactSource) => "EXACT SOURCE".to_owned(),
                Some(EvidenceDerivation::Graph) => "GRAPH".to_owned(),
                Some(EvidenceDerivation::Search) => "SEARCH".to_owned(),
                Some(EvidenceDerivation::Memory) => "MEMORY".to_owned(),
                Some(EvidenceDerivation::Plan) => "PLAN".to_owned(),
                Some(EvidenceDerivation::Inferred) | None => "VERIFIED".to_owned(),
            },
        }
    }
}
