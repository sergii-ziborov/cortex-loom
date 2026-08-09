//! Deterministic evidence sufficiency checks.
//!
//! This is deliberately not an answer-quality model. It verifies that the
//! gathered and finally selected packet contains the evidence classes the
//! task shape requires. A thin packet may be retried once by the adapter; if
//! it is still thin, the caller must escalate rather than treat absence as a
//! verified answer.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::plan::extract_identifiers;
use crate::plan_intent::TaskIntent;
use crate::{EvidenceBundle, EvidenceKind, PlanHints};

/// Result of the gather/verify phase exposed with the compiled context.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceSufficiency {
    pub sufficient: bool,
    pub retry_performed: bool,
    pub required_evidence: Vec<String>,
    pub present_evidence: Vec<String>,
    pub missing_evidence: Vec<String>,
}

#[derive(Default)]
struct EvidenceProfile {
    kinds: HashSet<EvidenceKind>,
    search_hits: usize,
}

/// Assess the evidence gathered before compilation. `search_hits` is the
/// parsed hit count retained by the adapter, so an empty search result cannot
/// pass merely because its response fragment exists.
pub(crate) fn assess_gathered(
    bundle: &EvidenceBundle,
    task: &str,
    symbol: Option<&str>,
    hints: PlanHints,
    source_followup: bool,
    search_hits: usize,
    retry_performed: bool,
) -> EvidenceSufficiency {
    let mut profile = profile(bundle.evidence.iter().map(|item| item.kind));
    profile.search_hits = search_hits;
    assess(
        &profile,
        task,
        symbol,
        hints,
        source_followup,
        retry_performed,
    )
}

/// Verify the evidence that survived the final compiler budget.
#[must_use]
pub fn assess_compiled(
    bundle: &EvidenceBundle,
    included_ids: &[String],
    task: &str,
    symbol: Option<&str>,
    hints: PlanHints,
    source_followup: bool,
    retry_performed: bool,
) -> EvidenceSufficiency {
    let included: HashSet<&str> = included_ids.iter().map(String::as_str).collect();
    let selected: Vec<_> = bundle
        .evidence
        .iter()
        .filter(|item| included.contains(item.id.as_str()))
        .collect();
    let mut profile = profile(selected.iter().map(|item| item.kind));
    profile.search_hits = selected
        .iter()
        .filter(|item| item.kind == EvidenceKind::SearchHits)
        .filter(|item| search_fragment_has_hits(&item.content))
        .count();
    assess(
        &profile,
        task,
        symbol,
        hints,
        source_followup,
        retry_performed,
    )
}

fn profile(kinds: impl Iterator<Item = EvidenceKind>) -> EvidenceProfile {
    EvidenceProfile {
        kinds: kinds.collect(),
        search_hits: 0,
    }
}

fn assess(
    profile: &EvidenceProfile,
    task: &str,
    symbol: Option<&str>,
    hints: PlanHints,
    source_followup: bool,
    retry_performed: bool,
) -> EvidenceSufficiency {
    let intent = hints.intent_or_detect(task);
    let has_identifiers = !extract_identifiers(task).is_empty();
    let mut required = Vec::new();
    match intent {
        TaskIntent::BlastRadius if symbol.is_some() => required.push(EvidenceKind::Dependents),
        TaskIntent::ApiContract => required.push(EvidenceKind::Endpoints),
        TaskIntent::ModuleTopology => required.push(EvidenceKind::ModuleMap),
        TaskIntent::IdentifierChange | TaskIntent::RuntimeConfig | TaskIntent::BlastRadius => {}
    }
    if has_identifiers {
        required.push(EvidenceKind::SearchHits);
        if source_followup {
            required.push(EvidenceKind::SourceReads);
        }
    }
    if required.is_empty() {
        required.push(EvidenceKind::ModuleMap);
    }
    required.sort_by_key(|kind| kind_name(*kind));
    required.dedup();

    let mut present: Vec<_> = profile.kinds.iter().copied().collect();
    present.sort_by_key(|kind| kind_name(*kind));
    let missing: Vec<_> = required
        .iter()
        .copied()
        .filter(|kind| {
            !profile.kinds.contains(kind)
                || (*kind == EvidenceKind::SearchHits && profile.search_hits == 0)
        })
        .collect();
    EvidenceSufficiency {
        sufficient: missing.is_empty(),
        retry_performed,
        required_evidence: required.into_iter().map(kind_name_owned).collect(),
        present_evidence: present.into_iter().map(kind_name_owned).collect(),
        missing_evidence: missing.into_iter().map(kind_name_owned).collect(),
    }
}

fn search_fragment_has_hits(content: &str) -> bool {
    content.contains("\"path\"") && content.contains("\"line\"")
}

pub(crate) const fn kind_name(kind: EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::GraphStats => "graph_stats",
        EvidenceKind::ModuleMap => "module_map",
        EvidenceKind::ChangePlan => "change_plan",
        EvidenceKind::SymbolContext => "symbol_context",
        EvidenceKind::SearchHits => "search_hits",
        EvidenceKind::Dependents => "dependents",
        EvidenceKind::Endpoints => "endpoints",
        EvidenceKind::SourceReads => "source_reads",
    }
}

fn kind_name_owned(kind: EvidenceKind) -> String {
    kind_name(kind).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EvidenceFragment;

    fn fragment(id: &str, kind: EvidenceKind, content: &str) -> EvidenceFragment {
        EvidenceFragment {
            id: id.to_owned(),
            kind,
            source: "test".to_owned(),
            content: content.to_owned(),
            head: true,
        }
    }

    #[test]
    fn config_context_is_thin_until_search_and_source_both_survive() {
        let bundle = EvidenceBundle {
            repository: "repo".to_owned(),
            evidence: vec![
                fragment(
                    "WX-SEARCH",
                    EvidenceKind::SearchHits,
                    r#"{"matches":[{"path":"config/a.json","line":2}]}"#,
                ),
                fragment("WX-SOURCE", EvidenceKind::SourceReads, "CORTEX_LLM"),
            ],
            warnings: Vec::new(),
        };
        let hints = PlanHints {
            intent: Some(crate::IntentHint::RuntimeConfig),
            source_followup: Some(true),
            skip_change_plan: true,
        };
        let thin = assess_compiled(
            &bundle,
            &["WX-SEARCH".to_owned()],
            "Inspect `CORTEX_LLM`",
            None,
            hints,
            true,
            false,
        );
        assert_eq!(thin.missing_evidence, ["source_reads"]);
        let enough = assess_compiled(
            &bundle,
            &["WX-SEARCH".to_owned(), "WX-SOURCE".to_owned()],
            "Inspect `CORTEX_LLM`",
            None,
            hints,
            true,
            true,
        );
        assert!(enough.sufficient);
        assert!(enough.retry_performed);
    }
}
