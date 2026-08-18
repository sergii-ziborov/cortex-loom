//! Agent-profile façade: two tools instead of the 27-tool admin surface.

use std::path::PathBuf;
use std::sync::Arc;

use cortex_router::{RoutingRequest, classify, route};
use cortex_sequences::candidate_templates;
use cortex_weavatrix::{
    BudgetPin, IntentHint, PlanHints, adaptive_budget, plan::extract_identifiers,
};
use mcport::{ConcurrentMcpServer, ToolReply};
use serde::Deserialize;
use serde_json::json;

use crate::CortexMcpState;
use crate::compile_session::{CompileArgs, compile_weavatrix};
use crate::packet_store::{StoredPacket, certificate_hash, packet_stale, task_hash};
use cortex_weavatrix::repository_snapshot;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepareArgs {
    repository: PathBuf,
    task: String,
    run_id: Option<String>,
    budget_class: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpandArgs {
    packet_id: String,
    facet: String,
}

pub(crate) fn register(
    server: ConcurrentMcpServer,
    state: &Arc<CortexMcpState>,
) -> ConcurrentMcpServer {
    let prepare_state = Arc::clone(state);
    let expand_state = Arc::clone(state);
    server
        .typed_tool(
            "cortex_prepare",
            "Route the task and compile a bounded evidence packet. The caller names repository, task, optional runId, and budgetClass only — mutation, verification, and availability are derived, never self-declared.",
            json!({
                "type": "object",
                "properties": {
                    "repository": {"type": "string", "maxLength": 4096},
                    "task": {"type": "string", "maxLength": 16384},
                    "runId": {"type": "string", "maxLength": 256},
                    "budgetClass": {"type": "string", "enum": ["auto", "tight", "normal", "wide"], "default": "auto"}
                },
                "required": ["repository", "task"],
                "additionalProperties": false
            }),
            move |context, arguments: PrepareArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                prepare(&prepare_state, arguments)
            },
        )
        .typed_tool(
            "cortex_expand",
            "Fetch one missing facet of a packet returned by cortex_prepare. The packetId is revision-bound. If the tree moved, the tool refuses and the caller must cortex_prepare again.",
            json!({
                "type": "object",
                "properties": {
                    "packetId": {"type": "string", "maxLength": 64},
                    "facet": {
                        "type": "string",
                        "enum": ["complete_definition", "callers", "public_api_effect", "tests", "git_history", "source"]
                    }
                },
                "required": ["packetId", "facet"],
                "additionalProperties": false
            }),
            move |context, arguments: ExpandArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                expand(&expand_state, &arguments)
            },
        )
}

fn prepare(state: &CortexMcpState, arguments: PrepareArgs) -> ToolReply {
    let pin = BudgetPin::parse(arguments.budget_class.as_deref());
    let max_tokens = adaptive_budget(&arguments.task, pin);
    let symbols = extract_identifiers(&arguments.task);
    let routing = route(&RoutingRequest::new(arguments.task.clone()));
    let classification = classify(&arguments.task);
    let workflow = candidate_templates(&arguments.task)
        .into_iter()
        .next()
        .map(|candidate| {
            json!({
                "sequenceId": candidate.template_id,
                "matchedHints": candidate.matched_hints,
            })
        });
    let compiled = match compile_weavatrix(
        state,
        &CompileArgs {
            repository: arguments.repository.clone(),
            task: arguments.task.clone(),
            symbol: symbols.first().cloned(),
            max_tokens,
            run_id: arguments.run_id.clone(),
            skill_id: None,
            targeted: true,
            hints: None,
        },
    ) {
        Ok(packet) => packet,
        Err(error) => return ToolReply::error(error),
    };
    let missing = compiled
        .sufficiency
        .as_ref()
        .map(|report| report.missing_evidence.clone())
        .unwrap_or_default();
    let Some(id) = compiled.context.packet_id.clone() else {
        return ToolReply::error("compiled packet has no packetId");
    };
    let snapshot = compiled.context.snapshot_id.clone();
    state.packets.insert(StoredPacket {
        id: id.clone(),
        repository: arguments.repository,
        task: arguments.task.clone(),
        task_hash: task_hash(&arguments.task),
        run_id: arguments.run_id,
        symbols,
        snapshot_id: snapshot.clone(),
        certificate_hash: compiled
            .sufficiency
            .as_ref()
            .map(|report| certificate_hash(&report.certificate)),
        max_tokens,
    });
    let handles: Vec<_> = missing
        .iter()
        .map(|facet| json!({ "facet": facet_name(facet) }))
        .collect();
    ToolReply::text(json!({
        "packetId": id,
        "snapshotId": snapshot,
        "taskHash": task_hash(&arguments.task),
        "certificateHash": compiled
            .sufficiency
            .as_ref()
            .map(|report| certificate_hash(&report.certificate)),
        "routing": routing,
        "mutationLikely": classification.mutation_likely,
        "workflowStep": workflow,
        "budgetClass": pin.as_str(),
        "maxTokens": max_tokens,
        "context": compiled.context,
        "coverage": compiled.sufficiency,
        "certificate": compiled.sufficiency.as_ref().map(|report| &report.certificate),
        "missingFacets": missing,
        "expansionHandles": handles,
        "warnings": compiled.warnings,
    }))
}

fn expand(state: &CortexMcpState, arguments: &ExpandArgs) -> ToolReply {
    let Some(stored) = state.packets.get(&arguments.packet_id) else {
        return ToolReply::error(format!("unknown packetId: {}", arguments.packet_id));
    };
    let current_snapshot = repository_snapshot(&stored.repository);
    if let Some(blocked) = packet_stale(&stored, &current_snapshot) {
        return ToolReply::error(blocked.to_string());
    }
    let (task, hints) = facet_request(&stored.task, &stored.symbols, &arguments.facet);
    match compile_weavatrix(
        state,
        &CompileArgs {
            repository: stored.repository.clone(),
            task,
            symbol: stored.symbols.first().cloned(),
            max_tokens: stored.max_tokens,
            run_id: stored.run_id.clone(),
            skill_id: None,
            targeted: true,
            hints: Some(hints),
        },
    ) {
        Ok(packet) => ToolReply::text(json!({
            "packetId": stored.id,
            "stale": false,
            "snapshotId": packet.context.snapshot_id,
            "taskHash": stored.task_hash,
            "certificateHash": stored.certificate_hash,
            "facet": arguments.facet,
            "context": packet.context,
            "coverage": packet.sufficiency,
            "certificate": packet.sufficiency.as_ref().map(|report| &report.certificate),
            "warnings": packet.warnings,
        })),
        Err(error) => ToolReply::error(error),
    }
}

fn facet_name(missing: &str) -> &str {
    if missing.contains("definition") || missing.contains("symbol") {
        "complete_definition"
    } else if missing.contains("dependent") || missing.contains("caller") {
        "callers"
    } else if missing.contains("endpoint") || missing.contains("public_api") {
        "public_api_effect"
    } else if missing.contains("test") {
        "tests"
    } else if missing.contains("git") || missing.contains("history") {
        "git_history"
    } else {
        "source"
    }
}

fn facet_request(task: &str, symbols: &[String], facet: &str) -> (String, PlanHints) {
    let named = symbols.first().map_or(task, String::as_str);
    match facet {
        "complete_definition" => (
            format!("complete definition of {named}. {task}"),
            PlanHints {
                intent: Some(IntentHint::IdentifierChange),
                ..PlanHints::default()
            },
        ),
        "callers" => (
            format!("who calls {named} and what breaks if it changes. {task}"),
            PlanHints {
                intent: Some(IntentHint::BlastRadius),
                ..PlanHints::default()
            },
        ),
        "public_api_effect" => (
            format!("what public API or HTTP contract {named} participates in. {task}"),
            PlanHints {
                intent: Some(IntentHint::ApiContract),
                ..PlanHints::default()
            },
        ),
        "tests" => (
            format!("which tests should run for {named}. {task}"),
            PlanHints {
                intent: Some(IntentHint::TestSelection),
                ..PlanHints::default()
            },
        ),
        "git_history" => (
            format!("git history and blame for {named}. {task}"),
            PlanHints {
                intent: Some(IntentHint::GitHistory),
                ..PlanHints::default()
            },
        ),
        _ => (task.to_owned(), PlanHints::default()),
    }
}
