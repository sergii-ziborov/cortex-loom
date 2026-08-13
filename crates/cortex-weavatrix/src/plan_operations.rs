use serde_json::json;

use super::{
    EvidenceKind, MIN_OPERATION_BUDGET, PlanPolicy, PlannedOperation, escape_regex_literal,
    search_pattern,
};

pub(super) fn search_op(
    id: &'static str,
    identifiers: &[String],
    search_budget: u32,
    policy: PlanPolicy,
    glob: &'static str,
) -> PlannedOperation {
    search_pattern_op(
        id,
        &search_pattern(identifiers),
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
    glob: &'static str,
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
        r"\b(fn|struct|enum|trait|type)\s+{symbol}\b|,\s*{symbol}\s*\)|[:=]\s*{symbol}\s*\(|(return|match)\s+{symbol}\s*\("
    )
}

pub(super) fn asks_for_change_plan(task: &str) -> bool {
    const CUES: &[&str] = &[
        "change plan",
        "implementation plan",
        "pre-commit plan",
        "prepare change",
        "plan the change",
        "plan this change",
    ];
    let lower = task.to_ascii_lowercase();
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

pub(super) fn share(budget: u32, numerator: u32, denominator: u32) -> u32 {
    budget
        .saturating_mul(numerator)
        .checked_div(denominator)
        .unwrap_or(MIN_OPERATION_BUDGET)
        .max(MIN_OPERATION_BUDGET)
}
