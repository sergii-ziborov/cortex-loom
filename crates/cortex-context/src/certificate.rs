//! Structured completeness for a compiled packet.
//!
//! A boolean `sufficient` only says the compiler is happy. A certificate
//! names every required facet, the citations that close it, what is still
//! missing, and the snapshot that claim was checked against.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Facet names the packet can require or expand.
pub const FACET_DEFINITION: &str = "target.complete_definition";
pub const FACET_SIGNATURES: &str = "target.signatures";
pub const FACET_GUARDS: &str = "target.guards";
pub const FACET_DEFAULTS: &str = "target.defaults";
pub const FACET_CALLERS: &str = "direct_callers";
pub const FACET_PUBLIC_API: &str = "public_api_effect";
pub const FACET_TESTS: &str = "relevant_tests";
pub const FACET_CONFIG: &str = "runtime_config";
pub const FACET_ERRORS: &str = "exact_errors";
pub const FACET_MEMORY: &str = "prior_verified_failure";
pub const FACET_GIT: &str = "git_history";

/// Highest-risk missing facet is expanded first.
pub const FACET_EXPANSION_ORDER: &[&str] = &[
    FACET_DEFINITION,
    FACET_SIGNATURES,
    FACET_CALLERS,
    FACET_PUBLIC_API,
    FACET_DEFAULTS,
    FACET_GUARDS,
    FACET_CONFIG,
    FACET_ERRORS,
    FACET_TESTS,
    FACET_MEMORY,
    FACET_GIT,
];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoverageCertificate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    pub required: Vec<String>,
    pub satisfied: BTreeMap<String, Vec<String>>,
    pub missing: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contradictions: Vec<ContradictionGroup>,
    #[serde(default)]
    pub stale: bool,
    pub sufficient: bool,
    #[serde(default)]
    pub expansions_performed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContradictionGroup {
    pub id: String,
    pub evidence_ids: Vec<String>,
}

impl CoverageCertificate {
    #[must_use]
    pub fn critical_missing(&self) -> Vec<&str> {
        self.missing
            .iter()
            .filter(|facet| is_critical_facet(facet))
            .map(String::as_str)
            .collect()
    }

    /// Facets and citations the L0 map prints. Packet/snapshot ids are
    /// stamped later and must not count as a ledger change.
    #[must_use]
    pub fn ledger_matches(&self, other: &Self) -> bool {
        self.required == other.required
            && self.satisfied == other.satisfied
            && self.missing == other.missing
            && self.sufficient == other.sufficient
            && self.contradictions == other.contradictions
    }

    #[must_use]
    pub fn next_expansion(&self) -> Option<&str> {
        FACET_EXPANSION_ORDER
            .iter()
            .copied()
            .find(|facet| self.missing.iter().any(|missing| missing == facet))
    }

    #[must_use]
    pub fn expand_handle(facet: &str) -> &'static str {
        match facet {
            FACET_DEFINITION | FACET_SIGNATURES => "complete_definition",
            FACET_CALLERS => "callers",
            FACET_PUBLIC_API => "public_api_effect",
            FACET_TESTS => "tests",
            FACET_GIT => "git_history",
            _ => "source",
        }
    }
}

#[must_use]
pub fn is_critical_facet(facet: &str) -> bool {
    matches!(
        facet,
        FACET_DEFINITION | FACET_SIGNATURES | FACET_CALLERS | FACET_PUBLIC_API
    )
}

/// L0 decision map: intent, snapshot, and a facet ledger.
#[must_use]
pub fn render_decision_map(
    intent: &str,
    targets: &[String],
    snapshot_id: Option<&str>,
    risk: &str,
    certificate: &CoverageCertificate,
) -> String {
    let mut lines = vec![
        format!("intent: {intent}"),
        format!("targets: {}", display_list(targets)),
        format!("snapshot: {}", snapshot_id.unwrap_or("unknown")),
        format!("risk: {risk}"),
        "required:".to_owned(),
    ];
    for facet in &certificate.required {
        let rank = if is_critical_facet(facet) {
            "CRITICAL"
        } else {
            "HIGH"
        };
        if let Some(ids) = certificate.satisfied.get(facet) {
            lines.push(format!("- {facet} [{rank}] {}", ids.join(", ")));
        } else {
            lines.push(format!(
                "- {facet} [{rank}] MISSING → EXPAND {}",
                CoverageCertificate::expand_handle(facet)
            ));
        }
    }
    if certificate.contradictions.is_empty() {
        lines.push("contradictions: none".to_owned());
    } else {
        lines.push("contradictions:".to_owned());
        for group in &certificate.contradictions {
            lines.push(format!("- {} {}", group.id, group.evidence_ids.join(", ")));
        }
    }
    lines.join("\n")
}

/// L2 expansion handles for facets the packet did not close.
#[must_use]
pub fn render_expansions(certificate: &CoverageCertificate) -> String {
    let lines: Vec<String> = certificate
        .missing
        .iter()
        .map(|facet| {
            format!(
                "EXPAND {} — {facet}",
                CoverageCertificate::expand_handle(facet)
            )
        })
        .collect();
    if lines.is_empty() {
        "EXPAND none — every required facet is cited".to_owned()
    } else {
        lines.join("\n")
    }
}

fn display_list(values: &[String]) -> String {
    if values.is_empty() {
        "(none)".to_owned()
    } else {
        values.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_picks_the_highest_risk_missing_facet() {
        let certificate = CoverageCertificate {
            required: vec![FACET_TESTS.to_owned(), FACET_DEFINITION.to_owned()],
            missing: vec![FACET_TESTS.to_owned(), FACET_DEFINITION.to_owned()],
            sufficient: false,
            ..CoverageCertificate::default()
        };
        assert_eq!(certificate.next_expansion(), Some(FACET_DEFINITION));
        assert_eq!(certificate.critical_missing(), [FACET_DEFINITION]);
    }

    #[test]
    fn decision_map_names_ids_and_expand_handles() {
        let mut certificate = CoverageCertificate {
            required: vec![FACET_DEFINITION.to_owned(), FACET_CALLERS.to_owned()],
            sufficient: false,
            ..CoverageCertificate::default()
        };
        certificate
            .satisfied
            .insert(FACET_DEFINITION.to_owned(), vec!["ev_a91".to_owned()]);
        certificate.missing.push(FACET_CALLERS.to_owned());
        let map = render_decision_map(
            "identifier_change",
            &["ArchiveOptions".to_owned()],
            Some("git:abc+dirty:0"),
            "mutation",
            &certificate,
        );
        assert!(map.contains("intent: identifier_change"));
        assert!(map.contains("target.complete_definition [CRITICAL] ev_a91"));
        assert!(map.contains("direct_callers [CRITICAL] MISSING → EXPAND callers"));
        assert!(render_expansions(&certificate).contains("EXPAND callers — direct_callers"));
    }

    #[test]
    fn ledger_ignores_packet_identity() {
        let mut left = CoverageCertificate {
            required: vec![FACET_CALLERS.to_owned()],
            missing: vec![FACET_CALLERS.to_owned()],
            sufficient: false,
            packet_id: Some("a".to_owned()),
            ..CoverageCertificate::default()
        };
        let mut right = left.clone();
        right.packet_id = Some("b".to_owned());
        assert!(left.ledger_matches(&right));
        left.missing.clear();
        assert!(!left.ledger_matches(&right));
    }
}
