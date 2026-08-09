use std::path::Path;

use serde_json::json;

use super::evidence::{
    EvidenceBundle, EvidenceKind, append_source_reads, budget_overrun, fragments, native_call,
    retry_wide_search,
};
use super::{WeavatrixAdapter, WeavatrixError};

struct TargetedEvidence {
    bundle: EvidenceBundle,
    search_hits: Vec<crate::source_followup::SearchHit>,
}

impl WeavatrixAdapter {
    pub fn prepare_context(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
    ) -> Result<EvidenceBundle, WeavatrixError> {
        let root = self.canonical_root(repository)?;
        let mut sessions = self
            .engines
            .lock()
            .map_err(|_| WeavatrixError::LockPoisoned)?;
        let engine = Self::session(&mut sessions, &root)?;
        let refreshed = engine.refresh_if_stale().map_err(|error| {
            WeavatrixError::Engine(format!("Weavatrix refresh failed: {error}"))
        })?;
        let graph_status = native_call(engine, "graph_stats", json!({}))?;
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
        Ok(EvidenceBundle {
            repository: repository.to_string_lossy().into_owned(),
            evidence,
            warnings: refreshed
                .then(|| "native Weavatrix graph refreshed from changed source evidence".to_owned())
                .into_iter()
                .collect(),
        })
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
        let source_followup = hints.source_followup_or(true);
        let mut gathered = self.collect_targeted(
            repository,
            task,
            symbol,
            budget,
            policy,
            hints,
            source_followup,
        )?;
        let initial = crate::verify::assess_gathered(
            &gathered.bundle,
            task,
            symbol,
            hints,
            source_followup,
            gathered.search_hits.len(),
            false,
        );
        if initial.sufficient {
            return Ok((gathered.bundle, initial));
        }
        self.retry_targeted(
            repository,
            task,
            symbol,
            budget,
            policy,
            hints,
            source_followup,
            &initial,
            &mut gathered,
        )?;
        let final_report = crate::verify::assess_gathered(
            &gathered.bundle,
            task,
            symbol,
            hints,
            source_followup,
            gathered.search_hits.len(),
            true,
        );
        Ok((gathered.bundle, final_report))
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
    ) -> Result<TargetedEvidence, WeavatrixError> {
        let root = self.canonical_root(repository)?;
        let mut sessions = self
            .engines
            .lock()
            .map_err(|_| WeavatrixError::LockPoisoned)?;
        let engine = Self::session(&mut sessions, &root)?;
        let refreshed = engine.refresh_if_stale().map_err(|error| {
            WeavatrixError::Engine(format!("Weavatrix refresh failed: {error}"))
        })?;
        let mut evidence = Vec::new();
        let mut warnings: Vec<String> = refreshed
            .then(|| "native Weavatrix graph refreshed from changed source evidence".to_owned())
            .into_iter()
            .collect();
        let mut search_hits = Vec::new();
        for operation in crate::plan::plan_with_hints(task, symbol, budget, policy, hints) {
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
            append_source_reads(
                engine,
                &mut evidence,
                &mut warnings,
                &search_hits,
                budget,
                policy,
                "WX-SOURCE",
            );
        }
        Ok(TargetedEvidence {
            bundle: EvidenceBundle {
                repository: repository.to_string_lossy().into_owned(),
                evidence,
                warnings,
            },
            search_hits,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn retry_targeted(
        &self,
        repository: &Path,
        task: &str,
        symbol: Option<&str>,
        budget: u32,
        policy: crate::plan::PlanPolicy,
        hints: crate::PlanHints,
        source_followup: bool,
        initial: &crate::EvidenceSufficiency,
        gathered: &mut TargetedEvidence,
    ) -> Result<(), WeavatrixError> {
        gathered.bundle.warnings.push(format!(
            "evidence sufficiency retry: missing {}",
            initial.missing_evidence.join(", ")
        ));
        let root = self.canonical_root(repository)?;
        let mut sessions = self
            .engines
            .lock()
            .map_err(|_| WeavatrixError::LockPoisoned)?;
        let engine = Self::session(&mut sessions, &root)?;
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
            if source_followup {
                let retry_hits = gathered.search_hits[first_retry_hit..].to_vec();
                if !retry_hits.is_empty() {
                    gathered
                        .bundle
                        .evidence
                        .retain(|item| item.kind != EvidenceKind::SourceReads);
                    append_source_reads(
                        engine,
                        &mut gathered.bundle.evidence,
                        &mut gathered.bundle.warnings,
                        &retry_hits,
                        budget,
                        policy,
                        "WX-RETRY-SOURCE",
                    );
                }
            }
        }
        for operation in crate::plan::plan_with_hints(task, symbol, budget, policy, hints) {
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
                "WX-RETRY-SOURCE",
            );
        }
        Ok(())
    }
}
