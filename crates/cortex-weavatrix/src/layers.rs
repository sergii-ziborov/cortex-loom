//! L0 decision map and L2 expansion handles for a compiled packet.

use cortex_context::{
    CoverageCertificate, EvidenceDerivation, EvidenceFacet, EvidenceItem, EvidencePriority,
    EvidenceState, render_decision_map, render_expansions,
};

use crate::plan::extract_identifiers;
use crate::plan_intent::detect;

#[must_use]
pub fn decision_map_item(
    repository: &str,
    task: &str,
    snapshot_id: Option<&str>,
    certificate: &CoverageCertificate,
) -> EvidenceItem {
    let intent = detect(task).as_str();
    let targets = extract_identifiers(task);
    let mut item = EvidenceItem::new(
        "WX-MAP",
        format!("cortex:decision_map:{repository}"),
        render_decision_map(intent, &targets, snapshot_id, risk_label(task), certificate),
        EvidencePriority::Critical,
        EvidenceState::Verified,
    );
    item.derivation = Some(EvidenceDerivation::Graph);
    item.facet = Some(EvidenceFacet::Structure);
    item
}

#[must_use]
pub fn expansion_item(certificate: &CoverageCertificate) -> Option<EvidenceItem> {
    if certificate.missing.is_empty() {
        return None;
    }
    let mut item = EvidenceItem::new(
        "WX-EXPAND",
        "cortex:expansions",
        render_expansions(certificate),
        EvidencePriority::Low,
        EvidenceState::Verified,
    );
    item.derivation = Some(EvidenceDerivation::Inferred);
    item.facet = Some(EvidenceFacet::Structure);
    Some(item)
}

fn risk_label(task: &str) -> &'static str {
    let lower = task.to_ascii_lowercase();
    if lower.contains("rename")
        || lower.contains("change")
        || lower.contains("implement")
        || lower.contains("add ")
        || lower.contains("delete")
    {
        "mutation"
    } else {
        "read"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_context::FACET_CALLERS;

    #[test]
    fn missing_callers_become_an_expand_handle() {
        let certificate = CoverageCertificate {
            required: vec![FACET_CALLERS.to_owned()],
            missing: vec![FACET_CALLERS.to_owned()],
            sufficient: false,
            ..CoverageCertificate::default()
        };
        let map = decision_map_item(
            "repo",
            "Rename ArchiveOptions",
            Some("git:1+dirty:0"),
            &certificate,
        );
        assert!(map.content.contains("EXPAND callers"));
        let expands = expansion_item(&certificate).expect("handles");
        assert!(expands.content.contains("EXPAND callers — direct_callers"));
    }
}
