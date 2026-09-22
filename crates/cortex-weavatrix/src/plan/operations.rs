use serde_json::json;

use super::{
    EvidenceKind, MIN_OPERATION_BUDGET, PlanPolicy, PlannedOperation, escape_regex_literal,
    search_pattern,
};
use crate::PriorRunMemory;

/// Query for planned search. Path strings are mentions of the ask
/// (catalogs quote them); they are not the file contents.
#[must_use]
pub fn search_code_query(identifiers: &[String]) -> String {
    let usable: Vec<String> = identifiers
        .iter()
        .filter(|identifier| !path_mention_noise(identifier))
        .cloned()
        .collect();
    if usable.is_empty() {
        return String::from(r"(?:fn|struct|enum|trait|impl|class|def|func)\s+\w+");
    }
    search_pattern(&usable)
}

fn path_mention_noise(identifier: &str) -> bool {
    crate::fold::is_repo_source_path(identifier)
        || crate::fold::SOURCE_SUFFIXES
            .iter()
            .any(|suffix| crate::fold::fold_text(identifier).ends_with(suffix))
}

pub(super) fn search_op(
    id: &'static str,
    identifiers: &[String],
    search_budget: u32,
    policy: PlanPolicy,
    glob: &str,
) -> PlannedOperation {
    search_pattern_op(
        id,
        &search_code_query(identifiers),
        search_budget,
        policy,
        glob,
    )
}

pub(super) fn search_pattern_op(
    id: &'static str,
    query: &str,
    search_budget: u32,
    policy: PlanPolicy,
    glob: &str,
) -> PlannedOperation {
    PlannedOperation {
        id,
        tool: "search_code",
        kind: EvidenceKind::SearchHits,
        arguments: json!({
            "query": query,
            "is_regex": true,
            "before": 1,
            "after": 1,
            "max_results": 40,
            "glob": glob,
            "token_budget": search_budget,
        }),
        expected_tokens: policy.search_tokens.max(search_budget),
        bounded: true,
    }
}

pub(super) fn blast_search_pattern(symbol: &str) -> String {
    let symbol = escape_regex_literal(symbol);
    format!(
        r"\b(fn|struct|enum|trait|type|class|interface|function|def|func|record)\s+{symbol}\b|,\s*{symbol}\s*\)|[:=]\s*{symbol}\s*\(|(return|match)\s+{symbol}\s*\("
    )
}

/// Call-site search for every named identifier, not just the seed symbol.
///
/// A "who calls A versus B" question is blast-radius. Searching only `A`
/// misses the `match B(` caller (measured: compile-bundle 2/4 after the
/// `compile_probe_bundle` split).
pub(super) fn blast_search_query(symbol: Option<&str>, identifiers: &[String]) -> String {
    let mut names = Vec::new();
    if let Some(symbol) = symbol {
        names.push(symbol.to_owned());
    }
    for identifier in identifiers {
        if path_mention_noise(identifier) {
            continue;
        }
        if !names.iter().any(|seen| seen == identifier) {
            names.push(identifier.clone());
        }
        if names.len() == super::MAX_IDENTIFIERS {
            break;
        }
    }
    names
        .iter()
        .map(|name| blast_search_pattern(name))
        .collect::<Vec<_>>()
        .join("|")
}

pub(super) fn asks_for_change_plan(task: &str) -> bool {
    const CUES: &[&str] = &[
        "change plan",
        "implementation plan",
        "pre-commit plan",
        "prepare change",
        "plan the change",
        "plan this change",
        "план изменений",
        "план зміни",
    ];
    let lower = crate::fold::fold_text(task);
    CUES.iter().any(|cue| lower.contains(cue))
}

pub(super) fn symbol_op(symbol: &str, structural: u32, policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-SYMBOL",
        tool: "context_bundle",
        kind: EvidenceKind::SymbolContext,
        arguments: json!({
            "label": symbol,
            "max_related": 30,
            "max_references": 30,
            "max_source_files": 12,
            "token_budget": structural,
        }),
        expected_tokens: policy.symbol_tokens,
        bounded: true,
    }
}

pub(super) fn modules_op(policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-MODULES",
        tool: "module_map",
        kind: EvidenceKind::ModuleMap,
        arguments: json!({"top_n": 16, "include_non_product": false}),
        expected_tokens: policy.modules_tokens,
        bounded: false,
    }
}

/// Occurrence list for one symbol. `get_dependents` can answer with
/// `related` neighbours that never name the function. `find_references`
/// (weavatrix-rust 2.17) lists the definition and each incoming site.
pub(super) fn references_op(symbol: &str, policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-REFS",
        tool: "find_references",
        kind: EvidenceKind::Dependents,
        arguments: json!({
            "label": symbol,
            "max_results": 40,
        }),
        expected_tokens: policy.dependents_tokens.min(1_200),
        bounded: false,
    }
}

pub(super) fn dependents_op(symbol: &str, policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-DEPENDENTS",
        tool: "get_dependents",
        kind: EvidenceKind::Dependents,
        arguments: json!({ "label": symbol }),
        expected_tokens: policy.dependents_tokens,
        bounded: false,
    }
}

/// Reference-relation neighbours of the symbol.
///
/// `get_dependents` walks call edges; measured on `weavatrix-rust` 2.5.1, a
/// struct's *type references* — the `fn default` and builder that mention it
/// without calling it — surface only through `get_neighbors`. A blast-radius
/// question about a struct answered from call edges alone scored 0/2 on the
/// reference ground truth while this call carried both misses in ~1.8 k
/// tokens.
pub(super) fn neighbors_op(symbol: &str, policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-NEIGHBORS",
        tool: "get_neighbors",
        kind: EvidenceKind::Dependents,
        arguments: json!({ "label": symbol }),
        expected_tokens: policy.dependents_tokens.min(2_000),
        bounded: false,
    }
}

pub(super) fn endpoints_op(policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-ENDPOINTS",
        tool: "list_endpoints",
        kind: EvidenceKind::Endpoints,
        arguments: json!({}),
        expected_tokens: policy.endpoints_tokens,
        bounded: false,
    }
}

pub(super) fn git_history_op(policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-GIT",
        tool: "git_history",
        kind: EvidenceKind::GitHistory,
        arguments: json!({
            "max_commits": 24,
            "months": 36,
            "first_parent": true,
            // Analytics (cochange / hotspots) serializes first and, under
            // the 800-token cap, drops the commit summaries a history
            // question is asking for.
            "include_analytics": false,
            "token_budget": policy.git_history_tokens,
        }),
        expected_tokens: policy.git_history_tokens,
        bounded: true,
    }
}

pub(super) fn stacktrace_op(task: &str, policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-STACK",
        tool: "map_stacktrace",
        kind: EvidenceKind::StackTrace,
        arguments: json!({
            "text": task,
            "max_frames": 40,
        }),
        expected_tokens: policy.stacktrace_tokens,
        bounded: false,
    }
}

pub(super) fn memory_op(
    task: &str,
    prior: &PriorRunMemory,
    policy: PlanPolicy,
) -> Option<PlannedOperation> {
    Some(PlannedOperation {
        id: "WX-MEMORY",
        tool: "memory_context",
        kind: EvidenceKind::Memory,
        arguments: prior.memory_arguments(task, policy.memory_tokens)?,
        expected_tokens: policy.memory_tokens,
        bounded: false,
    })
}

pub(super) fn select_tests_op(policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-TESTS",
        tool: "select_tests",
        kind: EvidenceKind::TestSelection,
        arguments: json!({
            "max_tests": 24,
            "depth": 3,
            "max_nodes": 200,
        }),
        expected_tokens: policy.test_selection_tokens,
        bounded: false,
    }
}

pub(super) fn verify_op(task: &str, policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-VERIFY",
        tool: "verified_change",
        kind: EvidenceKind::ChangePlan,
        arguments: json!({
            "task": task,
            "phase": "plan",
            "duplicate_ratchet": true,
            "run_tests": false,
        }),
        expected_tokens: policy.change_plan_tokens,
        bounded: false,
    }
}

/// Live classifier definitions, not a generic `fn|struct` dump or a path mention.
pub(super) const CLASSIFIER_SEARCH: &str =
    r"\bfn\s+\w*classif\w*|\bclassify_\w+|\bnaive_\w*classif\w*";

pub(super) fn identifier_search_ops(
    task: &str,
    symbol: Option<&str>,
    identifiers: &[String],
    search_budget: u32,
    policy: PlanPolicy,
    intent: crate::plan_intent::TaskIntent,
    inventory_glob: Option<&str>,
) -> Vec<PlannedOperation> {
    let glob = crate::fold::search_glob_in(identifiers, inventory_glob);
    let graph_symbol = symbol.filter(|name| crate::fold::is_graph_symbol(name));
    if intent == crate::plan_intent::TaskIntent::RuntimeConfig {
        let slice = share(search_budget, 1, 2);
        return vec![
            search_op("WX-SEARCH", identifiers, slice, policy, glob.as_str()),
            search_op("WX-CONFIG", identifiers, slice, policy, "config/**"),
        ];
    }
    if intent == crate::plan_intent::TaskIntent::BlastRadius {
        let query = blast_search_query(graph_symbol, identifiers);
        if query.is_empty() {
            return Vec::new();
        }
        return vec![search_pattern_op(
            "WX-SEARCH",
            &query,
            search_budget,
            policy,
            glob.as_str(),
        )];
    }
    if crate::plan_intent::asks_for_duplicate_share(task) {
        return vec![search_pattern_op(
            "WX-SEARCH",
            CLASSIFIER_SEARCH,
            search_budget,
            policy,
            glob.as_str(),
        )];
    }
    vec![search_op(
        "WX-SEARCH",
        identifiers,
        search_budget,
        policy,
        glob.as_str(),
    )]
}

/// Ops Weavatrix already has that Cortex used to skip: whole-file reads,
/// unreferenced production symbols, clone families, a historical blob
/// when the task names both a file and a revision, and measured coverage
/// when the question is about a report (ingest only; Quality may have
/// written `.weavatrix/coverage/lcov.info`).
pub(super) fn task_specific_ops(
    task: &str,
    identifiers: &[String],
    policy: PlanPolicy,
) -> Vec<PlannedOperation> {
    let mut extras = Vec::new();
    for path in crate::fold::named_source_files(identifiers) {
        extras.push(named_file_op(&path, policy));
    }
    if let Some(revision) = named_git_revision(task) {
        for path in crate::fold::named_source_files(identifiers) {
            extras.push(git_blob_op(&path, &revision, policy));
        }
    }
    if crate::plan_intent::asks_for_dead_production(task)
        && let Some(scope) = crate::fold::named_crate_scope(identifiers)
    {
        extras.push(dead_code_op(&scope, policy));
    }
    if crate::plan_intent::asks_for_duplicate_share(task) {
        extras.push(duplicates_op(policy));
    }
    if super::asks_for_coverage(task) {
        extras.push(super::coverage::coverage_map_op(
            super::coverage::coverage_path_filter(identifiers).as_deref(),
            policy,
        ));
    }
    extras
}

/// A revision other than worktree `HEAD` — `HEAD` itself is `read_source`.
pub(super) fn named_git_revision(task: &str) -> Option<String> {
    task.split(|character: char| {
        !(character.is_ascii_alphanumeric()
            || character == '~'
            || character == '^'
            || character == '.'
            || character == '_'
            || character == '-')
    })
    .find_map(revision_token)
}

fn revision_token(token: &str) -> Option<String> {
    let upper = token.to_ascii_uppercase();
    if upper == "HEAD" {
        return None;
    }
    if let Some(rest) = upper.strip_prefix("HEAD~")
        && !rest.is_empty()
        && rest.chars().all(|character| character.is_ascii_digit())
    {
        return Some(format!("HEAD~{rest}"));
    }
    if upper.starts_with("HEAD^") && upper.bytes().skip(4).all(|byte| byte == b'^') {
        return Some(upper);
    }
    let hex = (7..=40).contains(&token.len())
        && token.chars().all(|character| character.is_ascii_hexdigit());
    if hex && (token.len() >= 12 || token.chars().any(|character| character.is_ascii_digit())) {
        return Some(token.to_ascii_lowercase());
    }
    None
}

fn git_blob_op(path: &str, revision: &str, policy: PlanPolicy) -> PlannedOperation {
    let token_budget = policy.source_tokens.max(1_200);
    PlannedOperation {
        id: "WX-BLOB",
        tool: "git_read_blob",
        kind: EvidenceKind::SourceReads,
        arguments: json!({
            "path": path,
            "revision": revision,
            "token_budget": token_budget,
        }),
        expected_tokens: token_budget,
        bounded: true,
    }
}

fn named_file_op(path: &str, policy: PlanPolicy) -> PlannedOperation {
    let token_budget = policy.source_tokens.max(2_400);
    PlannedOperation {
        id: "WX-FILE",
        tool: "read_source",
        kind: EvidenceKind::SourceReads,
        arguments: json!({
            "path": path,
            "start_line": 1,
            "before": 0,
            "after": 800,
            "token_budget": token_budget,
        }),
        expected_tokens: token_budget,
        bounded: true,
    }
}

fn dead_code_op(scope: &str, policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-DEAD",
        tool: "find_dead_code",
        kind: EvidenceKind::DeadCode,
        arguments: json!({
            "path": scope,
            "min_confidence": 50,
            "include_tests": false,
            "include_classified": false,
            "top_n": 24,
        }),
        expected_tokens: policy.test_selection_tokens,
        bounded: false,
    }
}

fn duplicates_op(policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-DUP",
        tool: "find_duplicates",
        kind: EvidenceKind::Duplicates,
        arguments: json!({
            "include_tests": false,
            "include_classified": false,
            "top_n": 24,
        }),
        expected_tokens: policy.dependents_tokens.min(1_600),
        bounded: false,
    }
}

pub(super) fn share(budget: u32, numerator: u32, denominator: u32) -> u32 {
    budget
        .saturating_mul(numerator)
        .checked_div(denominator)
        .unwrap_or(MIN_OPERATION_BUDGET)
        .max(MIN_OPERATION_BUDGET)
}

#[cfg(test)]
#[path = "task_ops_tests.rs"]
mod task_ops_tests;
