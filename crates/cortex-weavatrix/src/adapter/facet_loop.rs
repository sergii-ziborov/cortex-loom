//! Bounded facet expansion: one missing facet per turn, then stop.

use cortex_context::{CharDiv4Counter, CoverageCertificate, FACET_DEFINITION, TokenCounter};

use crate::EvidenceBundle;

/// Hard cap on gather-time expansions for one compile.
pub(super) const MAX_FACET_EXPANSIONS: u32 = 4;
/// Stop when the remaining compile budget cannot carry another atom.
pub(super) const MIN_REMAINING_TOKENS: u32 = 400;

#[must_use]
pub(super) fn remaining_tokens(bundle: &EvidenceBundle, budget: u32) -> u32 {
    let used = bundle
        .evidence
        .iter()
        .map(|fragment| CharDiv4Counter.count(&fragment.content))
        .fold(0_u32, u32::saturating_add);
    budget.saturating_sub(used)
}

#[must_use]
pub(super) fn should_expand(
    certificate: &CoverageCertificate,
    remaining: u32,
    expansions: u32,
    last_added: bool,
    gather_missing: &[String],
) -> bool {
    expansions < MAX_FACET_EXPANSIONS
        && remaining >= MIN_REMAINING_TOKENS
        && last_added
        && (certificate.next_expansion().is_some() || !gather_missing.is_empty())
}

#[must_use]
pub(super) fn expansion_targets(key: &str, all_missing: &[String]) -> Vec<String> {
    if key == FACET_DEFINITION {
        let named: Vec<String> = all_missing
            .iter()
            .filter(|item| item.starts_with("definition:"))
            .cloned()
            .collect();
        return if named.is_empty() {
            vec!["definition:target".to_owned()]
        } else {
            named
        };
    }
    if key == cortex_context::FACET_CALLERS {
        return vec!["dependents".to_owned()];
    }
    if key == cortex_context::FACET_PUBLIC_API {
        return vec!["endpoints".to_owned()];
    }
    if key == cortex_context::FACET_TESTS {
        return vec!["test_selection".to_owned()];
    }
    if key == cortex_context::FACET_GIT {
        return vec!["git_history".to_owned()];
    }
    if key == cortex_context::FACET_MEMORY {
        return vec!["memory".to_owned()];
    }
    if key == cortex_context::FACET_ERRORS {
        return vec!["stack_trace".to_owned()];
    }
    vec![key.to_owned()]
}

/// Map a gather missing key (kind or `source_term`) onto a certificate facet
/// so the loop and L2 handles share one vocabulary.
#[must_use]
pub(super) fn expansion_key(missing: &str) -> String {
    if missing.starts_with("definition:") {
        FACET_DEFINITION.to_owned()
    } else if missing == "dependents" {
        cortex_context::FACET_CALLERS.to_owned()
    } else if missing == "endpoints" {
        cortex_context::FACET_PUBLIC_API.to_owned()
    } else if missing == "test_selection" {
        cortex_context::FACET_TESTS.to_owned()
    } else if missing == "git_history" {
        cortex_context::FACET_GIT.to_owned()
    } else if missing == "memory" {
        cortex_context::FACET_MEMORY.to_owned()
    } else if missing == "stack_trace" {
        cortex_context::FACET_ERRORS.to_owned()
    } else {
        missing.to_owned()
    }
}

#[must_use]
pub(super) fn next_gather_expansion(missing: &[String], tried: &[String]) -> Option<String> {
    let mut keys: Vec<String> = missing.iter().map(|item| expansion_key(item)).collect();
    keys.extend(missing.iter().cloned());
    keys.sort();
    keys.dedup();
    let mut certificate = CoverageCertificate {
        missing: keys,
        sufficient: false,
        ..CoverageCertificate::default()
    };
    // Prefer the certificate order, then leftover gather keys.
    if let Some(facet) = certificate.next_expansion().map(str::to_owned) {
        if !tried.iter().any(|item| item == &facet) {
            return Some(facet);
        }
        certificate.missing.retain(|item| item != &facet);
    }
    missing
        .iter()
        .find(|item| {
            !tried
                .iter()
                .any(|seen| seen == *item || expansion_key(item) == *seen)
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EvidenceBundle, EvidenceFragment, EvidenceKind};

    #[test]
    fn definition_outranks_search_in_the_expansion_queue() {
        let next = next_gather_expansion(
            &[
                "search_hits".to_owned(),
                "definition:archiveoptions".to_owned(),
            ],
            &[],
        );
        assert_eq!(next.as_deref(), Some(FACET_DEFINITION));
    }

    #[test]
    fn a_repeat_or_empty_gain_stops_the_loop() {
        let mut certificate = CoverageCertificate {
            missing: vec![FACET_DEFINITION.to_owned()],
            sufficient: false,
            ..CoverageCertificate::default()
        };
        assert!(should_expand(&certificate, 1_000, 0, true, &[]));
        assert!(!should_expand(&certificate, 1_000, 0, false, &[]));
        assert!(!should_expand(&certificate, 50, 0, true, &[]));
        assert!(!should_expand(
            &certificate,
            1_000,
            MAX_FACET_EXPANSIONS,
            true,
            &[]
        ));
        certificate.missing.clear();
        assert!(!should_expand(&certificate, 1_000, 0, true, &[]));
        assert!(should_expand(
            &certificate,
            1_000,
            0,
            true,
            &["search_hits".to_owned()]
        ));
    }

    #[test]
    fn remaining_tokens_shrink_with_assembled_evidence() {
        let bundle = EvidenceBundle {
            repository: "repo".to_owned(),
            evidence: vec![EvidenceFragment::new(
                "a",
                EvidenceKind::SourceReads,
                "src",
                "x".repeat(400),
            )],
            warnings: Vec::new(),
            ..EvidenceBundle::default()
        };
        assert!(remaining_tokens(&bundle, 4_000) < 4_000);
    }
}
