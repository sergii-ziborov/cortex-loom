//! Definition spans are scoped to one named target, even in multi-target Tasks.
use super::evidence::{EvidenceFragment, EvidenceKind};
use cortex_context::{EvidenceFacet, EvidenceLocator};

pub(super) fn belongs_to_definition(fragment: &EvidenceFragment, symbol: &str) -> bool {
    fragment.source.ends_with(&format!("definition:{symbol}"))
        || crate::source_followup::definition_head_index(&fragment.content, symbol).is_some()
}

pub(super) fn definition_span(
    evidence: &[EvidenceFragment],
    symbol: &str,
) -> Option<EvidenceLocator> {
    evidence.iter().find_map(|fragment| {
        let locator = &fragment.locator;
        let usable = locator.path.is_some()
            && locator.start_line.is_some()
            && locator.end_line.is_some()
            && belongs_to_definition(fragment, symbol)
            && (fragment.facet == EvidenceFacet::Definition
                || fragment.kind == EvidenceKind::SymbolContext);
        usable.then(|| locator.clone())
    })
}
