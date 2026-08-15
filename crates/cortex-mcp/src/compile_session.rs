//! Shared Weavatrix compile used by the context profile and `cortex_prepare`.

use std::path::PathBuf;
use std::time::Instant;

use cortex_shadow::{CompressionSnapshot, ShadowEvidence, ShadowTask};
use cortex_store::{UsageOperation, UsageSample};
use cortex_weavatrix::{CompiledEvidenceBundle, PlanHints, assess_compiled};

use crate::{CortexMcpState, plan_hints, record_usage};

pub(crate) struct CompileArgs {
    pub repository: PathBuf,
    pub task: String,
    pub symbol: Option<String>,
    pub max_tokens: u32,
    pub run_id: Option<String>,
    pub skill_id: Option<String>,
    pub targeted: bool,
    pub hints: Option<PlanHints>,
}

pub(crate) fn compile_weavatrix(
    state: &CortexMcpState,
    arguments: &CompileArgs,
) -> Result<CompiledEvidenceBundle, String> {
    let hints = resolve_hints(state, arguments)?;
    let source_followup = hints.source_followup_or(true);
    let prior = crate::context_memory::load_prior(&state.store, arguments.run_id.as_deref());
    let (prepared, retry) = gather(state, arguments, hints, prior);
    let mut bundle = prepared.map_err(|error| error.to_string())?;
    let verification = bundle.clone();
    let shadow_evidence = state.shadow.as_ref().map(|_| {
        bundle
            .evidence
            .iter()
            .map(|fragment| ShadowEvidence {
                id: fragment.id.clone(),
                source: fragment.source.clone(),
                content: fragment.content.clone(),
            })
            .collect::<Vec<_>>()
    });
    let started = Instant::now();
    let mut semantic_note = None;
    let relevance = score(state, arguments, &mut bundle, &mut semantic_note);
    let mut packet = cortex_weavatrix::compile_evidence_bundle(
        bundle,
        &arguments.task,
        arguments.max_tokens,
        relevance.as_ref(),
    )
    .map_err(|error| error.to_string())?;
    packet.semantic_ranking = semantic_note;
    apply_sufficiency(
        &mut packet,
        &verification,
        arguments,
        hints,
        source_followup,
        retry,
    );
    record_compile(state, arguments, &packet, started);
    observe_shadow(state, arguments, shadow_evidence, &packet);
    Ok(packet)
}

fn resolve_hints(state: &CortexMcpState, arguments: &CompileArgs) -> Result<PlanHints, String> {
    if let Some(hints) = arguments.hints {
        return Ok(hints);
    }
    match arguments.skill_id.as_deref() {
        Some(skill_id) => match state.store.get(skill_id) {
            Ok(Some(graph)) => plan_hints::from_graph(&graph),
            Ok(None) => Err(format!("active skill not found: {skill_id}")),
            Err(error) => Err(error.to_string()),
        },
        None => Ok(PlanHints::default()),
    }
}

fn gather(
    state: &CortexMcpState,
    arguments: &CompileArgs,
    hints: PlanHints,
    prior: cortex_weavatrix::PriorRunMemory,
) -> (
    Result<cortex_weavatrix::EvidenceBundle, cortex_weavatrix::WeavatrixError>,
    Option<cortex_weavatrix::EvidenceSufficiency>,
) {
    if arguments.targeted {
        match state
            .weavatrix
            .prepare_verified_targeted_context_with_prior(
                &arguments.repository,
                &arguments.task,
                arguments.symbol.as_deref(),
                arguments.max_tokens,
                cortex_weavatrix::plan::PlanPolicy::default(),
                hints,
                Some(prior),
            ) {
            Ok((bundle, report)) => (Ok(bundle), Some(report)),
            Err(error) => (Err(error), None),
        }
    } else {
        (
            state.weavatrix.prepare_context(
                &arguments.repository,
                &arguments.task,
                arguments.symbol.as_deref(),
            ),
            None,
        )
    }
}

fn score(
    state: &CortexMcpState,
    arguments: &CompileArgs,
    bundle: &mut cortex_weavatrix::EvidenceBundle,
    semantic_note: &mut Option<String>,
) -> Option<std::collections::HashMap<String, f64>> {
    state.semantic.as_ref().and_then(|scorer| {
        let fragments: Vec<(String, String)> = bundle
            .evidence
            .iter()
            .map(|fragment| (fragment.id.clone(), fragment.content.clone()))
            .collect();
        match scorer.score(&arguments.task, &fragments) {
            Ok(scores) => {
                *semantic_note = Some(scorer.provenance());
                Some(scores)
            }
            Err(error) => {
                bundle.warnings.push(format!(
                    "semantic ordering unavailable: {error}; deterministic order used"
                ));
                None
            }
        }
    })
}

fn apply_sufficiency(
    packet: &mut CompiledEvidenceBundle,
    verification: &cortex_weavatrix::EvidenceBundle,
    arguments: &CompileArgs,
    hints: PlanHints,
    source_followup: bool,
    retry: Option<cortex_weavatrix::EvidenceSufficiency>,
) {
    let Some(gather_report) = retry else {
        return;
    };
    let final_report = assess_compiled(
        verification,
        &packet.context.included_ids,
        &arguments.task,
        arguments.symbol.as_deref(),
        hints,
        source_followup,
        gather_report.retry_performed,
    );
    if !final_report.sufficient {
        packet.context.requires_upstream = true;
        packet.warnings.push(format!(
            "evidence remains thin after verification: {}",
            final_report.missing_evidence.join(", ")
        ));
    }
    packet.sufficiency = Some(final_report);
}

fn record_compile(
    state: &CortexMcpState,
    arguments: &CompileArgs,
    packet: &CompiledEvidenceBundle,
    started: Instant,
) {
    record_usage(
        &state.store,
        &UsageSample {
            operation: UsageOperation::ContextCompile,
            run_id: arguments.run_id.clone(),
            target: None,
            model_tier: None,
            task_class: None,
            budget_tokens: Some(arguments.max_tokens),
            raw_tokens: Some(packet.context.raw_estimated_tokens),
            selected_tokens: Some(packet.context.selected_estimated_tokens),
            omitted_tokens: Some(packet.context.omitted_estimated_tokens),
            requires_upstream: Some(packet.context.requires_upstream),
            latency_ms: Some(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
            token_accounting: packet
                .context
                .token_breakdown
                .as_ref()
                .and_then(|breakdown| serde_json::to_string(breakdown).ok()),
        },
    );
}

fn observe_shadow(
    state: &CortexMcpState,
    arguments: &CompileArgs,
    shadow_evidence: Option<Vec<ShadowEvidence>>,
    packet: &CompiledEvidenceBundle,
) {
    if let (Some(shadow), Some(evidence)) = (&state.shadow, shadow_evidence) {
        shadow.observe(ShadowTask::ContextCompression {
            task: arguments.task.clone(),
            evidence,
            deterministic: CompressionSnapshot {
                included_ids: packet.context.included_ids.clone(),
                omitted_ids: packet.context.omitted_ids.clone(),
                selected_estimated_tokens: packet.context.selected_estimated_tokens,
                requires_upstream: packet.context.requires_upstream,
            },
        });
    }
}
