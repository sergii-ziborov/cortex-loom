use std::path::Path;

use serde_json::json;

use super::cleanup::prune_incomplete_definition_duplicates;
use super::evidence::{
    EvidenceBundle, EvidenceKind, budget_overrun, fragments, native_call, normalize_graph_stats,
    stamp_bundle,
};
use super::expand::{append_type_expansion_reads, callee_hits_from_evidence};
use super::source_reads::{SourceReadPlan, append_definition_read, append_source_reads};
use super::{WeavatrixAdapter, WeavatrixError};

pub(super) struct TargetedEvidence {
    pub bundle: EvidenceBundle,
    pub search_hits: Vec<crate::source_followup::SearchHit>,
}

impl WeavatrixAdapter {
    pub fn prepare_context(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
    ) -> Result<EvidenceBundle, WeavatrixError> {
        let root = self.canonical_root(repository)?;
        let slot = self.lock_engine(&root)?;
        let mut engine = slot.lock().map_err(|_| WeavatrixError::LockPoisoned)?;
        let engine = &mut *engine;
        let refreshed = engine.refresh_if_stale().map_err(|error| {
            WeavatrixError::Engine(format!("Weavatrix refresh failed: {error}"))
        })?;
        let mut graph_status = native_call(engine, "graph_stats", json!({}))?;
        normalize_graph_stats(&mut graph_status);
        let module_map = native_call(
            engine,
            "module_map",
            json!({"top_n": 24, "include_non_product": false}),
        )?;
        let symbol_context = symbol
            .map(|label| {
                native_call(
                    engine,
                    "context_bundle",
                    json!({
                        "label": label,
                        "max_related": 30,
                        "max_references": 30,
                        "max_source_files": 12
                    }),
                )
            })
            .transpose()?;
        let verification = native_call(
            engine,
            "verified_change",
            json!({
                "task": task,
                "phase": "plan",
                "duplicate_ratchet": true,
                "run_tests": false
            }),
        )?;
        let mut evidence = Vec::new();
        evidence.extend(fragments(
            "WX-GRAPH",
            EvidenceKind::GraphStats,
            "weavatrix:graph_stats",
            &graph_status,
        ));
        evidence.extend(fragments(
            "WX-MODULES",
            EvidenceKind::ModuleMap,
            "weavatrix:module_map",
            &module_map,
        ));
        evidence.extend(fragments(
            "WX-VERIFY",
            EvidenceKind::ChangePlan,
            "weavatrix:verified_change",
            &verification,
        ));
        if let Some(symbol_context) = &symbol_context {
            evidence.extend(fragments(
                "WX-SYMBOL",
                EvidenceKind::SymbolContext,
                "weavatrix:context_bundle",
                symbol_context,
            ));
        }
        let mut bundle = EvidenceBundle {
            repository: repository.to_string_lossy().into_owned(),
            evidence,
            warnings: refreshed
                .then(|| "native Weavatrix graph refreshed from changed source evidence".to_owned())
                .into_iter()
                .collect(),
            ..EvidenceBundle::default()
        };
        stamp_bundle(&mut bundle, &crate::repository_snapshot(&root));
        Ok(bundle)
    }

    pub fn prepare_targeted_context(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
    ) -> Result<EvidenceBundle, WeavatrixError> {
        self.prepare_targeted_context_with(
            repository,
            task,
            symbol,
            budget,
            crate::plan::PlanPolicy::default(),
        )
    }

    /// # Errors
    ///
    /// Returns [`WeavatrixError`] when the repository or native graph cannot
    /// be opened or refreshed.
    pub fn prepare_targeted_context_with(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
    ) -> Result<EvidenceBundle, WeavatrixError> {
        self.collect_targeted(
            repository,
            task,
            symbol,
            budget,
            policy,
            crate::PlanHints::default(),
            false,
            None,
        )
        .map(|gathered| gathered.bundle)
    }

    /// # Errors
    ///
    /// Returns [`WeavatrixError`] when the repository or native graph cannot
    /// be opened or refreshed.
    pub fn prepare_targeted_context_with_source_reads(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
    ) -> Result<EvidenceBundle, WeavatrixError> {
        self.collect_targeted(
            repository,
            task,
            symbol,
            budget,
            policy,
            crate::PlanHints::default(),
            true,
            None,
        )
        .map(|gathered| gathered.bundle)
    }

    /// Gather with active-skill hints, verify sufficiency, and run at most one
    /// deterministic recovery pass.
    ///
    /// # Errors
    ///
    /// Returns [`WeavatrixError`] when the repository or native graph cannot
    /// be opened or refreshed.
    pub fn prepare_verified_targeted_context(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
        hints: crate::PlanHints,
    ) -> Result<(EvidenceBundle, crate::EvidenceSufficiency), WeavatrixError> {
        self.prepare_verified_targeted_context_with_prior(
            repository, task, symbol, budget, policy, hints, None,
        )
    }

    /// As [`Self::prepare_verified_targeted_context`], with prior-run memory.
    ///
    /// # Errors
    ///
    /// Returns [`WeavatrixError`] when the repository or native graph cannot
    /// be opened or refreshed.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_verified_targeted_context_with_prior(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
        mut hints: crate::PlanHints,
        prior: Option<crate::PriorRunMemory>,
    ) -> Result<(EvidenceBundle, crate::EvidenceSufficiency), WeavatrixError> {
        let prior = prior.filter(|memory| !memory.is_empty());
        if prior.is_some() {
            hints.has_prior_attempts = true;
        }
        let source_followup = hints.source_followup_or(true);
        let mut gathered = self.collect_targeted(
            repository,
            task,
            symbol,
            budget,
            policy,
            hints,
            source_followup,
            prior.as_ref(),
        )?;
        let mut report = crate::verify::assess_gathered(
            &gathered.bundle,
            task,
            symbol,
            hints,
            source_followup,
            gathered.search_hits.len(),
            false,
        );
        let mut tried = Vec::new();
        let mut expansions = 0_u32;
        let mut last_added = true;
        while !report.sufficient
            && super::facet_loop::should_expand(
                &report.certificate,
                super::facet_loop::remaining_tokens(&gathered.bundle, budget),
                expansions,
                last_added,
                &report.missing_evidence,
            )
        {
            let Some(key) =
                super::facet_loop::next_gather_expansion(&report.missing_evidence, &tried)
                    .or_else(|| report.certificate.next_expansion().map(ToOwned::to_owned))
            else {
                break;
            };
            if tried.iter().any(|seen| seen == &key) {
                break;
            }
            tried.push(key.clone());
            let before = gathered.bundle.evidence.len();
            let mut focused = report.clone();
            focused.missing_evidence =
                super::facet_loop::expansion_targets(&key, &report.missing_evidence);
            self.retry_targeted(
                repository,
                task,
                symbol,
                budget,
                policy,
                hints,
                source_followup,
                prior.as_ref(),
                &focused,
                &mut gathered,
            )?;
            last_added = gathered.bundle.evidence.len() > before;
            expansions = expansions.saturating_add(1);
            report = crate::verify::assess_gathered(
                &gathered.bundle,
                task,
                symbol,
                hints,
                source_followup,
                gathered.search_hits.len(),
                true,
            );
        }
        report.retry_performed = expansions > 0;
        report.certificate.expansions_performed = expansions;
        Ok((gathered.bundle, report))
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_targeted(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
        hints: crate::PlanHints,
        source_followup: bool,
        prior: Option<&crate::PriorRunMemory>,
    ) -> Result<TargetedEvidence, WeavatrixError> {
        let root = self.canonical_root(repository)?;
        let slot = self.lock_engine(&root)?;
        let mut engine = slot.lock().map_err(|_| WeavatrixError::LockPoisoned)?;
        let engine = &mut *engine;
        let refreshed = engine.refresh_if_stale().map_err(|error| {
            WeavatrixError::Engine(format!("Weavatrix refresh failed: {error}"))
        })?;
        let mut evidence = Vec::new();
        let mut warnings: Vec<String> = refreshed
            .then(|| "native Weavatrix graph refreshed from changed source evidence".to_owned())
            .into_iter()
            .collect();
        let mut search_hits = Vec::new();
        for operation in crate::plan::plan_with_prior(task, symbol, budget, policy, hints, prior) {
            match native_call(engine, operation.tool, operation.arguments.clone()) {
                Ok(value) => {
                    if let Some(overrun) = budget_overrun(operation.tool, &value) {
                        warnings.push(overrun);
                    }
                    if operation.tool == "search_code" {
                        search_hits.extend(crate::source_followup::hits_from_search(&value));
                    }
                    evidence.extend(fragments(
                        operation.id,
                        operation.kind,
                        &format!("weavatrix:{}", operation.tool),
                        &value,
                    ));
                }
                Err(error) => warnings.push(format!("{} unavailable: {error}", operation.tool)),
            }
        }
        if source_followup {
            if crate::plan_intent::is_broad(task) {
                search_hits.extend(callee_hits_from_evidence(&evidence));
            }
            if let Some(symbol) = symbol {
                append_definition_read(
                    engine,
                    &mut evidence,
                    &mut warnings,
                    &search_hits,
                    symbol,
                    budget,
                    false,
                );
            }
            let preferred = crate::verify::source_priority_patterns(task, symbol, hints);
            append_source_reads(
                engine,
                &mut evidence,
                &mut warnings,
                &search_hits,
                budget,
                policy,
                SourceReadPlan {
                    id_prefix: "WX-SOURCE",
                    preferred_patterns: &preferred,
                    window: crate::source_followup::SourceWindow::for_task(task),
                },
            );
            if crate::plan_intent::is_broad(task) {
                append_type_expansion_reads(engine, &mut evidence, &mut warnings, task, budget);
            }
            if let Some(symbol) = symbol {
                prune_incomplete_definition_duplicates(&mut evidence, symbol);
            }
        }
        let mut bundle = EvidenceBundle {
            repository: repository.to_string_lossy().into_owned(),
            evidence,
            warnings,
            ..EvidenceBundle::default()
        };
        stamp_bundle(&mut bundle, &crate::repository_snapshot(&root));
        Ok(TargetedEvidence {
            bundle,
            search_hits,
        })
    }
}
