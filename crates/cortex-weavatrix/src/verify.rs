//! Deterministic evidence sufficiency checks.
//!
//! This is not an answer-quality model. It verifies that gathered and selected
//! packets contain the evidence classes the task requires. A thin packet may
//! be retried once; if it remains thin, the caller must escalate.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::plan::extract_identifiers;
use crate::plan_intent::TaskIntent;
use crate::{EvidenceBundle, EvidenceKind, PlanHints};

#[path = "verify_coverage.rs"]
mod coverage;
use coverage::coverage_requirements;

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
    coverage_text: String,
    /// Original-case bodies for structural, boundary-sensitive checks.
    coverage_fragments: Vec<String>,
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
    let mut profile = profile(bundle.evidence.iter());
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
    let mut profile = profile(selected.iter().copied());
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

fn profile<'a>(evidence: impl Iterator<Item = &'a crate::EvidenceFragment>) -> EvidenceProfile {
    let mut profile = EvidenceProfile::default();
    for item in evidence {
        profile.kinds.insert(item.kind);
        if matches!(
            item.kind,
            EvidenceKind::SourceReads | EvidenceKind::TypeExpansion
        ) {
            profile
                .coverage_text
                .push_str(&item.content.to_ascii_lowercase());
            profile.coverage_text.push('\n');
        }
        if matches!(
            item.kind,
            EvidenceKind::SourceReads | EvidenceKind::SymbolContext | EvidenceKind::TypeExpansion
        ) {
            profile.coverage_fragments.push(item.content.clone());
        }
    }
    profile
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
    let has_identifiers = symbol.is_some() || !extract_identifiers(task).is_empty();
    let mut required_kinds = Vec::new();
    match intent {
        TaskIntent::BlastRadius if symbol.is_some() => {
            required_kinds.push(EvidenceKind::Dependents);
        }
        TaskIntent::ApiContract => required_kinds.push(EvidenceKind::Endpoints),
        TaskIntent::ModuleTopology => required_kinds.push(EvidenceKind::ModuleMap),
        TaskIntent::GitHistory => required_kinds.push(EvidenceKind::GitHistory),
        TaskIntent::StackTrace => required_kinds.push(EvidenceKind::StackTrace),
        TaskIntent::TestSelection => required_kinds.push(EvidenceKind::TestSelection),
        TaskIntent::IdentifierChange
        | TaskIntent::RuntimeConfig
        | TaskIntent::BlastRadius
        | TaskIntent::PriorAttempt => {}
    }
    if hints.has_prior_attempts {
        required_kinds.push(EvidenceKind::Memory);
    }
    if has_identifiers {
        required_kinds.push(EvidenceKind::SearchHits);
        if source_followup {
            required_kinds.push(EvidenceKind::SourceReads);
        }
    }
    if required_kinds.is_empty() {
        required_kinds.push(EvidenceKind::ModuleMap);
    }
    required_kinds.sort_by_key(|kind| kind_name(*kind));
    required_kinds.dedup();

    let mut present: Vec<String> = profile.kinds.iter().copied().map(kind_name_owned).collect();
    present.sort();
    let mut required: Vec<String> = required_kinds
        .iter()
        .copied()
        .map(kind_name_owned)
        .collect();
    let mut missing: Vec<String> = required_kinds
        .iter()
        .copied()
        .filter(|kind| {
            !profile.kinds.contains(kind)
                || (*kind == EvidenceKind::SearchHits && profile.search_hits == 0)
        })
        .map(kind_name_owned)
        .collect();
    if source_followup {
        for coverage in coverage_requirements(task, symbol, intent) {
            let name = format!("source_term:{}", coverage.label);
            required.push(name.clone());
            if coverage
                .content_patterns
                .iter()
                .any(|pattern| profile.coverage_text.contains(pattern))
            {
                present.push(name);
            } else {
                missing.push(name);
            }
        }
        // A definition head is not enough: its braces must balance inside one
        // fragment so a clipped body cannot pass sufficiency.
        if let Some(symbol) = symbol {
            let name = format!("definition:{}", symbol.to_ascii_lowercase());
            required.push(name.clone());
            let complete = profile.coverage_fragments.iter().any(|fragment| {
                crate::source_followup::definition_is_complete(fragment, symbol) == Some(true)
            });
            if complete {
                present.push(name);
            } else {
                missing.push(name);
            }
        }
    }
    required.sort();
    required.dedup();
    present.sort();
    present.dedup();
    missing.sort();
    missing.dedup();
    EvidenceSufficiency {
        sufficient: missing.is_empty(),
        retry_performed,
        required_evidence: required,
        present_evidence: present,
        missing_evidence: missing,
    }
}

pub(crate) fn retry_search_pattern(
    task: &str,
    symbol: Option<&str>,
    hints: PlanHints,
    missing: &[String],
) -> String {
    let intent = hints.intent_or_detect(task);
    let requirements = coverage_requirements(task, symbol, intent);
    let mut patterns = Vec::new();
    let semantic_retry = missing.iter().any(|item| item.starts_with("source_term:"));
    for requirement in requirements {
        let name = format!("source_term:{}", requirement.label);
        let semantic_contract = semantic_retry && !requirement.label.starts_with("identifier:");
        if semantic_contract || missing.iter().any(|item| item == &name) {
            patterns.extend(requirement.search_patterns);
        }
    }
    if missing.iter().any(|item| item == "search_hits") {
        patterns.extend(
            extract_identifiers(task)
                .into_iter()
                .map(|identifier| crate::plan::search_pattern(&[identifier])),
        );
        if let Some(symbol) = symbol {
            patterns.push(crate::plan::search_pattern(&[symbol.to_owned()]));
        }
    }
    patterns.retain(|pattern| !pattern.is_empty());
    patterns.sort();
    patterns.dedup();
    patterns.join("|")
}

pub(crate) fn retry_search_queries(
    task: &str,
    symbol: Option<&str>,
    hints: PlanHints,
    missing: &[String],
) -> Vec<String> {
    let intent = hints.intent_or_detect(task);
    let semantic_retry = missing.iter().any(|item| item.starts_with("source_term:"));
    if !semantic_retry {
        let query = retry_search_pattern(task, symbol, hints, missing);
        return (!query.is_empty()).then_some(query).into_iter().collect();
    }
    let queries: Vec<String> = coverage_requirements(task, symbol, intent)
        .into_iter()
        .filter(|requirement| !requirement.label.starts_with("identifier:"))
        .map(|requirement| requirement.search_patterns.join("|"))
        .filter(|query| !query.is_empty())
        .collect();
    if queries.is_empty() {
        let query = retry_search_pattern(task, symbol, hints, missing);
        return (!query.is_empty()).then_some(query).into_iter().collect();
    }
    queries
}

pub(crate) fn source_priority_patterns(
    task: &str,
    symbol: Option<&str>,
    hints: PlanHints,
) -> Vec<String> {
    let intent = hints.intent_or_detect(task);
    let mut patterns = Vec::new();
    for requirement in coverage_requirements(task, symbol, intent) {
        if !requirement.label.starts_with("identifier:") {
            patterns.extend(requirement.content_patterns);
        }
    }
    patterns.sort();
    patterns.dedup();
    patterns
}

/// Whether a search fragment carries actual hits rather than an empty result.
///
/// Search evidence is rendered as `path:line: text` now, so the old check for
/// the `"path"`/`"line"` JSON keys would report every fragment as empty and
/// fail sufficiency on tasks that had found everything they needed.
fn search_fragment_has_hits(content: &str) -> bool {
    if let Some(rest) = content.strip_prefix(crate::adapter::SEARCH_HEADER) {
        return rest
            .split_whitespace()
            .next()
            .and_then(|count| count.parse::<usize>().ok())
            .is_some_and(|count| count > 0);
    }
    // Fragments split off the head keep hit lines without the header, and a
    // legacy JSON fragment must still be recognised.
    content.contains(".rs:") || (content.contains("\"path\"") && content.contains("\"line\""))
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
        EvidenceKind::TypeExpansion => "type_expansion",
        EvidenceKind::GitHistory => "git_history",
        EvidenceKind::StackTrace => "stack_trace",
        EvidenceKind::TestSelection => "test_selection",
        EvidenceKind::Memory => "memory",
    }
}

fn kind_name_owned(kind: EvidenceKind) -> String {
    kind_name(kind).to_owned()
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
