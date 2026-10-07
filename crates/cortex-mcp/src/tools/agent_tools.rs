//! Agent-profile façade: two tools instead of the 27-tool admin surface.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use cortex_context::{build_change_plan, packet_id as context_packet_id};
use cortex_router::{RoutingRequest, classify};
use cortex_sequences::candidate_templates;
use cortex_weavatrix::{
    BudgetPin, IntentHint, PlanHints, adaptive_budget, graph_seed, is_graph_symbol,
    plan::extract_identifiers,
};
use mcport::{ConcurrentMcpServer, ToolReply};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::CortexMcpState;
use crate::compile_session::{CompileArgs, compile_weavatrix};
use crate::packet_store::{StoredPacket, certificate_hash, packet_stale, task_hash};
use cortex_weavatrix::repository_snapshot;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPrepare {
    pub repository: PathBuf,
    pub task: String,
    pub run_id: Option<String>,
    pub budget_class: Option<String>,
    /// Exact compiler body budget. Separate from the adaptive budget class.
    pub max_tokens: Option<u32>,
    /// Loopback classifier alias for this call only: composer, sonnet-5, opus-5, haiku.
    pub classifier_model: Option<String>,
    /// Optional Composer alias for the advisory coding draft. Defaults to
    /// classifierModel, then the operator's configured model.
    pub draft_model: Option<String>,
    /// `plain` keeps source code unescaped in one MCP text block. `json`
    /// retains the machine-readable response for existing clients.
    pub response_format: Option<String>,
}

/// Disk-restored packet so CLI expand can run in a new process.
#[derive(Debug, Clone)]
pub struct AgentPacket {
    pub id: String,
    pub repository: PathBuf,
    pub task: String,
    pub run_id: Option<String>,
    pub task_hash: String,
    pub symbols: Vec<String>,
    pub snapshot_id: Option<String>,
    pub certificate_hash: Option<String>,
    pub max_tokens: u32,
}

type PrepareArgs = AgentPrepare;

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
            "Classify and compile a bounded evidence packet. Optional budgetClass selects an adaptive band; maxTokens is an exact compiler-body limit. Routing is advisory, not execution permission.",
            json!({
                "type": "object",
                "properties": {
                    "repository": {"type": "string", "maxLength": 4096},
                    "task": {"type": "string", "maxLength": 16384},
                    "runId": {"type": "string", "maxLength": 256},
                    "budgetClass": {"type": "string", "enum": ["auto", "tight", "normal", "wide"], "default": "auto"},
                    "maxTokens": {"type": "integer", "minimum": 1, "maximum": 100_000},
                    "classifierModel": {
                        "type": "string",
                        "enum": ["composer", "sonnet-5", "opus-5", "haiku"],
                        "description": "Loopback classifier via the cursor-agent proxy. Overrides CORTEX_CLASSIFIER_MODEL for this prepare only."
                    },
                    "draftModel": {
                        "type": "string",
                        "enum": ["composer", "sonnet-5", "opus-5", "haiku"],
                        "description": "Composer model for a source-anchored advisory coding draft. The operator backend must be composer."
                    },
                    "responseFormat": {
                        "type": "string",
                        "enum": ["json", "plain"],
                        "default": "json",
                        "description": "Use plain for coding agents: metadata stays JSON, but exact source appears as readable text without JSON escaping."
                    }
                },
                "required": ["repository", "task"],
                "additionalProperties": false
            }),
            move |context, arguments: PrepareArgs| {
                if context.is_cancelled() {
                    return ToolReply::error("cancelled");
                }
                let plain = arguments.response_format.as_deref() == Some("plain");
                match prepare_packet(&prepare_state, &arguments) {
                    Ok(value) if plain => match super::agent_reply::plain_packet(&value) {
                        Ok(text) => ToolReply::literal_text(text),
                        Err(error) => ToolReply::error(error),
                    },
                    Ok(value) => ToolReply::text(value),
                    Err(error) => ToolReply::error(error),
                }
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
                match expand_packet(&expand_state, &arguments.packet_id, &arguments.facet) {
                    Ok(value) => ToolReply::text(value),
                    Err(error) => ToolReply::error(error),
                }
            },
        )
}

#[allow(clippy::too_many_lines)] // One admission/compile/store transaction owns the final packet.
pub fn prepare_packet(state: &CortexMcpState, arguments: &AgentPrepare) -> Result<Value, String> {
    let started = Instant::now();
    if !matches!(
        arguments.response_format.as_deref(),
        None | Some("json" | "plain")
    ) {
        return Err("responseFormat must be json or plain".to_owned());
    }
    // Admission must precede classifier inference: task text can itself be private.
    state.workspaces.check(&arguments.repository)?;
    if arguments.task.trim().is_empty() || arguments.task.chars().count() > 16_384 {
        return Err("task must contain 1..=16384 characters".to_owned());
    }
    let (pin, max_tokens) = prepare_budget(arguments)?;
    let symbols = extract_identifiers(&arguments.task);
    let request = RoutingRequest::new(arguments.task.clone());
    let routed =
        super::agent_route::route_prepare(state, &request, arguments.classifier_model.as_deref());
    let routing = routed.decision.clone();
    let classification = classify(&arguments.task);
    let workflow = workflow_hint(&arguments.task);
    let mut compiled = compile_weavatrix(
        state,
        &CompileArgs {
            repository: arguments.repository.clone(),
            task: arguments.task.clone(),
            symbol: compile_symbol(&symbols, &arguments.task),
            max_tokens,
            run_id: arguments.run_id.clone(),
            skill_id: None,
            targeted: true,
            hints: None,
        },
    )?;
    let task_digest = task_hash(&arguments.task);
    let base_id = compiled
        .context
        .packet_id
        .as_deref()
        .ok_or("compiled packet has no packetId")?;
    let id = context_packet_id(&[
        base_id,
        &task_digest,
        arguments.run_id.as_deref().unwrap_or_default(),
        &max_tokens.to_string(),
    ]);
    compiled.context.packet_id = Some(id.clone());
    if let Some(report) = compiled.sufficiency.as_mut() {
        report.certificate.packet_id = Some(id.clone());
    }
    let missing = compiled
        .sufficiency
        .as_ref()
        .map(|report| report.missing_evidence.clone())
        .unwrap_or_default();
    let snapshot = compiled.context.snapshot_id.clone();
    let change_plan = build_change_plan(
        &arguments.task,
        &compiled.selected_evidence,
        compiled
            .sufficiency
            .as_ref()
            .map(|report| &report.certificate),
        compiled
            .sufficiency
            .as_ref()
            .is_some_and(|report| report.sufficient),
    );
    let coding_draft =
        super::coding_advice::draft_for_prepare(state, arguments, &routed, &compiled, &change_plan);
    state.packets.insert(super::agent_store::stored_packet(
        arguments,
        &compiled,
        &id,
        &task_digest,
        &symbols,
        max_tokens,
    ));
    let handles: Vec<_> = missing
        .iter()
        .map(|facet| json!({ "facet": facet_name(facet) }))
        .collect();
    let warnings =
        super::agent_route::combined_warnings(&compiled.warnings, routed.warning.as_deref());
    Ok(json!({
        "packetId": id,
        "repository": arguments.repository,
        "symbols": symbols,
        "snapshotId": snapshot,
        "taskHash": task_digest,
        "certificateHash": compiled
            .sufficiency
            .as_ref()
            .map(|report| certificate_hash(&report.certificate)),
        "routing": routing,
        "executionAdmission": {
            "ready": false,
            "reason": if compiled.sufficiency.as_ref().is_some_and(|report| !report.sufficient) {
                "insufficient_evidence"
            } else {
                "external_executor_unverified"
            },
            "localAvailability": "unverified",
        },
        "mutationLikely": classification.mutation_likely,
        "workflowStep": workflow,
        "budgetClass": if arguments.max_tokens.is_some() { "exact" } else { pin.as_str() },
        "maxTokens": max_tokens,
        "context": compiled.context,
        "coverage": compiled.sufficiency,
        "changePlan": change_plan,
        "codingDraft": coding_draft,
        "missingFacets": missing,
        "expansionHandles": handles,
        "warnings": warnings,
        "internalModel": routed.internal_model_json(),
        "prepareLatencyMs": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    }))
}

fn prepare_budget(arguments: &AgentPrepare) -> Result<(BudgetPin, u32), String> {
    let pin = BudgetPin::parse(arguments.budget_class.as_deref());
    let max_tokens = match arguments.max_tokens {
        Some(value @ 1..=100_000) => value,
        Some(_) => return Err("maxTokens must be in 1..=100000".to_owned()),
        None => adaptive_budget(&arguments.task, pin),
    };
    Ok((pin, max_tokens))
}

fn workflow_hint(task: &str) -> Option<Value> {
    candidate_templates(task)
        .into_iter()
        .next()
        .map(|candidate| {
            json!({
                "sequenceId": candidate.template_id,
                "active": false,
                "matchedHints": candidate.matched_hints,
            })
        })
}

pub fn expand_saved(
    state: &CortexMcpState,
    packet: AgentPacket,
    facet: &str,
) -> Result<Value, String> {
    let id = packet.id.clone();
    state.packets.insert(StoredPacket {
        id: id.clone(),
        repository: packet.repository,
        task: packet.task,
        task_hash: packet.task_hash,
        run_id: packet.run_id,
        symbols: packet.symbols,
        snapshot_id: packet.snapshot_id,
        certificate_hash: packet.certificate_hash,
        max_tokens: packet.max_tokens,
    });
    expand_packet(state, &id, facet)
}

pub fn expand_packet(
    state: &CortexMcpState,
    packet_id: &str,
    facet: &str,
) -> Result<Value, String> {
    let Some(stored) = state.packets.get(packet_id) else {
        return Err(format!(
            "unknown packetId: {packet_id}. Run cortex-loom prepare first, or cortex-loom report --last."
        ));
    };
    let current_snapshot = repository_snapshot(&stored.repository);
    if let Some(blocked) = packet_stale(&stored, &current_snapshot) {
        return Err(blocked.to_string());
    }
    let (task, hints) = facet_request(&stored.task, &stored.symbols, facet);
    match compile_weavatrix(
        state,
        &CompileArgs {
            repository: stored.repository.clone(),
            task,
            symbol: compile_symbol(&stored.symbols, &stored.task),
            max_tokens: stored.max_tokens,
            run_id: stored.run_id.clone(),
            skill_id: None,
            targeted: true,
            hints: Some(hints),
        },
    ) {
        Ok(mut packet) => {
            let base_id = packet
                .context
                .packet_id
                .as_deref()
                .ok_or("expanded packet has no packetId")?;
            let child_id = context_packet_id(&[base_id, &stored.id, facet]);
            packet.context.packet_id = Some(child_id.clone());
            if let Some(report) = packet.sufficiency.as_mut() {
                report.certificate.packet_id = Some(child_id.clone());
            }
            Ok(json!({
                "packetId": child_id,
                "parentPacketId": stored.id,
                "stale": false,
                "snapshotId": packet.context.snapshot_id,
                "taskHash": task_hash(&packet.task),
                "parentTaskHash": stored.task_hash,
                "certificateHash": packet.sufficiency.as_ref().map(|report| certificate_hash(&report.certificate)),
                "parentCertificateHash": stored.certificate_hash,
                "facet": facet,
                "context": packet.context,
                "coverage": packet.sufficiency,
                "warnings": packet.warnings,
            }))
        }
        Err(error) => Err(error),
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

fn compile_symbol(symbols: &[String], task: &str) -> Option<String> {
    if cortex_weavatrix::plan::is_new_feature_without_owner(task) {
        return None;
    }
    graph_seed(symbols).map(str::to_owned)
}

fn expand_target<'a>(symbols: &'a [String], task: &'a str) -> &'a str {
    graph_seed(symbols)
        .or_else(|| {
            symbols
                .iter()
                .map(String::as_str)
                .find(|name| !is_graph_symbol(name))
        })
        .or_else(|| symbols.first().map(String::as_str))
        .unwrap_or(task)
}

fn facet_request(task: &str, symbols: &[String], facet: &str) -> (String, PlanHints) {
    let named = expand_target(symbols, task);
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

#[cfg(test)]
mod creation_tests {
    use super::compile_symbol;

    #[test]
    fn a_new_feature_does_not_demand_a_tool_label_definition() {
        let labels = vec!["context_compile".to_owned()];
        assert_eq!(
            compile_symbol(
                &labels,
                "Add Elevated priority and update MCP context_compile schema"
            ),
            None
        );
        let owner = vec!["EvidencePriority".to_owned()];
        assert_eq!(
            compile_symbol(&owner, "Add Elevated to EvidencePriority"),
            Some("EvidencePriority".to_owned())
        );
    }
}
