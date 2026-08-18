//! Build a [`CoverageCertificate`] from gathered or compiled fragments.

use std::collections::BTreeMap;

use cortex_context::{
    CoverageCertificate, FACET_CALLERS, FACET_CONFIG, FACET_DEFAULTS, FACET_DEFINITION,
    FACET_ERRORS, FACET_GIT, FACET_GUARDS, FACET_MEMORY, FACET_PUBLIC_API, FACET_SIGNATURES,
    FACET_TESTS, FacetClaim,
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
    symbol: Option<&str>,
) -> CoverageCertificate {
    let mut satisfied: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut claims: BTreeMap<String, FacetClaim> = BTreeMap::new();
    for fragment in fragments {
        for close in facets_closed_by(fragment, definition_complete, symbol) {
            let ids = satisfied.entry(close.facet.clone()).or_default();
            if !ids.iter().any(|id| id == fragment.id) {
                ids.push(fragment.id.to_owned());
            }
            let claim = claims
                .entry(close.facet.clone())
                .or_insert_with(|| FacetClaim {
                    facet: close.facet,
                    target: symbol
                        .filter(|name| !name.is_empty())
                        .map(ToOwned::to_owned),
                    evidence_ids: Vec::new(),
                    validator: close.validator.to_owned(),
                    cardinality: 0,
                });
            if !claim.evidence_ids.iter().any(|id| id == fragment.id) {
                claim.evidence_ids.push(fragment.id.to_owned());
                claim.cardinality = claim.cardinality.saturating_add(1);
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
        claims: claims.into_values().collect(),
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

struct Close {
    facet: String,
    validator: &'static str,
}

fn facets_closed_by(
    fragment: &TrackedFragment<'_>,
    definition_complete: bool,
    symbol: Option<&str>,
) -> Vec<Close> {
    let mut closed = Vec::new();
    let lower = fragment.content.to_ascii_lowercase();
    if definition_complete
        && names_target(fragment.content, symbol)
        && (fragment.facet == cortex_context::EvidenceFacet::Definition
            || fragment.declared_complete == Some(true)
            || matches!(
                fragment.kind,
                EvidenceKind::SourceReads | EvidenceKind::SymbolContext
            ))
    {
        closed.push(Close {
            facet: FACET_DEFINITION.to_owned(),
            validator: "definition_span/v1",
        });
    }
    if (fragment.facet == cortex_context::EvidenceFacet::CallerSignature
        || fragment.kind == EvidenceKind::Dependents)
        && has_caller_payload(fragment.content)
        && names_target(fragment.content, symbol)
    {
        closed.push(Close {
            facet: FACET_CALLERS.to_owned(),
            validator: "graph_dependents/v1",
        });
    }
    match fragment.kind {
        EvidenceKind::Endpoints if has_endpoint_payload(fragment.content) => {
            closed.push(Close {
                facet: FACET_PUBLIC_API.to_owned(),
                validator: "endpoints/v1",
            });
        }
        EvidenceKind::TestSelection if has_test_payload(fragment.content) => {
            closed.push(Close {
                facet: FACET_TESTS.to_owned(),
                validator: "select_tests/v1",
            });
        }
        EvidenceKind::GitHistory if has_commit_payload(fragment.content) => {
            closed.push(Close {
                facet: FACET_GIT.to_owned(),
                validator: "git_history/v1",
            });
        }
        EvidenceKind::Memory if !fragment.content.trim().is_empty() => {
            closed.push(Close {
                facet: FACET_MEMORY.to_owned(),
                validator: "memory_context/v1",
            });
        }
        EvidenceKind::StackTrace if has_stack_payload(fragment.content) => {
            closed.push(Close {
                facet: FACET_ERRORS.to_owned(),
                validator: "map_stacktrace/v1",
            });
        }
        EvidenceKind::SymbolContext => {
            closed.push(Close {
                facet: FACET_SIGNATURES.to_owned(),
                validator: "symbol_context/v1",
            });
        }
        EvidenceKind::SourceReads | EvidenceKind::TypeExpansion => {
            if lower.contains("default") || lower.contains("enabled:") {
                closed.push(Close {
                    facet: FACET_DEFAULTS.to_owned(),
                    validator: "source_terms/v1",
                });
            }
            if lower.contains("if ") || lower.contains("guard") || lower.contains("enabled") {
                closed.push(Close {
                    facet: FACET_GUARDS.to_owned(),
                    validator: "source_terms/v1",
                });
            }
            if lower.contains("cortex_") || lower.contains("config") {
                closed.push(Close {
                    facet: FACET_CONFIG.to_owned(),
                    validator: "source_terms/v1",
                });
            }
        }
        _ => {}
    }
    closed
}

fn names_target(content: &str, symbol: Option<&str>) -> bool {
    let Some(symbol) = symbol.filter(|name| !name.is_empty()) else {
        return true;
    };
    content
        .to_ascii_lowercase()
        .contains(&symbol.to_ascii_lowercase())
}

fn has_caller_payload(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    if lower.contains("relationships: 0") || lower.contains("0 dependents") {
        return false;
    }
    content.contains("->")
        || content.contains("<-")
        || lower.contains(" calls ")
        || lower.contains("called")
}

fn has_endpoint_payload(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    (content.contains('/') || lower.contains("get ") || lower.contains("post "))
        && !lower.contains("0 endpoint")
}

fn has_test_payload(content: &str) -> bool {
    let lower = content.replace('\\', "/").to_ascii_lowercase();
    (lower.contains("tests.rs")
        || lower.contains("/tests/")
        || lower.contains(".test.")
        || lower.contains("#[test]"))
        && !lower.contains("\"tests\":[]")
}

fn has_commit_payload(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    if lower.contains("commits: 0") {
        return false;
    }
    lower.contains("commits: ") || content.split_whitespace().any(|token| token.len() == 12)
}

fn has_stack_payload(content: &str) -> bool {
    content.contains(".rs:") || content.contains(".rs")
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
        let certificate = certificate_from(
            &tracked,
            vec![FACET_DEFINITION.to_owned()],
            true,
            Some("ArchiveOptions"),
        );
        assert!(certificate.sufficient);
        assert_eq!(
            certificate.satisfied.get(FACET_DEFINITION),
            Some(&vec!["ev_def".to_owned()])
        );
        assert_eq!(certificate.claims[0].validator, "definition_span/v1");
    }

    #[test]
    fn empty_dependents_do_not_close_callers() {
        let empty = EvidenceFragment::new(
            "ev_none",
            EvidenceKind::Dependents,
            "weavatrix:dependents",
            "relationships: 0\n",
        );
        let certificate = certificate_from(
            &[tracked(&empty)],
            vec![FACET_CALLERS.to_owned()],
            false,
            Some("compile_context"),
        );
        assert!(!certificate.sufficient);
        assert_eq!(certificate.missing, [FACET_CALLERS]);
        assert!(certificate.claims.is_empty());
    }

    #[test]
    fn dependents_must_name_the_target() {
        let other = EvidenceFragment::new(
            "ev_other",
            EvidenceKind::Dependents,
            "weavatrix:dependents",
            "  -> calls beta (function) src/beta.rs:1",
        );
        let about = EvidenceFragment::new(
            "ev_hit",
            EvidenceKind::Dependents,
            "weavatrix:dependents",
            "  -> calls compile_context (function) src/lib.rs:10",
        );
        let miss = certificate_from(
            &[tracked(&other)],
            vec![FACET_CALLERS.to_owned()],
            false,
            Some("compile_context"),
        );
        assert!(!miss.sufficient);
        let hit = certificate_from(
            &[tracked(&about)],
            vec![FACET_CALLERS.to_owned()],
            false,
            Some("compile_context"),
        );
        assert!(hit.sufficient);
        assert_eq!(hit.claims[0].cardinality, 1);
        assert_eq!(hit.claims[0].validator, "graph_dependents/v1");
    }
}
