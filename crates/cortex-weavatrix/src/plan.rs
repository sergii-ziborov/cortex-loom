//! Deterministic task-aware evidence planning.
//!
//! Weavatrix exposes 42 operations. Asking the same four of them for every
//! task is why a compiled packet could describe a repository's structure
//! without containing a single identifier the task named — measured, not
//! assumed: see `docs/benchmark.md`.
//!
//! This module decides **which** operations to ask for, from the text of the
//! task alone. It is deterministic and contains no model: identifiers are
//! extracted by shape, intent cues pick structural tools when the question is
//! about dependents or contracts, and the plan is a pure function of the task,
//! the optional symbol, and the budget.

use serde_json::{Value, json};

use crate::EvidenceKind;
use crate::plan_intent::{TaskIntent, detect};

/// Most identifiers to carry into a search. Beyond this the alternation stops
/// discriminating and the result is a repository-wide dump.
pub const MAX_IDENTIFIERS: usize = 8;

/// Share of the caller's budget offered to the fact-carrying search. The rest
/// funds structure and the change plan.
const SEARCH_BUDGET_NUMERATOR: u32 = 2;
const SEARCH_BUDGET_DENOMINATOR: u32 = 5;

/// Smallest budget worth passing to a Weavatrix operation.
const MIN_OPERATION_BUDGET: u32 = 200;

/// What an unbounded operation is assumed to cost.
///
/// Only an estimate is possible for these, and an estimate is a policy rather
/// than a fact — so it is a value a deployment can change, not a constant
/// compiled in. Defaults come from one measurement on one repository
/// (2026-08-05) and will be wrong somewhere else; that is exactly why they
/// are overridable.
///
/// Bounded operations need entries too. `bounded` means "trims what it can
/// and reports whether it fitted", not "will fit", and that is by design:
/// Weavatrix's graph relationships are lossless, so it drops source excerpts,
/// keeps the relationships, and says `fit: false` rather than returning a
/// smaller answer that is also a wrong one. Measured on `weavatrix-rust`
/// 2.2.0 and 2.2.1: `context_bundle` under an 800-token request dropped 46
/// items and still returned 4 778. Treating a granted request as a guarantee
/// would under-count the plan six-fold, so bounded operations are estimated
/// like the rest and the overrun is surfaced as a warning on the bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanPolicy {
    pub search_tokens: u32,
    pub symbol_tokens: u32,
    pub modules_tokens: u32,
    pub dependents_tokens: u32,
    pub endpoints_tokens: u32,
    pub change_plan_tokens: u32,
    /// Pool for bounded `read_source` windows after search hits.
    pub source_tokens: u32,
    /// How far the plan may commit beyond the budget before it stops asking.
    ///
    /// Above 1 because an operation often returns less than its estimate, so
    /// cutting at exactly the budget would drop evidence that would have fit.
    pub overcommit: u32,
}

impl Default for PlanPolicy {
    fn default() -> Self {
        Self {
            search_tokens: 1_400,
            symbol_tokens: 4_900,
            modules_tokens: 100,
            dependents_tokens: 2_500,
            endpoints_tokens: 400,
            change_plan_tokens: 6_000,
            source_tokens: 1_300,
            overcommit: 2,
        }
    }
}

/// Weavatrix operations that bound their answer by `token_budget`.
///
/// Named by `weavatrix-rust` 2.2.0 itself: passing the parameter to anything
/// else is now a hard error rather than a silent no-op, which is the right
/// behaviour and the reason this list can be trusted instead of guessed.
/// Keep it in step with the runtime — a stale entry here becomes a failed
/// operation, not a silent overrun.
pub const BUDGET_HONOURING: &[&str] = &[
    "context_bundle",
    "query_graph",
    "read_source",
    "search_code",
];

/// One Weavatrix call the plan intends to make.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedOperation {
    /// Evidence id prefix, e.g. `WX-SEARCH`.
    pub id: &'static str,
    /// Native operation name.
    pub tool: &'static str,
    pub kind: EvidenceKind,
    pub arguments: Value,
    /// What this operation is expected to cost.
    ///
    /// For a bounded operation this is the `token_budget` it was given, and
    /// the runtime is contractually held to it. For an unbounded one it is a
    /// [`PlanPolicy`] estimate, which is a guess with a knob rather than a
    /// promise. The plan trims against this so that a budget is never spent
    /// on evidence the compiler would only discard.
    pub expected_tokens: u32,
    /// True when the operation bounds its own answer by `token_budget`.
    ///
    /// Not cosmetic: `weavatrix-rust` 2.2.0 rejects the parameter on
    /// operations that do not implement it, so this decides whether it is
    /// sent at all.
    pub bounded: bool,
}

/// Identifier-shaped tokens in `task`, in first-seen order.
///
/// Recognised shapes, each of which a human writes when naming real code:
/// backticked spans, `snake_case`, `SCREAMING_SNAKE`, `PascalCase`,
/// `camelCase`, paths ending in a source extension, URL-like `/api/...`
/// paths, and `a::b` segments. Ordinary prose words are deliberately not
/// identifiers — searching for "change" matches everything and discriminates
/// nothing.
#[must_use]
pub fn extract_identifiers(task: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for candidate in candidates(task) {
        let candidate =
            candidate.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '/');
        if !is_identifier(candidate) {
            continue;
        }
        if !found.iter().any(|seen| seen == candidate) {
            found.push(candidate.to_owned());
        }
        if found.len() == MAX_IDENTIFIERS {
            break;
        }
    }
    found
}

fn candidates(task: &str) -> Vec<&str> {
    let mut out = Vec::new();
    // Backticked spans first: an author who typed backticks was explicit.
    let mut rest = task;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        out.push(&after[..close]);
        rest = &after[close + 1..];
    }
    out.extend(task.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '"')));
    out
}

/// Suffixes that make a token a file path worth searching for by name.
const SOURCE_SUFFIXES: &[&str] = &[
    ".rs", ".ts", ".tsx", ".toml", ".json", ".yaml", ".yml", ".md", ".sql", ".proto",
];

fn is_identifier(value: &str) -> bool {
    if value.len() < 3 || value.len() > 96 {
        return false;
    }
    if value.starts_with('/')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '{' | '}'))
    {
        return true;
    }
    let lowercase = value.to_ascii_lowercase();
    if value.contains("::")
        || SOURCE_SUFFIXES
            .iter()
            .any(|suffix| lowercase.ends_with(suffix))
    {
        return true;
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return false;
    }
    // snake_case / SCREAMING_SNAKE (underscore), or true camel/Pascal case
    // (both a lowercase letter and an interior capital). All-caps tokens
    // without an underscore — `HTTP`, `API`, `MCP` — are prose acronyms; if
    // they enter the regex as alternation arms they match half the tree and
    // crowd out the real hit.
    value.contains('_')
        || (value.chars().any(|c| c.is_ascii_lowercase())
            && value.chars().skip(1).any(|c| c.is_ascii_uppercase()))
}

/// A Rust-regex alternation matching any of `identifiers`, literal-escaped.
///
/// Only metacharacters recognised by Rust's `regex` crate are escaped.
/// Escaping `/` as `\/` is rejected there (unnecessary escapes are errors),
/// which made URL-path identifiers silently fail to match TypeScript and
/// other non-Rust hits — measured on the skills-compile-contract fixture.
#[must_use]
pub fn search_pattern(identifiers: &[String]) -> String {
    identifiers
        .iter()
        .map(|identifier| escape_regex_literal(identifier))
        .collect::<Vec<_>>()
        .join("|")
}

fn escape_regex_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(
            character,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// The operations to run for one task, under the default policy.
///
/// A bounded operation carries a `token_budget` and is held to it. An
/// unbounded one carries none — `weavatrix-rust` 2.2.0 rejects the parameter
/// where it is not implemented — so its size is a [`PlanPolicy`] estimate.
/// The tail is dropped once the plan has committed more than the budget can
/// carry, because evidence the compiler would only discard still costs
/// latency and Weavatrix work to fetch.
#[must_use]
pub fn plan(task: &str, symbol: Option<&str>, budget: u32) -> Vec<PlannedOperation> {
    plan_with(task, symbol, budget, PlanPolicy::default())
}

/// As [`plan`], with an explicit cost policy.
///
/// The estimates for unbounded operations are a snapshot of one repository;
/// a deployment whose Weavatrix answers are much larger or much smaller
/// should say so here rather than live with a constant compiled in.
#[must_use]
pub fn plan_with(
    task: &str,
    symbol: Option<&str>,
    budget: u32,
    policy: PlanPolicy,
) -> Vec<PlannedOperation> {
    let mut operations = plan_all(task, symbol, budget, policy);
    let ceiling = budget.saturating_mul(policy.overcommit.max(1));
    let mut committed = 0_u32;
    operations.retain(|operation| {
        // A bounded operation cannot overrun: the runtime holds it to the
        // budget it was given, so admitting one that reaches the ceiling is
        // safe. An unbounded one is an estimate that could be wrong in either
        // direction, so it is only asked for if it fits whole.
        let fits = if operation.bounded {
            committed < ceiling
        } else {
            committed.saturating_add(operation.expected_tokens) <= ceiling
        };
        if !fits {
            return false;
        }
        committed = committed.saturating_add(operation.expected_tokens);
        true
    });
    operations
}

fn plan_all(
    task: &str,
    symbol: Option<&str>,
    budget: u32,
    policy: PlanPolicy,
) -> Vec<PlannedOperation> {
    let intent = detect(task);
    let identifiers = extract_identifiers(task);
    let search_budget = share(budget, SEARCH_BUDGET_NUMERATOR, SEARCH_BUDGET_DENOMINATOR);
    let remainder = budget
        .saturating_sub(search_budget)
        .max(MIN_OPERATION_BUDGET);
    let structural = share(remainder, 1, 3);

    let mut operations = Vec::with_capacity(6);
    // Structural intents put the graph tool that answers the question first.
    // Identifier-driven plans still prefer cheap search before orientation.
    match intent {
        TaskIntent::BlastRadius => {
            if let Some(symbol) = symbol {
                operations.push(dependents_op(symbol, policy));
            }
        }
        TaskIntent::ApiContract => {
            operations.push(endpoints_op(policy));
        }
        TaskIntent::ModuleTopology => {
            operations.push(modules_op(policy));
        }
        TaskIntent::IdentifierChange | TaskIntent::RuntimeConfig => {}
    }
    if !identifiers.is_empty() {
        if intent == TaskIntent::RuntimeConfig {
            let slice = share(search_budget, 1, 2);
            operations.push(search_op(
                "WX-SEARCH",
                &identifiers,
                slice,
                policy,
                "{apps,crates}/**/*.rs",
            ));
            operations.push(search_op(
                "WX-CONFIG",
                &identifiers,
                slice,
                policy,
                "config/**",
            ));
        } else {
            operations.push(search_op(
                "WX-SEARCH",
                &identifiers,
                search_budget,
                policy,
                "{apps,crates}/**/*.rs",
            ));
        }
    }
    // Blast-radius questions already have dependents; symbol source is
    // secondary and often too large to keep under a 4k budget.
    if let Some(symbol) = symbol
        && intent != TaskIntent::BlastRadius
    {
        operations.push(symbol_op(symbol, structural, policy));
    }
    if intent != TaskIntent::ModuleTopology {
        operations.push(modules_op(policy));
    }
    if let Some(symbol) = symbol
        && intent != TaskIntent::BlastRadius
    {
        operations.push(dependents_op(symbol, policy));
    }
    if asks_for_change_plan(task) {
        operations.push(verify_op(task, policy));
    }
    operations
}

fn search_op(
    id: &'static str,
    identifiers: &[String],
    search_budget: u32,
    policy: PlanPolicy,
    glob: &'static str,
) -> PlannedOperation {
    PlannedOperation {
        id,
        tool: "search_code",
        kind: EvidenceKind::SearchHits,
        arguments: json!({
            "query": search_pattern(identifiers),
            "is_regex": true,
            "before": 1,
            "after": 1,
            "max_results": 40,
            // Prefer Rust product trees; docs/bench noise burns the budget first.
            "glob": glob,
            "token_budget": search_budget,
        }),
        expected_tokens: policy.search_tokens.max(search_budget),
        bounded: true,
    }
}

fn asks_for_change_plan(task: &str) -> bool {
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

fn symbol_op(symbol: &str, structural: u32, policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-SYMBOL",
        // `context_bundle` bounds its answer; `inspect_symbol` does not
        // and returned 4 834 tokens against an 800 request before 2.2.0
        // started rejecting the parameter outright. Preferring the
        // bounded operation is now both correct and cheaper.
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

fn modules_op(policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-MODULES",
        tool: "module_map",
        kind: EvidenceKind::ModuleMap,
        arguments: json!({"top_n": 16, "include_non_product": false}),
        expected_tokens: policy.modules_tokens,
        bounded: false,
    }
}

fn dependents_op(symbol: &str, policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-DEPENDENTS",
        tool: "get_dependents",
        kind: EvidenceKind::Dependents,
        arguments: json!({ "label": symbol }),
        expected_tokens: policy.dependents_tokens,
        bounded: false,
    }
}

fn endpoints_op(policy: PlanPolicy) -> PlannedOperation {
    PlannedOperation {
        id: "WX-ENDPOINTS",
        tool: "list_endpoints",
        kind: EvidenceKind::Endpoints,
        arguments: json!({}),
        expected_tokens: policy.endpoints_tokens,
        bounded: false,
    }
}

fn verify_op(task: &str, policy: PlanPolicy) -> PlannedOperation {
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

fn share(budget: u32, numerator: u32, denominator: u32) -> u32 {
    budget
        .saturating_mul(numerator)
        .checked_div(denominator)
        .unwrap_or(MIN_OPERATION_BUDGET)
        .max(MIN_OPERATION_BUDGET)
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
