//! A named code edit needs the owning file, not only a short type span.

use cortex_context::EvidenceFacet;

use super::{SearchHit, definition_head_index};
use crate::{EvidenceFragment, plan::extract_identifiers};

pub(crate) fn coding_owner_hit(evidence: &[EvidenceFragment], task: &str) -> Option<SearchHit> {
    if !crate::plan_intent::is_coding_change(task) {
        return None;
    }
    let identifiers = extract_identifiers(task);
    let symbol = crate::fold::graph_seed(&identifiers)?;
    let fragment = evidence.iter().find(|fragment| {
        fragment.facet == EvidenceFacet::Definition
            && definition_head_index(&fragment.content, symbol).is_some()
            && fragment.locator.path.is_some()
    })?;
    let path = fragment.locator.path.as_ref()?;
    Some(SearchHit {
        path: path.replace('\\', "/"),
        line: 1,
        text: symbol.to_owned(),
    })
}

pub(crate) fn owner_budget(total: u32) -> u32 {
    (total / 4).clamp(200, 2_400)
}

#[cfg(test)]
mod tests {
    use cortex_context::EvidenceFacet;

    use super::*;
    use crate::{EvidenceFragment, EvidenceKind};

    #[test]
    fn named_code_edit_opens_the_owner_from_line_one() {
        let mut source = EvidenceFragment::new(
            "definition",
            EvidenceKind::SourceReads,
            "weavatrix:read_source",
            "pub(crate) struct PacketStore { inner: Mutex<HashMap<String, StoredPacket>> }",
        );
        source.facet = EvidenceFacet::Definition;
        source.locator.path = Some("crates/cortex-mcp/src/runtime/packet_store.rs".to_owned());
        let hit = coding_owner_hit(
            &[source],
            "Fix PacketStore eviction and add focused tests; preserve HashMap lookup",
        )
        .expect("the named owner");
        assert_eq!(hit.path, "crates/cortex-mcp/src/runtime/packet_store.rs");
        assert_eq!(hit.line, 1);
        assert_eq!(owner_budget(6_000), 1_500);
        assert_eq!(owner_budget(400), 200);
    }

    #[test]
    fn named_constant_edit_opens_the_owner_for_inline_tests() {
        let mut source = EvidenceFragment::new(
            "definition",
            EvidenceKind::SourceReads,
            "weavatrix:read_source",
            "const MAX_PACKETS: usize = 32;",
        );
        source.facet = EvidenceFacet::Definition;
        source.locator.path = Some("crates/cortex-mcp/src/runtime/packet_store.rs".to_owned());
        let hit = coding_owner_hit(
            &[source],
            "Rename MAX_PACKETS and update inline tests in packet_store.rs",
        )
        .expect("the named constant owner");
        assert_eq!(hit.path, "crates/cortex-mcp/src/runtime/packet_store.rs");
    }
}
