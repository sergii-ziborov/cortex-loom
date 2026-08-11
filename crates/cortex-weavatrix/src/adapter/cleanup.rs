use super::evidence::{EvidenceFragment, EvidenceKind};

/// Remove clipped copies of a named definition once one complete source read
/// is available. Repeating a four-field prefix after the six-field source of
/// truth caused the local model to implement the later, incomplete shape.
pub(super) fn prune_incomplete_definition_duplicates(
    evidence: &mut Vec<EvidenceFragment>,
    symbol: &str,
) {
    let has_complete = evidence.iter().any(|fragment| {
        fragment.kind == EvidenceKind::SourceReads
            && crate::source_followup::definition_is_complete(&fragment.content, symbol)
                == Some(true)
    });
    if !has_complete {
        return;
    }
    evidence.retain(|fragment| {
        fragment.kind != EvidenceKind::SourceReads
            || crate::source_followup::definition_is_complete(&fragment.content, symbol)
                != Some(false)
    });
}

#[cfg(test)]
mod tests {
    use super::prune_incomplete_definition_duplicates;
    use crate::{EvidenceFragment, EvidenceKind};

    fn fragment(id: &str, content: &str) -> EvidenceFragment {
        EvidenceFragment {
            id: id.to_owned(),
            kind: EvidenceKind::SourceReads,
            source: "weavatrix:read_source".to_owned(),
            content: content.to_owned(),
            head: true,
        }
    }

    #[test]
    fn a_complete_definition_removes_only_its_truncated_duplicates() {
        let mut evidence = vec![
            fragment(
                "WX-DEF",
                "pub struct ArchiveOptions {\n    enabled: bool,\n    max_entries: usize,\n}",
            ),
            fragment(
                "WX-SOURCE-1",
                "pub struct ArchiveOptions {\n    enabled: bool,",
            ),
            fragment("WX-SOURCE-2", "fn uses_archive_options() {}"),
        ];

        prune_incomplete_definition_duplicates(&mut evidence, "ArchiveOptions");

        let ids: Vec<&str> = evidence
            .iter()
            .map(|fragment| fragment.id.as_str())
            .collect();
        assert_eq!(ids, ["WX-DEF", "WX-SOURCE-2"]);
    }
}
