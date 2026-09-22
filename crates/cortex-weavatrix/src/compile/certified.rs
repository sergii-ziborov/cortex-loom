//! Compile, certify the selected set, then render L0/L2 from that set.
//!
//! A one-shot recompile after the first certificate can evict a citation
//! that the already-rendered `WX-MAP` still calls satisfied. The loop
//! re-renders the map until the ledger matches the selected ids, then
//! freezes that set so a larger map cannot pull omitted evidence back.

use std::collections::HashMap;

use cortex_context::ContextError;

use crate::context::{
    CompiledEvidenceBundle, compile_evidence_bundle, compile_evidence_bundle_layered,
};
use crate::{EvidenceBundle, PlanHints, assess_compiled};

const MAX_SETTLE: u32 = 3;

/// Select evidence, certify it, layer L0/L2, and settle the map.
///
/// # Errors
///
/// Returns when the underlying compile fails closed.
#[allow(clippy::implicit_hasher, clippy::too_many_arguments)]
pub fn compile_certified_bundle(
    bundle: EvidenceBundle,
    task: &str,
    symbol: Option<&str>,
    max_tokens: u32,
    relevance: Option<&HashMap<String, f64>>,
    hints: PlanHints,
    source_followup: bool,
    retry_performed: bool,
) -> Result<CompiledEvidenceBundle, ContextError> {
    let mut compiled = compile_evidence_bundle(bundle.clone(), task, max_tokens, relevance)?;
    attach_sufficiency(
        &mut compiled,
        &bundle,
        task,
        symbol,
        hints,
        source_followup,
        retry_performed,
    );
    for _ in 0..MAX_SETTLE {
        let Some(certificate) = compiled
            .sufficiency
            .as_ref()
            .map(|report| report.certificate.clone())
        else {
            return Ok(compiled);
        };
        let mut layered = compile_evidence_bundle_layered(
            bundle.clone(),
            task,
            max_tokens,
            relevance,
            Some(&certificate),
        )?;
        attach_sufficiency(
            &mut layered,
            &bundle,
            task,
            symbol,
            hints,
            source_followup,
            retry_performed,
        );
        if layered
            .sufficiency
            .as_ref()
            .is_some_and(|report| report.certificate.ledger_matches(&certificate))
        {
            return Ok(layered);
        }
        compiled = layered;
    }
    freeze_layers(compiled, bundle, task, max_tokens, relevance)
}

fn freeze_layers(
    compiled: CompiledEvidenceBundle,
    bundle: EvidenceBundle,
    task: &str,
    max_tokens: u32,
    relevance: Option<&HashMap<String, f64>>,
) -> Result<CompiledEvidenceBundle, ContextError> {
    let Some(report) = compiled.sufficiency.clone() else {
        return Ok(compiled);
    };
    let kept: Vec<_> = bundle
        .evidence
        .iter()
        .filter(|fragment| {
            compiled
                .context
                .included_ids
                .iter()
                .any(|id| id == &fragment.id)
        })
        .cloned()
        .collect();
    let frozen = EvidenceBundle {
        repository: bundle.repository,
        evidence: kept,
        warnings: bundle.warnings,
        snapshot_id: bundle.snapshot_id,
    };
    let mut layered = compile_evidence_bundle_layered(
        frozen,
        task,
        max_tokens,
        relevance,
        Some(&report.certificate),
    )?;
    layered.sufficiency = Some(report);
    layered.warnings.push(
        "decision map settled from the selected set after the layer budget shifted citations"
            .to_owned(),
    );
    Ok(layered)
}

fn attach_sufficiency(
    compiled: &mut CompiledEvidenceBundle,
    bundle: &EvidenceBundle,
    task: &str,
    symbol: Option<&str>,
    hints: PlanHints,
    source_followup: bool,
    retry_performed: bool,
) {
    let mut report = assess_compiled(
        bundle,
        &compiled.context.included_ids,
        task,
        symbol,
        hints,
        source_followup,
        retry_performed,
    );
    if !report.sufficient {
        compiled.context.requires_upstream = true;
        compiled.warnings.push(format!(
            "evidence remains thin after verification: {}",
            report.missing_evidence.join(", ")
        ));
    }
    report
        .certificate
        .packet_id
        .clone_from(&compiled.context.packet_id);
    report
        .certificate
        .snapshot_id
        .clone_from(&compiled.context.snapshot_id);
    compiled.sufficiency = Some(report);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EvidenceFragment, EvidenceKind};
    use cortex_context::CoverageCertificate;

    fn map_cites_omitted(certificate: &CoverageCertificate, included: &[String]) -> bool {
        certificate.satisfied.values().flatten().any(|id| {
            id != "TASK"
                && id != "WX-MAP"
                && id != "WX-EXPAND"
                && !included.iter().any(|kept| kept == id)
        })
    }

    fn fragment(id: &str, kind: EvidenceKind, content: &str) -> EvidenceFragment {
        EvidenceFragment::new(id, kind, format!("weavatrix:{id}"), content)
    }

    #[test]
    fn one_shot_layering_can_keep_a_citation_the_packet_dropped() {
        let bundle = eviction_bundle();
        let task = "Who calls `alpha` and what breaks if it changes?";
        let mut stale = None;
        for budget in (240..640).step_by(20) {
            let Ok(plain) = compile_evidence_bundle(bundle.clone(), task, budget, None) else {
                continue;
            };
            if !plain
                .context
                .included_ids
                .iter()
                .any(|id| id == "ev_callers")
            {
                continue;
            }
            let report = assess_compiled(
                &bundle,
                &plain.context.included_ids,
                task,
                Some("alpha"),
                PlanHints::default(),
                false,
                false,
            );
            if !report.certificate.satisfied.contains_key("direct_callers") {
                continue;
            }
            let Ok(layered) = compile_evidence_bundle_layered(
                bundle.clone(),
                task,
                budget,
                None,
                Some(&report.certificate),
            ) else {
                continue;
            };
            if layered
                .context
                .included_ids
                .iter()
                .any(|id| id == "ev_callers")
            {
                continue;
            }
            stale = Some((budget, layered.context.content.contains("ev_callers")));
            break;
        }
        let Some((budget, map_still_names_callers)) = stale else {
            return;
        };
        assert!(
            map_still_names_callers,
            "budget {budget}: expected the one-shot map to keep the evicted id"
        );
        let compiled = compile_certified_bundle(
            bundle,
            task,
            Some("alpha"),
            budget,
            None,
            PlanHints::default(),
            false,
            false,
        )
        .expect("compile");
        assert!(!map_cites_omitted(
            &compiled.sufficiency.as_ref().expect("cert").certificate,
            &compiled.context.included_ids
        ));
        if !compiled
            .context
            .included_ids
            .iter()
            .any(|id| id == "ev_callers")
        {
            assert!(
                !compiled.context.content.contains("ev_callers"),
                "settled WX-MAP still names the evicted citation:\n{}",
                compiled.context.content
            );
        }
    }

    fn eviction_bundle() -> EvidenceBundle {
        EvidenceBundle {
            repository: "repo".to_owned(),
            evidence: vec![
                fragment("ev_pad", EvidenceKind::SearchHits, &"n".repeat(1_200)),
                fragment(
                    "ev_callers",
                    EvidenceKind::Dependents,
                    "alpha is called from beta",
                ),
            ],
            warnings: Vec::new(),
            snapshot_id: Some("git:abc+dirty:0".to_owned()),
        }
    }

    #[test]
    fn a_layer_eviction_does_not_leave_a_stale_satisfied_citation() {
        let bundle = eviction_bundle();
        let task = "Who calls `alpha` and what breaks if it changes?";
        let compiled = compile_certified_bundle(
            bundle,
            task,
            Some("alpha"),
            420,
            None,
            PlanHints::default(),
            false,
            false,
        )
        .expect("compile");
        let included = &compiled.context.included_ids;
        let certificate = &compiled
            .sufficiency
            .as_ref()
            .expect("certificate")
            .certificate;
        assert!(
            !map_cites_omitted(certificate, included),
            "certificate still names an omitted id: {certificate:?} in {included:?}"
        );
        if !included.iter().any(|id| id == "ev_callers") {
            assert!(
                compiled.context.content.contains("MISSING"),
                "WX-MAP must not keep a satisfied callers line after eviction:\n{}",
                compiled.context.content
            );
            assert!(!compiled.context.content.contains("ev_callers"));
        }
    }

    #[test]
    fn a_wide_budget_keeps_the_map_and_the_citation() {
        let callers = fragment(
            "ev_callers",
            EvidenceKind::Dependents,
            "alpha is called from beta",
        );
        let bundle = EvidenceBundle {
            repository: "repo".to_owned(),
            evidence: vec![callers],
            warnings: Vec::new(),
            snapshot_id: Some("git:abc+dirty:0".to_owned()),
        };
        let compiled = compile_certified_bundle(
            bundle,
            "Who calls `alpha`?",
            Some("alpha"),
            4_000,
            None,
            PlanHints::default(),
            false,
            false,
        )
        .expect("compile");
        assert!(compiled.context.included_ids.contains(&"WX-MAP".to_owned()));
        assert!(
            compiled
                .context
                .included_ids
                .contains(&"ev_callers".to_owned())
        );
        assert!(compiled.context.content.contains("ev_callers"));
        assert!(
            compiled
                .sufficiency
                .as_ref()
                .is_some_and(|report| report.certificate.satisfied.contains_key("direct_callers"))
        );
    }
}
