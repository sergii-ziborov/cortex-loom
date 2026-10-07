//! Advisory code previews. Exact source anchors are checked before an edit
//! suggestion reaches the upstream agent; this module never writes files.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use cortex_context::{ChangePlan, ChangePlanStatus, EvidenceItem, SourceTarget};
use cortex_llm::{CodingDraftRequest, TokenUsage};
use cortex_router::{RiskLevel, classify};
use cortex_weavatrix::{CompiledEvidenceBundle, repository_snapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::agent_tools::AgentPrepare;
use crate::CortexMcpState;
use crate::llm_route::{CodingModelResponse, LlmBackend, LlmRouteConfig, LlmRouter, RoutedWork};

const MAX_EXCERPT_CHARS: usize = 4_000;
const MAX_INPUT_CHARS: usize = 17_000;
const MAX_TASK_CHARS: usize = 3_000;
const MAX_DRAFT_TOKENS: u32 = 1_024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EditDraft {
    edits: Vec<EditSuggestion>,
}

#[derive(Debug)]
struct ValidatedDraft {
    edits: Vec<EditSuggestion>,
    discarded: usize,
    duplicates: usize,
    first_discard_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EditSuggestion {
    evidence_id: String,
    find: String,
    replace: String,
    rationale: String,
}

struct PromptSource<'a> {
    target: &'a SourceTarget,
    excerpt: String,
}

pub(super) fn draft_for_prepare(
    state: &CortexMcpState,
    arguments: &AgentPrepare,
    routed: &RoutedWork,
    compiled: &CompiledEvidenceBundle,
    plan: &ChangePlan,
) -> Value {
    let mode = routed.backend;
    if mode == LlmBackend::Off {
        return skipped(mode, "backend_off");
    }
    if !classify(&arguments.task).mutation_likely {
        return skipped(mode, "not_a_code_change");
    }
    if routed.decision.risk >= RiskLevel::High {
        return skipped(mode, "high_risk_upstream");
    }
    if plan.status != ChangePlanStatus::ReadyForUpstreamReview {
        return skipped(mode, "source_or_coverage_incomplete");
    }
    if arguments.task.chars().count() > MAX_TASK_CHARS {
        return skipped(mode, "task_exceeds_draft_budget");
    }
    let Some(snapshot) = compiled.context.snapshot_id.as_deref() else {
        return skipped(mode, "snapshot_missing");
    };
    if repository_snapshot(&arguments.repository) != snapshot {
        return skipped(mode, "snapshot_changed_before_draft");
    }
    let sources = prompt_sources(plan, &compiled.selected_evidence);
    if sources.is_empty() {
        return skipped(mode, "verified_source_missing");
    }
    let request = CodingDraftRequest {
        task: arguments.task.clone(),
        verified_source: render_sources(&sources),
        max_output_tokens: MAX_DRAFT_TOKENS,
    };
    let response = match ask_model(state, arguments, mode, &request) {
        Ok(response) => response,
        Err((called, error)) => return rejected(mode, called, None, &error),
    };
    if repository_snapshot(&arguments.repository) != snapshot {
        return rejected(mode, true, Some(&response), "snapshot_changed_during_draft");
    }
    let draft = match validate_draft(&response.content, &sources, &arguments.repository) {
        Ok(draft) => draft,
        Err(error) => return rejected(mode, true, Some(&response), &error),
    };
    let usage = response.usage;
    json!({
        "role": "coding_draft",
        "mode": mode.as_str(),
        "status": "accepted_for_upstream_review",
        "called": true,
        "accepted": true,
        "affectsEvidence": false,
        "mutationAuthority": false,
        "profile": response.profile,
        "model": response.model,
        "latencyMs": response.latency_ms,
        "usageKind": usage_kind(usage, true),
        "promptTokens": usage.map(|value| value.prompt_tokens),
        "completionTokens": usage.map(|value| value.completion_tokens),
        "totalTokens": usage.map(TokenUsage::total),
        "summary": format!("{} source-anchored suggestion(s) for upstream review; task completion unverified", draft.edits.len()),
        "discardedSuggestions": draft.discarded,
        "duplicateSuggestions": draft.duplicates,
        "discardReason": draft.first_discard_reason,
        "edits": draft.edits,
    })
}

fn ask_model(
    state: &CortexMcpState,
    arguments: &AgentPrepare,
    mode: LlmBackend,
    request: &CodingDraftRequest,
) -> Result<CodingModelResponse, (bool, String)> {
    let operator_mode = LlmRouteConfig::from_env()
        .resolve_backend()
        .map_err(|error| (false, error))?;
    if mode != operator_mode {
        return Err((
            false,
            "draft backend differs from operator policy".to_owned(),
        ));
    }
    let alias = arguments
        .draft_model
        .as_deref()
        .or(arguments.classifier_model.as_deref());
    if let Some(alias) = alias {
        if mode != LlmBackend::Composer {
            return Err((false, "draftModel requires the composer backend".to_owned()));
        }
        let router =
            crate::composer_llm::router_for_alias(alias).map_err(|error| (false, error))?;
        return router.draft_coding(request).map_err(|error| (true, error));
    }
    let router: &LlmRouter = state
        .llm_router
        .as_deref()
        .ok_or_else(|| (false, "configured model is unavailable".to_owned()))?;
    router.draft_coding(request).map_err(|error| (true, error))
}

fn prompt_sources<'a>(plan: &'a ChangePlan, selected: &'a [EvidenceItem]) -> Vec<PromptSource<'a>> {
    let mut sources = Vec::new();
    let mut total = 0_usize;
    let mut seen = HashSet::new();
    for target in plan.source_targets.iter().chain(plan.test_targets.iter()) {
        if !seen.insert(target.evidence_id.as_str()) {
            continue;
        }
        let Some(item) = selected.iter().find(|item| item.id == target.evidence_id) else {
            continue;
        };
        let excerpt: String = item.content.chars().take(MAX_EXCERPT_CHARS).collect();
        if excerpt.trim().is_empty() {
            continue;
        }
        // Reserve room for the citation header and separators too.
        if total.saturating_add(excerpt.len()).saturating_add(256) > MAX_INPUT_CHARS {
            break;
        }
        total += excerpt.len() + 256;
        sources.push(PromptSource { target, excerpt });
        if sources.len() == 6 {
            break;
        }
    }
    if let Some(primary) = plan.source_targets.first() {
        sources
            .sort_by_key(|source| (source.target.path != primary.path, source.target.start_line));
    }
    sources
}

fn render_sources(sources: &[PromptSource<'_>]) -> String {
    let mut text = String::new();
    for source in sources {
        let block = format!(
            "\n[evidenceId={} path={} lines={}-{}]\n{}\n[/evidence]\n",
            source.target.evidence_id,
            source.target.path,
            source.target.start_line,
            source.target.end_line,
            source.excerpt
        );
        if text.len().saturating_add(block.len()) > MAX_INPUT_CHARS {
            break;
        }
        text.push_str(&block);
    }
    text
}

fn validate_draft(
    content: &str,
    sources: &[PromptSource<'_>],
    repository: &Path,
) -> Result<ValidatedDraft, String> {
    let draft: EditDraft =
        serde_json::from_str(content).map_err(|error| format!("draft schema invalid: {error}"))?;
    if draft.edits.is_empty() || draft.edits.len() > 3 {
        return Err("draft has no bounded edit suggestions".to_owned());
    }
    let root = repository
        .canonicalize()
        .map_err(|error| format!("cannot inspect repository: {error}"))?;
    let mut seen = Vec::<(PathBuf, String, String)>::new();
    let mut accepted = Vec::with_capacity(draft.edits.len());
    let mut discarded = 0;
    let mut duplicates = 0;
    let mut first_discard_reason = None;
    for edit in draft.edits {
        let path = match validate_edit_anchor(&edit, sources, &root) {
            Ok(path) => path,
            Err(error) => {
                discarded += 1;
                first_discard_reason.get_or_insert(error);
                continue;
            }
        };
        if seen.iter().any(|(prior_path, prior_find, prior_replace)| {
            prior_path == &path && prior_find == &edit.find && prior_replace == &edit.replace
        }) {
            duplicates += 1;
            continue;
        }
        if seen.iter().any(|(prior_path, prior_find, _)| {
            prior_path == &path
                && (prior_find.contains(&edit.find) || edit.find.contains(prior_find))
        }) {
            return Err("draft edits overlap".to_owned());
        }
        seen.push((path, edit.find.clone(), edit.replace.clone()));
        accepted.push(edit);
    }
    if accepted.is_empty() {
        return Err(first_discard_reason.unwrap_or_else(|| "draft has no valid edits".to_owned()));
    }
    Ok(ValidatedDraft {
        edits: accepted,
        discarded,
        duplicates,
        first_discard_reason,
    })
}

fn validate_edit_anchor(
    edit: &EditSuggestion,
    sources: &[PromptSource<'_>],
    root: &Path,
) -> Result<PathBuf, String> {
    let source = sources
        .iter()
        .find(|source| source.target.evidence_id == edit.evidence_id)
        .ok_or("draft cites source absent from its prompt")?;
    if edit.find.trim().chars().count() < 12
        || edit.find.chars().count() > 2_000
        || edit.replace.trim().is_empty()
        || edit.replace.chars().count() > 4_000
        || edit.find == edit.replace
        || edit.rationale.trim().is_empty()
        || edit.rationale.chars().count() > 300
    {
        return Err("draft edit exceeds bounds or has no change".to_owned());
    }
    if source.excerpt.matches(&edit.find).count() != 1 {
        return Err("draft find text is not unique in cited source".to_owned());
    }
    let path = root
        .join(&source.target.path)
        .canonicalize()
        .map_err(|error| format!("draft source path unavailable: {error}"))?;
    if !path.starts_with(root) || !path.is_file() {
        return Err("draft source path escapes repository".to_owned());
    }
    let live = fs::read_to_string(&path)
        .map_err(|error| format!("draft source cannot be read: {error}"))?;
    if live.matches(&edit.find).count() != 1 {
        return Err("draft find text does not uniquely match live source".to_owned());
    }
    Ok(path)
}

fn skipped(mode: LlmBackend, reason: &str) -> Value {
    json!({
        "role": "coding_draft", "mode": mode.as_str(), "status": "skipped",
        "called": false, "accepted": false, "affectsEvidence": false,
        "mutationAuthority": false, "usageKind": "not_called", "reason": reason,
    })
}

fn rejected(
    mode: LlmBackend,
    called: bool,
    response: Option<&CodingModelResponse>,
    reason: &str,
) -> Value {
    let usage = response.and_then(|value| value.usage);
    json!({
        "role": "coding_draft", "mode": mode.as_str(), "status": "rejected",
        "called": called, "accepted": false, "affectsEvidence": false,
        "mutationAuthority": false, "usageKind": usage_kind(usage, called),
        "promptTokens": usage.map(|value| value.prompt_tokens),
        "completionTokens": usage.map(|value| value.completion_tokens),
        "totalTokens": usage.map(TokenUsage::total),
        "model": response.and_then(|value| value.model.as_deref()),
        "latencyMs": response.map(|value| value.latency_ms),
        "reason": reason,
    })
}

fn usage_kind(usage: Option<TokenUsage>, called: bool) -> &'static str {
    if usage.is_some() {
        "provider_reported"
    } else if called {
        "unknown"
    } else {
        "not_called"
    }
}

#[cfg(test)]
#[path = "coding_advice_tests.rs"]
mod tests;
