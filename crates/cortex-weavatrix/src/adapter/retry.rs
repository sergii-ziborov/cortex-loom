use std::path::Path;

use serde_json::json;
use weavatrix_rust::Weavatrix;

use super::cleanup::prune_incomplete_definition_duplicates;
use super::evidence::{EvidenceKind, fragments, native_call, stamp_bundle};
use super::expand::append_type_expansion_reads;
use super::gather::TargetedEvidence;
use super::source_reads::{SourceReadPlan, append_definition_read, append_source_reads};
use super::{WeavatrixAdapter, WeavatrixError};

impl WeavatrixAdapter {
    /// Re-read the named symbol's definition with a doubled window when the
    /// sufficiency report flagged it as incomplete.
    fn retry_definition(
        engine: &mut Weavatrix,
        symbol: Option<&str>,
        budget: u32,
        initial: &crate::EvidenceSufficiency,
        gathered: &mut TargetedEvidence,
    ) {
        let Some(symbol) = symbol else { return };
        if !initial
            .missing_evidence
            .iter()
            .any(|item| item.starts_with("definition:"))
        {
            return;
        }
        append_definition_read(
            engine,
            &mut gathered.bundle.evidence,
            &mut gathered.bundle.warnings,
            &gathered.search_hits,
            symbol,
            budget,
            true,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn retry_targeted(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
        hints: crate::PlanHints,
        source_followup: bool,
        prior: Option<&crate::PriorRunMemory>,
        initial: &crate::EvidenceSufficiency,
        gathered: &mut TargetedEvidence,
    ) -> Result<(), WeavatrixError> {
        gathered.bundle.warnings.push(format!(
            "evidence sufficiency retry: missing {}",
            initial.missing_evidence.join(", ")
        ));
        let root = self.canonical_root(repository)?;
        let slot = self.lock_engine(&root)?;
        let mut engine = slot.lock().map_err(|_| WeavatrixError::LockPoisoned)?;
        let engine = &mut *engine;
        Self::retry_definition(engine, symbol, budget, initial, gathered);
        let needs_search_retry = initial
            .missing_evidence
            .iter()
            .any(|kind| kind == "search_hits" || kind.starts_with("source_term:"));
        if needs_search_retry {
            let first_retry_hit = gathered.search_hits.len();
            retry_wide_search(
                engine,
                &mut gathered.bundle.evidence,
                &mut gathered.bundle.warnings,
                &mut gathered.search_hits,
                task,
                symbol,
                hints,
                &initial.missing_evidence,
                budget,
                policy,
            );
            if source_followup && gathered.search_hits.len() > first_retry_hit {
                rebuild_retry_sources(engine, gathered, task, symbol, hints, budget, policy);
            }
        }
        let inventory_glob = crate::inventory(&root).glob();
        let operations = crate::plan::plan_with_prior(
            task,
            symbol,
            budget,
            policy,
            hints,
            prior,
            Some(inventory_glob.as_str()),
        );
        for operation in operations {
            let kind = crate::verify::kind_name(operation.kind);
            if !initial
                .missing_evidence
                .iter()
                .any(|missing| missing == kind)
                || operation.kind == EvidenceKind::SearchHits
            {
                continue;
            }
            match native_call(engine, operation.tool, operation.arguments) {
                Ok(value) => gathered.bundle.evidence.extend(fragments(
                    &format!("WX-RETRY-{}", operation.id.trim_start_matches("WX-")),
                    operation.kind,
                    &format!("weavatrix:{}", operation.tool),
                    &value,
                )),
                Err(error) => gathered
                    .bundle
                    .warnings
                    .push(format!("{} retry unavailable: {error}", operation.tool)),
            }
        }
        let has_source = gathered
            .bundle
            .evidence
            .iter()
            .any(|item| item.kind == EvidenceKind::SourceReads);
        if source_followup && !has_source {
            append_source_reads(
                engine,
                &mut gathered.bundle.evidence,
                &mut gathered.bundle.warnings,
                &gathered.search_hits,
                budget,
                policy,
                SourceReadPlan {
                    id_prefix: "WX-RETRY-SOURCE",
                    preferred_patterns: &[],
                    window: crate::source_followup::SourceWindow::for_task(task),
                },
            );
        }
        if let Some(symbol) = symbol {
            prune_incomplete_definition_duplicates(&mut gathered.bundle.evidence, symbol);
        }
        if let Some(snapshot) = gathered.bundle.snapshot_id.clone() {
            stamp_bundle(&mut gathered.bundle, &snapshot);
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn retry_wide_search(
    engine: &mut Weavatrix,
    evidence: &mut Vec<super::evidence::EvidenceFragment>,
    warnings: &mut Vec<String>,
    search_hits: &mut Vec<crate::source_followup::SearchHit>,
    task: &str,
    symbol: Option<&str>,
    hints: crate::PlanHints,
    missing: &[String],
    budget: u32,
    policy: crate::plan::PlanPolicy,
) {
    let queries = crate::verify::retry_search_queries(task, symbol, hints, missing);
    if queries.is_empty() {
        warnings.push("wide search retry skipped: no missing semantic terms".to_owned());
        return;
    }
    let token_budget = policy
        .search_tokens
        .min(budget.saturating_mul(2) / 5)
        .max(200);
    let query_count = u32::try_from(queries.len()).unwrap_or(u32::MAX).max(1);
    let per_query_budget = (token_budget / query_count).max(200);
    for (index, query) in queries.into_iter().enumerate() {
        let arguments = retry_search_arguments(&query, per_query_budget);
        match native_call(engine, "search_code", arguments) {
            Ok(value) => {
                search_hits.extend(crate::source_followup::hits_from_search(&value));
                evidence.extend(fragments(
                    &format!("WX-RETRY-SEARCH-{}", index + 1),
                    EvidenceKind::SearchHits,
                    "weavatrix:search_code",
                    &value,
                ));
            }
            Err(error) => warnings.push(format!("wide search retry unavailable: {error}")),
        }
    }
}

fn retry_search_arguments(query: &str, token_budget: u32) -> serde_json::Value {
    json!({
        "query": query,
        "is_regex": true,
        "before": 2,
        "after": 2,
        "max_results": 40,
        "glob": "{src,apps,crates,ui,config}/**/*",
        "token_budget": token_budget,
    })
}

fn rebuild_retry_sources(
    engine: &mut Weavatrix,
    gathered: &mut TargetedEvidence,
    task: &str,
    symbol: Option<&str>,
    hints: crate::PlanHints,
    budget: u32,
    policy: crate::plan::PlanPolicy,
) {
    let preferred_patterns = crate::verify::source_priority_patterns(task, symbol, hints);
    // The definition read survives the source rebuild: it is the one
    // fragment whose absence the retry may exist to repair.
    gathered.bundle.evidence.retain(|item| {
        item.kind != EvidenceKind::SourceReads
            || item.facet == cortex_context::EvidenceFacet::Definition
    });
    append_source_reads(
        engine,
        &mut gathered.bundle.evidence,
        &mut gathered.bundle.warnings,
        &gathered.search_hits,
        budget,
        policy,
        SourceReadPlan {
            id_prefix: "WX-RETRY-SOURCE",
            preferred_patterns: &preferred_patterns,
            window: crate::source_followup::SourceWindow::for_task(task),
        },
    );
    if crate::plan_intent::is_broad(task) {
        append_type_expansion_reads(
            engine,
            &mut gathered.bundle.evidence,
            &mut gathered.bundle.warnings,
            task,
            budget,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::retry_search_arguments;

    #[test]
    fn recovery_search_stays_inside_source_and_config_trees() {
        let arguments = retry_search_arguments("ArchiveOptions", 1_400);

        assert_eq!(arguments["glob"], "{src,apps,crates,ui,config}/**/*");
    }
}
