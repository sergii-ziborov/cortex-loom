use super::{
    Arc, CompressionSnapshot, ConcurrentMcpServer, ContextRequest, CortexMcpState, PlanHints,
    ShadowEvidence, ShadowTask, ToolReply, UsageOperation, UsageSample, WeavatrixContextArgs,
    assess_compiled, compile_context, compile_evidence_bundle, json, plan_hints, record_usage,
};

#[allow(clippy::too_many_lines)]
pub(crate) fn register(
    server: ConcurrentMcpServer,
    state: &Arc<CortexMcpState>,
) -> ConcurrentMcpServer {
    let context_state = Arc::clone(state);
    server
        .typed_tool(
            "context_compile",
            "Select bounded evidence deterministically and report omitted IDs and token savings.",
            json!({
                "type": "object",
                "properties": {
                    "items": {
                    "type": "array",
                    "maxItems": 4096,
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string", "description": "Stable citation ID; keep it in derived output."},
                            "source": {"type": "string", "description": "Where the evidence came from, e.g. src/lib.rs:120."},
                            "content": {"type": "string"},
                            "priority": {"type": "string", "enum": ["critical", "high", "normal", "low"]},
                            "state": {"type": "string", "enum": ["verified", "unverified", "contradictory"]},
                            "relevance": {"type": "number", "description": "Optional score; reorders only within a priority band."}
                        },
                        "required": ["id", "source", "content", "priority", "state"],
                        "additionalProperties": false
                    }
                },
                    "maxTokens": {"type": "integer", "minimum": 1}
                },
                "required": ["items", "maxTokens"],
                "additionalProperties": false
            }),
            move |context, arguments: ContextRequest| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                match compile_context(&arguments) {
                    Ok(packet) => ToolReply::text(packet),
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
        .typed_tool(
            "weavatrix_context_compile",
            "Plan Weavatrix operations from the task, then compile a cortex-source packet: complete named definitions, bounded read_source on search hits and callee files, one sufficiency retry, then a budgeted compile with stable citation IDs. Name the symbols, files, and constants you care about in `task`. Leave `targeted` unset: `targeted=false` is a retired fixed operation set that is worse on tokens and recall.",
            json!({
                "type": "object",
                "properties": {
                    "repository": {"type": "string", "maxLength": 4096},
                    "task": {"type": "string", "maxLength": 16384},
                    "symbol": {"type": "string", "maxLength": 4096},
                    "maxTokens": {"type": "integer", "minimum": 1, "maximum": 100_000},
                    "runId": {"type": "string", "maxLength": 256},
                    "skillId": {"type": "string", "maxLength": 256, "description": "Optional active skill graph whose context-intent, source-followup, and skip-change-plan frontmatter guide evidence gathering."},
                    "targeted": {"type": "boolean", "default": true}
                },
                "required": ["repository", "task", "maxTokens"],
                "additionalProperties": false
            }),
            move |context, arguments: WeavatrixContextArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let hints = match arguments.skill_id.as_deref() {
                    Some(skill_id) => match context_state.store.get(skill_id) {
                        Ok(Some(graph)) => match plan_hints::from_graph(&graph) {
                            Ok(hints) => hints,
                            Err(error) => return ToolReply::error(error),
                        },
                        Ok(None) => {
                            return ToolReply::error(format!("active skill not found: {skill_id}"));
                        }
                        Err(error) => return ToolReply::error(error.to_string()),
                    },
                    None => PlanHints::default(),
                };
                let source_followup = hints.source_followup_or(true);
                let (prepared, gather_report) = if arguments.targeted {
                    match context_state.weavatrix.prepare_verified_targeted_context(
                        &arguments.repository,
                        &arguments.task,
                        arguments.symbol.as_deref(),
                        arguments.max_tokens,
                        cortex_weavatrix::plan::PlanPolicy::default(),
                        hints,
                    ) {
                        Ok((bundle, report)) => (Ok(bundle), Some(report)),
                        Err(error) => (Err(error), None),
                    }
                } else {
                    (
                        context_state.weavatrix.prepare_context(
                            &arguments.repository,
                            &arguments.task,
                            arguments.symbol.as_deref(),
                        ),
                        None,
                    )
                };
                let mut bundle = match prepared {
                    Ok(bundle) => bundle,
                    Err(error) => return ToolReply::error(error.to_string()),
                };
                let verification_bundle = bundle.clone();
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                // Copied only when shadow observation is active; the shadow
                // runner never touches the deterministic reply below.
                let shadow_evidence = context_state.shadow.as_ref().map(|_| {
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
                let started = std::time::Instant::now();
                // Gated semantic ordering: on any failure the packet keeps
                // the deterministic order and records why.
                let mut semantic_note = None;
                let relevance = context_state.semantic.as_ref().and_then(|scorer| {
                    let fragments: Vec<(String, String)> = bundle
                        .evidence
                        .iter()
                        .map(|fragment| (fragment.id.clone(), fragment.content.clone()))
                        .collect();
                    match scorer.score(&arguments.task, &fragments) {
                        Ok(scores) => {
                            semantic_note = Some(scorer.provenance());
                            Some(scores)
                        }
                        Err(error) => {
                            bundle.warnings.push(format!(
                                "semantic ordering unavailable: {error}; deterministic order used"
                            ));
                            None
                        }
                    }
                });
                match compile_evidence_bundle(
                    bundle,
                    &arguments.task,
                    arguments.max_tokens,
                    relevance.as_ref(),
                ) {
                    Ok(mut packet) => {
                        packet.semantic_ranking = semantic_note;
                        if let Some(gather_report) = gather_report {
                            let final_report = assess_compiled(
                                &verification_bundle,
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
                        record_usage(
                            &context_state.store,
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
                                latency_ms: Some(
                                    u64::try_from(started.elapsed().as_millis())
                                        .unwrap_or(u64::MAX),
                                ),
                            },
                        );
                        if let (Some(shadow), Some(evidence)) =
                            (&context_state.shadow, shadow_evidence)
                        {
                            shadow.observe(ShadowTask::ContextCompression {
                                task: arguments.task.clone(),
                                evidence,
                                deterministic: CompressionSnapshot {
                                    included_ids: packet.context.included_ids.clone(),
                                    omitted_ids: packet.context.omitted_ids.clone(),
                                    selected_estimated_tokens: packet
                                        .context
                                        .selected_estimated_tokens,
                                    requires_upstream: packet.context.requires_upstream,
                                },
                            });
                        }
                        ToolReply::text(packet)
                    }
                    Err(error) => ToolReply::error(error.to_string()),
                }
            },
        )
}
