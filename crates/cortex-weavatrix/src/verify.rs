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
    coverage_text: String,
}

struct CoverageRequirement {
    label: String,
    content_patterns: Vec<String>,
    search_patterns: Vec<String>,
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
            EvidenceKind::SearchHits | EvidenceKind::SourceReads | EvidenceKind::SymbolContext
        ) {
            profile
                .coverage_text
                .push_str(&item.content.to_ascii_lowercase());
            profile.coverage_text.push('\n');
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
        TaskIntent::IdentifierChange | TaskIntent::RuntimeConfig | TaskIntent::BlastRadius => {}
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
        if semantic_retry || missing.iter().any(|item| item == &name) {
            patterns.extend(requirement.search_patterns);
        }
    }
    if missing
        .iter()
        .any(|item| item == "search_hits" || item.starts_with("source_term:"))
    {
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

fn coverage_requirements(
    task: &str,
    symbol: Option<&str>,
    intent: TaskIntent,
) -> Vec<CoverageRequirement> {
    let lower = task.to_ascii_lowercase();
    let mut requirements = Vec::new();
    let mut identifiers = extract_identifiers(task);
    if let Some(symbol) = symbol
        && !identifiers.iter().any(|identifier| identifier == symbol)
    {
        identifiers.insert(0, symbol.to_owned());
    }
    for identifier in identifiers.into_iter().take(4) {
        requirements.push(requirement(
            format!("identifier:{identifier}"),
            &[&identifier.to_ascii_lowercase()],
            &[&crate::plan::search_pattern(&[identifier])],
        ));
    }
    if lower.contains("uncalibrat") || lower.contains("profile gate") {
        requirements.push(requirement(
            "profile_gate_state",
            &["gate_passed", "gatepassed"],
            &["gate_passed", "gatePassed"],
        ));
        requirements.push(requirement(
            "profile_rejection",
            &["notcalibrated", "not calibrated"],
            &["NotCalibrated", "not calibrated"],
        ));
        requirements.push(requirement(
            "profile_selection",
            &["fn select", "pub fn select"],
            &["select"],
        ));
    }
    if lower.contains("env flag") || lower.contains("environment variable") {
        requirements.push(requirement(
            "runtime_flag",
            &["cortex_"],
            &["CORTEX_[A-Z0-9_]+"],
        ));
    }
    if lower.contains("spawn") {
        requirements.push(requirement(
            "spawn_lifecycle",
            &["fn spawn", "pub fn spawn"],
            &["spawn"],
        ));
    }
    if lower.contains("shadowhandle") && lower.contains("spawn") {
        requirements.push(requirement(
            "shadow_observe",
            &["fn observe", "pub fn observe"],
            &["observe"],
        ));
    }
    if lower.contains("wire") || lower.contains("wiring") {
        requirements.push(requirement(
            "router_wiring",
            &["router"],
            &["[A-Za-z0-9_]*Router"],
        ));
        requirements.push(requirement(
            "tier_merge",
            &["merge_", "merge tiers"],
            &["merge_[A-Za-z0-9_]+"],
        ));
    }
    if lower.contains("policy") || lower.contains("permit") || lower.contains("refuse cpu") {
        requirements.push(requirement(
            "policy_predicate",
            &["fn permits", "pub fn permits"],
            &["permits"],
        ));
    }
    if intent == TaskIntent::ModuleTopology
        && lower.contains("run")
        && (lower.contains("persist") || lower.contains("store"))
    {
        requirements.push(requirement(
            "run_persistence",
            &["run_store", "runstore"],
            &["run_store", "RunStore"],
        ));
        requirements.push(requirement(
            "store_entrypoint",
            &["fn open", "pub fn open"],
            &["open"],
        ));
    }
    requirements
}

fn requirement(
    label: impl Into<String>,
    content_patterns: &[&str],
    search_patterns: &[&str],
) -> CoverageRequirement {
    CoverageRequirement {
        label: label.into(),
        content_patterns: content_patterns
            .iter()
            .map(|pattern| pattern.to_ascii_lowercase())
            .collect(),
        search_patterns: search_patterns
            .iter()
            .map(|pattern| (*pattern).to_owned())
            .collect(),
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
        assert_eq!(
            thin.missing_evidence,
            ["source_reads", "source_term:identifier:CORTEX_LLM"]
        );
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

    #[test]
    fn profile_gate_requires_semantic_source_coverage_not_just_source_presence() {
        let bundle = EvidenceBundle {
            repository: "repo".to_owned(),
            evidence: vec![
                fragment(
                    "WX-SEARCH",
                    EvidenceKind::SearchHits,
                    r#"{"matches":[{"path":"crates/cortex-llm/src/profile.rs","line":1}]}"#,
                ),
                fragment(
                    "WX-SOURCE",
                    EvidenceKind::SourceReads,
                    "pub struct ProfileRegistry;",
                ),
            ],
            warnings: Vec::new(),
        };
        let hints = PlanHints {
            intent: Some(crate::IntentHint::RuntimeConfig),
            source_followup: Some(true),
            skip_change_plan: true,
        };
        let report = assess_compiled(
            &bundle,
            &["WX-SEARCH".to_owned(), "WX-SOURCE".to_owned()],
            "How does `ProfileRegistry` refuse an uncalibrated classification profile?",
            Some("ProfileRegistry"),
            hints,
            true,
            false,
        );
        assert!(!report.sufficient);
        assert!(
            report
                .missing_evidence
                .iter()
                .any(|item| item == "source_term:profile_gate_state")
        );
        assert!(
            report
                .missing_evidence
                .iter()
                .any(|item| item == "source_term:profile_rejection")
        );
    }

    #[test]
    fn semantic_retry_replays_the_whole_contract_not_only_the_last_gap() {
        let profile = retry_search_pattern(
            "How does `ProfileRegistry` refuse an uncalibrated classification profile?",
            Some("ProfileRegistry"),
            PlanHints::default(),
            &["source_term:profile_selection".to_owned()],
        );
        assert!(profile.contains("select"));
        assert!(profile.contains("gate_passed"));
        assert!(profile.contains("NotCalibrated"));

        let shadow = retry_search_pattern(
            "How is `ShadowHandle` spawned, and which env flag turns shadow mode on?",
            Some("ShadowHandle"),
            PlanHints::default(),
            &["source_term:spawn_lifecycle".to_owned()],
        );
        assert!(shadow.contains("spawn"));
        assert!(shadow.contains("observe"));
        assert!(shadow.contains("CORTEX_"));
    }
}
