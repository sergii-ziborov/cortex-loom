//! Deterministic task-aware evidence planning.
//!
//! Weavatrix exposes 43 operations. Asking the same four of them for every
//! task is why a compiled packet could describe a repository's structure
//! without containing a single identifier the task named — measured, not
//! assumed: see `docs/benchmark.md`. Git history, stack-trace mapping, and
//! test selection are planned when the question names those shapes.
//!
//! This module decides **which** operations to ask for, from the text of the
//! task alone. It is deterministic and contains no model: identifiers are
//! extracted by shape, intent cues pick structural tools when the question is
//! about dependents or contracts, and the plan is a pure function of the task,
//! the optional symbol, and the budget.

use serde_json::Value;

use crate::plan_intent::TaskIntent;
use crate::{EvidenceKind, PlanHints, PriorRunMemory};

#[path = "plan_operations.rs"]
mod operations;

use operations::{
    asks_for_change_plan, blast_search_query, dependents_op, endpoints_op, git_history_op,
    memory_op, modules_op, neighbors_op, search_op, search_pattern_op, select_tests_op, share,
    stacktrace_op, symbol_op, verify_op,
};

/// Most identifiers to carry into a search. Beyond this the alternation stops
/// discriminating and the result is a repository-wide dump.
pub const MAX_IDENTIFIERS: usize = 8;

/// Share of the caller's budget offered to the fact-carrying search. The rest
/// funds structure and the change plan.
const SEARCH_BUDGET_NUMERATOR: u32 = 2;
const SEARCH_BUDGET_DENOMINATOR: u32 = 5;

const MIN_OPERATION_BUDGET: u32 = 200;

/// Only an estimate is possible for these, and an estimate is a policy rather
/// than a fact — so it is a value a deployment can change, not a constant
/// compiled in. Defaults come from one measurement on one repository
/// (2026-08-05) and will be wrong somewhere else; that is exactly why they
/// are overridable.
///
/// `bounded` means "trims what it can and reports whether it fitted", not
/// "will fit", and that is by design:
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
    pub git_history_tokens: u32,
    pub stacktrace_tokens: u32,
    pub test_selection_tokens: u32,
    pub memory_tokens: u32,
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
            git_history_tokens: 800,
            stacktrace_tokens: 1_500,
            test_selection_tokens: 1_200,
            memory_tokens: 600,
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
    "git_history",
    "query_graph",
    "read_source",
    "search_code",
];

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedOperation {
    /// Evidence id prefix, e.g. `WX-SEARCH`.
    pub id: &'static str,
    /// Native operation name.
    pub tool: &'static str,
    pub kind: EvidenceKind,
    pub arguments: Value,
    /// For a bounded operation this is the `token_budget` it was given, and
    /// the runtime is contractually held to it. For an unbounded one it is a
    /// [`PlanPolicy`] estimate, which is a guess with a knob rather than a
    /// promise. The plan trims against this so that a budget is never spent
    /// on evidence the compiler would only discard.
    pub expected_tokens: u32,
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
    let mut rest = task;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        push_identifier(&mut found, &after[..close], true);
        if found.len() == MAX_IDENTIFIERS {
            return found;
        }
        rest = &after[close + 1..];
    }
    for candidate in
        task.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '"'))
    {
        push_identifier(&mut found, candidate, false);
        if found.len() == MAX_IDENTIFIERS {
            break;
        }
    }
    found
}

fn push_identifier(found: &mut Vec<String>, candidate: &str, explicit: bool) {
    let candidate = candidate.trim_matches(|c: char| {
        !c.is_alphanumeric() && !matches!(c, '_' | '/' | '{' | '}' | ':' | '.' | '-')
    });
    if !(is_identifier(candidate) || (explicit && is_explicit_identifier(candidate))) {
        return;
    }
    if !found.iter().any(|seen| seen == candidate) {
        found.push(candidate.to_owned());
    }
}

fn is_explicit_identifier(value: &str) -> bool {
    let prose_acronym = !value.contains(['_', '/'])
        && value
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && value
            .chars()
            .filter(char::is_ascii_alphabetic)
            .all(|character| character.is_ascii_uppercase());
    !prose_acronym
        && (3..=96).contains(&value.len())
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '_' | '.' | '/' | ':' | '{' | '}' | '-')
        })
}

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
    let lowercase = crate::fold::fold_text(value);
    if value.contains("::")
        || crate::fold::SOURCE_SUFFIXES
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

/// Whether a symbol is spelled like a type: `PascalCase` with a lowercase tail.
///
/// All-caps constants and `snake_case` functions are excluded on purpose —
/// the reference-edge gap this gates on was measured only for type names.
fn is_type_name(symbol: &str) -> bool {
    symbol.chars().next().is_some_and(char::is_uppercase)
        && symbol.chars().any(char::is_lowercase)
        && !symbol.contains('_')
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
    plan_with_hints(task, symbol, budget, policy, PlanHints::default())
}

/// As [`plan_with`], with deterministic controls supplied by an active skill.
#[must_use]
pub fn plan_with_hints(
    task: &str,
    symbol: Option<&str>,
    budget: u32,
    policy: PlanPolicy,
    hints: PlanHints,
) -> Vec<PlannedOperation> {
    plan_with_prior(task, symbol, budget, policy, hints, None, None)
}

/// As [`plan_with_hints`], including prior-run memory when the caller has it.
#[must_use]
pub fn plan_with_prior(
    task: &str,
    symbol: Option<&str>,
    budget: u32,
    policy: PlanPolicy,
    hints: PlanHints,
    prior: Option<&PriorRunMemory>,
    inventory_glob: Option<&str>,
) -> Vec<PlannedOperation> {
    let mut operations = plan_all(task, symbol, budget, policy, hints, prior, inventory_glob);
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
    hints: PlanHints,
    prior: Option<&PriorRunMemory>,
    inventory_glob: Option<&str>,
) -> Vec<PlannedOperation> {
    let intent = hints.intent_or_detect(task);
    let mut identifiers = extract_identifiers(task);
    if let Some(symbol) = symbol
        && !identifiers.iter().any(|identifier| identifier == symbol)
    {
        identifiers.insert(0, symbol.to_owned());
        identifiers.truncate(MAX_IDENTIFIERS);
    }
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
                // Type names only. Call edges cover functions completely
                // (measured 4/4 on the reference ground truth), so adding
                // neighbours there just evicted the search hits under the
                // budget and cost two probe anchors. The reference-edge gap
                // `get_neighbors` closes exists for structs and enums, whose
                // names are PascalCase.
                if is_type_name(symbol) {
                    operations.push(neighbors_op(symbol, policy));
                }
            }
        }
        TaskIntent::ApiContract => {
            operations.push(endpoints_op(policy));
        }
        TaskIntent::ModuleTopology => {
            operations.push(modules_op(policy));
        }
        TaskIntent::GitHistory => {
            operations.push(git_history_op(policy));
        }
        TaskIntent::StackTrace => {
            operations.push(stacktrace_op(task, policy));
        }
        TaskIntent::TestSelection => {
            operations.push(select_tests_op(policy));
        }
        TaskIntent::IdentifierChange | TaskIntent::RuntimeConfig | TaskIntent::PriorAttempt => {}
    }
    if let Some(prior) = prior.filter(|memory| !memory.is_empty())
        && let Some(memory) = memory_op(task, prior, policy)
    {
        if intent == TaskIntent::PriorAttempt {
            operations.insert(0, memory);
        } else {
            operations.push(memory);
        }
    }
    if !identifiers.is_empty() {
        let glob = crate::fold::search_glob_in(&identifiers, inventory_glob);
        if intent == TaskIntent::RuntimeConfig {
            let slice = share(search_budget, 1, 2);
            operations.push(search_op(
                "WX-SEARCH",
                &identifiers,
                slice,
                policy,
                glob.as_str(),
            ));
            operations.push(search_op(
                "WX-CONFIG",
                &identifiers,
                slice,
                policy,
                "config/**",
            ));
        } else if intent == TaskIntent::BlastRadius {
            let query = blast_search_query(symbol, &identifiers);
            if !query.is_empty() {
                operations.push(search_pattern_op(
                    "WX-SEARCH",
                    &query,
                    search_budget,
                    policy,
                    glob.as_str(),
                ));
            }
        } else {
            operations.push(search_op(
                "WX-SEARCH",
                &identifiers,
                search_budget,
                policy,
                glob.as_str(),
            ));
        }
    }
    // Blast-radius questions already have dependents; symbol source is
    // secondary and often too large to keep under a 4k budget.
    if let Some(symbol) = symbol
        && !skip_secondary_graph(intent)
    {
        operations.push(symbol_op(symbol, structural, policy));
    }
    if intent != TaskIntent::ModuleTopology {
        operations.push(modules_op(policy));
    }
    if let Some(symbol) = symbol
        && !skip_secondary_graph(intent)
    {
        operations.push(dependents_op(symbol, policy));
    }
    if !hints.skip_change_plan && asks_for_change_plan(task) {
        operations.push(verify_op(task, policy));
    }
    operations
}

/// These intents already asked the operation that answers the question.
/// A 4k budget cannot also carry `context_bundle` / extra dependents.
const fn skip_secondary_graph(intent: TaskIntent) -> bool {
    matches!(
        intent,
        TaskIntent::BlastRadius
            | TaskIntent::GitHistory
            | TaskIntent::StackTrace
            | TaskIntent::TestSelection
    )
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
