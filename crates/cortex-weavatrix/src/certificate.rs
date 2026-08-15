//! Build a [`CoverageCertificate`] from gathered or compiled fragments.

use std::collections::BTreeMap;

use cortex_context::{
    CoverageCertificate, FACET_CALLERS, FACET_CONFIG, FACET_DEFAULTS, FACET_DEFINITION,
    FACET_ERRORS, FACET_GIT, FACET_GUARDS, FACET_MEMORY, FACET_PUBLIC_API, FACET_SIGNATURES,
    FACET_TESTS,
};

use crate::plan_intent::TaskIntent;
use crate::{EvidenceFragment, EvidenceKind, PlanHints};

pub(crate) struct TrackedFragment<'a> {
    pub id: &'a str,
    pub kind: EvidenceKind,
    pub facet: cortex_context::EvidenceFacet,
    pub content: &'a str,
    pub declared_complete: Option<bool>,
}

pub(crate) fn required_facets(
    task: &str,
    symbol: Option<&str>,
    hints: PlanHints,
    source_followup: bool,
) -> Vec<String> {
    let intent = hints.intent_or_detect(task);
    let mut required = Vec::new();
    match intent {
        TaskIntent::BlastRadius if symbol.is_some() => required.push(FACET_CALLERS.to_owned()),
        TaskIntent::ApiContract => required.push(FACET_PUBLIC_API.to_owned()),
        TaskIntent::GitHistory => required.push(FACET_GIT.to_owned()),
        TaskIntent::StackTrace => required.push(FACET_ERRORS.to_owned()),
        TaskIntent::TestSelection => required.push(FACET_TESTS.to_owned()),
        TaskIntent::RuntimeConfig => required.push(FACET_CONFIG.to_owned()),
        TaskIntent::IdentifierChange
        | TaskIntent::ModuleTopology
        | TaskIntent::BlastRadius
        | TaskIntent::PriorAttempt => {}
    }
    if hints.has_prior_attempts {
        required.push(FACET_MEMORY.to_owned());
    }
    if source_followup && symbol.is_some() {
        required.push(FACET_DEFINITION.to_owned());
    }
    if mentions_defaults(task) {
        required.push(FACET_DEFAULTS.to_owned());
    }
    if mentions_guards(task) {
        required.push(FACET_GUARDS.to_owned());
    }
    required.sort();
    required.dedup();
    required
}

pub(crate) fn certificate_from(
    fragments: &[TrackedFragment<'_>],
    required: Vec<String>,
    definition_complete: bool,
) -> CoverageCertificate {
    let mut satisfied: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for fragment in fragments {
        for facet in facets_closed_by(fragment, definition_complete) {
            let ids = satisfied.entry(facet).or_default();
            if !ids.iter().any(|id| id == fragment.id) {
                ids.push(fragment.id.to_owned());
            }
        }
    }
    let missing: Vec<String> = required
        .iter()
        .filter(|facet| !satisfied.contains_key(facet.as_str()))
        .cloned()
        .collect();
    CoverageCertificate {
        required,
        satisfied,
        sufficient: missing.is_empty(),
        missing,
        ..CoverageCertificate::default()
    }
}

pub(crate) fn tracked(fragment: &EvidenceFragment) -> TrackedFragment<'_> {
    TrackedFragment {
        id: fragment.id.as_str(),
        kind: fragment.kind,
        facet: fragment.facet,
        content: fragment.content.as_str(),
        declared_complete: fragment.declared_complete,
    }
}

fn facets_closed_by(fragment: &TrackedFragment<'_>, definition_complete: bool) -> Vec<String> {
    let mut closed = Vec::new();
    let lower = fragment.content.to_ascii_lowercase();
    if definition_complete
        && (fragment.facet == cortex_context::EvidenceFacet::Definition
            || fragment.declared_complete == Some(true)
            || matches!(
                fragment.kind,
                EvidenceKind::SourceReads | EvidenceKind::SymbolContext
            ))
    {
        closed.push(FACET_DEFINITION.to_owned());
    }
    if fragment.facet == cortex_context::EvidenceFacet::CallerSignature
        || fragment.kind == EvidenceKind::Dependents
    {
        closed.push(FACET_CALLERS.to_owned());
    }
    match fragment.kind {
        EvidenceKind::Endpoints => closed.push(FACET_PUBLIC_API.to_owned()),
        EvidenceKind::TestSelection => closed.push(FACET_TESTS.to_owned()),
        EvidenceKind::GitHistory => closed.push(FACET_GIT.to_owned()),
        EvidenceKind::Memory => closed.push(FACET_MEMORY.to_owned()),
        EvidenceKind::StackTrace => closed.push(FACET_ERRORS.to_owned()),
        EvidenceKind::SymbolContext => closed.push(FACET_SIGNATURES.to_owned()),
        EvidenceKind::SourceReads | EvidenceKind::TypeExpansion => {
            if lower.contains("default") || lower.contains("enabled:") {
                closed.push(FACET_DEFAULTS.to_owned());
            }
            if lower.contains("if ") || lower.contains("guard") || lower.contains("enabled") {
                closed.push(FACET_GUARDS.to_owned());
            }
            if lower.contains("cortex_") || lower.contains("config") {
                closed.push(FACET_CONFIG.to_owned());
            }
        }
        _ => {}
    }
    closed.sort();
    closed.dedup();
    closed
}

fn mentions_defaults(task: &str) -> bool {
    let lower = task.to_ascii_lowercase();
    lower.contains("default") || lower.contains("enabled")
}

fn mentions_guards(task: &str) -> bool {
    let lower = task.to_ascii_lowercase();
    lower.contains("guard") || lower.contains("silent") || lower.contains("skip")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EvidenceFragment, EvidenceKind};

    #[test]
    fn a_complete_definition_closes_the_target_facet() {
        let mut fragment = EvidenceFragment::new(
            "ev_def",
            EvidenceKind::SourceReads,
            "src/lib.rs:1",
            "pub struct ArchiveOptions { pub enabled: bool }",
        );
        fragment.facet = cortex_context::EvidenceFacet::Definition;
        fragment.declared_complete = Some(true);
        let tracked = [tracked(&fragment)];
        let certificate = certificate_from(&tracked, vec![FACET_DEFINITION.to_owned()], true);
        assert!(certificate.sufficient);
        assert_eq!(
            certificate.satisfied.get(FACET_DEFINITION),
            Some(&vec!["ev_def".to_owned()])
        );
    }
}
