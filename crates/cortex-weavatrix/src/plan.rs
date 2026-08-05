//! Deterministic task-aware evidence planning.
//!
//! Weavatrix exposes 42 operations. Asking the same four of them for every
//! task is why a compiled packet could describe a repository's structure
//! without containing a single identifier the task named — measured, not
//! assumed: see `docs/benchmark.md`.
//!
//! This module decides **which** operations to ask for, from the text of the
//! task alone. It is deterministic and contains no model: identifiers are
//! extracted by shape, and the plan is a pure function of the task, the
//! optional symbol, and the budget. A planner that guessed would reintroduce
//! exactly the unaccountability the rest of the crate exists to prevent.

use serde_json::{Value, json};

use crate::EvidenceKind;

/// Most identifiers to carry into a search. Beyond this the alternation stops
/// discriminating and the result is a repository-wide dump.
pub const MAX_IDENTIFIERS: usize = 8;

/// Share of the caller's budget offered to the fact-carrying search. The rest
/// funds structure and the change plan.
const SEARCH_BUDGET_NUMERATOR: u32 = 2;
const SEARCH_BUDGET_DENOMINATOR: u32 = 5;

/// Smallest budget worth passing to a Weavatrix operation.
const MIN_OPERATION_BUDGET: u32 = 200;

/// One Weavatrix call the plan intends to make.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedOperation {
    /// Evidence id prefix, e.g. `WX-SEARCH`.
    pub id: &'static str,
    /// Native operation name.
    pub tool: &'static str,
    pub kind: EvidenceKind,
    pub arguments: Value,
}

/// Identifier-shaped tokens in `task`, in first-seen order.
///
/// Recognised shapes, each of which a human writes when naming real code:
/// backticked spans, `snake_case`, `SCREAMING_SNAKE`, `PascalCase`,
/// `camelCase`, paths ending in a source extension, and `a::b` segments.
/// Ordinary prose words are deliberately not identifiers — searching for
/// "change" matches everything and discriminates nothing.
#[must_use]
pub fn extract_identifiers(task: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for candidate in candidates(task) {
        let candidate = candidate.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
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
const SOURCE_SUFFIXES: &[&str] = &[".rs", ".ts", ".tsx", ".toml", ".md", ".sql", ".proto"];

fn is_identifier(value: &str) -> bool {
    if value.len() < 3 || value.len() > 96 {
        return false;
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
    // snake_case and SCREAMING_SNAKE, or a case change *inside* the word:
    // `maxAttempts` and `RetryLimitTooLarge` qualify, a sentence-initial
    // "Change" does not. Requiring the capital past the first character is
    // what keeps ordinary prose out of the search pattern.
    value.contains('_') || value.chars().skip(1).any(|c| c.is_ascii_uppercase())
}

/// A Rust-regex alternation matching any of `identifiers`, literal-escaped.
#[must_use]
pub fn search_pattern(identifiers: &[String]) -> String {
    identifiers
        .iter()
        .map(|identifier| {
            identifier
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        c.to_string()
                    } else {
                        format!("\\{c}")
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// The operations to run for one task.
///
/// `budget` is the caller's whole evidence budget. Each operation receives a
/// share as a Weavatrix `token_budget`, so trimming happens inside the tool —
/// where the tool knows which array to cut — instead of a fragment being
/// dropped whole afterwards.
#[must_use]
pub fn plan(task: &str, symbol: Option<&str>, budget: u32) -> Vec<PlannedOperation> {
    let identifiers = extract_identifiers(task);
    let search_budget = share(budget, SEARCH_BUDGET_NUMERATOR, SEARCH_BUDGET_DENOMINATOR);
    let remainder = budget
        .saturating_sub(search_budget)
        .max(MIN_OPERATION_BUDGET);
    let structural = share(remainder, 1, 3);

    let mut operations = Vec::with_capacity(6);
    if !identifiers.is_empty() {
        operations.push(PlannedOperation {
            id: "WX-SEARCH",
            tool: "search_code",
            kind: EvidenceKind::SearchHits,
            arguments: json!({
                "query": search_pattern(&identifiers),
                "is_regex": true,
                "before": 1,
                "after": 1,
                "max_results": 40,
                "token_budget": search_budget,
            }),
        });
    }
    if let Some(symbol) = symbol {
        operations.push(PlannedOperation {
            id: "WX-SYMBOL",
            tool: "inspect_symbol",
            kind: EvidenceKind::SymbolContext,
            arguments: json!({
                "label": symbol,
                "context_lines": 3,
                "max_references": 20,
                "token_budget": structural,
            }),
        });
        operations.push(PlannedOperation {
            id: "WX-DEPENDENTS",
            tool: "get_dependents",
            kind: EvidenceKind::Dependents,
            arguments: json!({ "label": symbol, "token_budget": structural }),
        });
    }
    operations.push(PlannedOperation {
        id: "WX-MODULES",
        tool: "module_map",
        kind: EvidenceKind::ModuleMap,
        arguments: json!({
            "top_n": 16,
            "include_non_product": false,
            "token_budget": structural,
        }),
    });
    operations.push(PlannedOperation {
        id: "WX-VERIFY",
        tool: "verified_change",
        kind: EvidenceKind::ChangePlan,
        arguments: json!({
            "task": task,
            "phase": "plan",
            "duplicate_ratchet": true,
            "run_tests": false,
            "token_budget": structural,
        }),
    });
    operations
}

fn share(budget: u32, numerator: u32, denominator: u32) -> u32 {
    budget
        .saturating_mul(numerator)
        .checked_div(denominator)
        .unwrap_or(MIN_OPERATION_BUDGET)
        .max(MIN_OPERATION_BUDGET)
}

#[cfg(test)]
mod tests {
    use super::{EvidenceKind, extract_identifiers, plan, search_pattern};

    #[test]
    fn identifiers_are_recognised_by_shape_not_by_vocabulary() {
        let found = extract_identifiers(
            "Change bounded retry so MAX_RETRY_ATTEMPTS and maxAttempts agree, \
             see crates/cortex-run/src/retry.rs and RunError::RetryLimitTooLarge.",
        );
        assert!(found.contains(&"MAX_RETRY_ATTEMPTS".to_owned()));
        assert!(found.contains(&"maxAttempts".to_owned()));
        assert!(found.iter().any(|value| value.ends_with("retry.rs")));
        assert!(
            found
                .iter()
                .any(|value| value.contains("RetryLimitTooLarge"))
        );
        // Prose is not an identifier: searching for it discriminates nothing.
        for word in ["Change", "bounded", "retry", "and", "agree"] {
            assert!(
                !found.contains(&word.to_owned()),
                "{word} was taken as code"
            );
        }
    }

    #[test]
    fn backticked_spans_win_and_the_list_is_bounded() {
        let task = "`alpha_one` `beta_two` `gamma_three` `delta_four` \
                    `epsilon_five` `zeta_six` `eta_seven` `theta_eight` `iota_nine`";
        let found = extract_identifiers(task);
        assert_eq!(found.len(), super::MAX_IDENTIFIERS);
        assert_eq!(found[0], "alpha_one", "first-seen order is stable");
        assert!(
            !found.contains(&"iota_nine".to_owned()),
            "the tail is bounded"
        );
    }

    #[test]
    fn a_task_naming_code_asks_for_the_facts_a_summary_cannot_carry() {
        let operations = plan("rename `RetryLimitTooLarge`", Some("apply_command"), 4_000);
        let tools: Vec<&str> = operations.iter().map(|operation| operation.tool).collect();
        assert_eq!(
            tools,
            [
                "search_code",
                "inspect_symbol",
                "get_dependents",
                "module_map",
                "verified_change"
            ]
        );
        // Every operation carries a budget, so Weavatrix trims where it knows
        // what to cut instead of a whole fragment being dropped afterwards.
        for operation in &operations {
            assert!(
                operation.arguments.get("token_budget").is_some(),
                "{} has no budget",
                operation.tool
            );
        }
        assert_eq!(operations[0].kind, EvidenceKind::SearchHits);
    }

    #[test]
    fn a_task_naming_no_code_falls_back_to_structure() {
        let operations = plan("make the thing faster please", None, 4_000);
        let tools: Vec<&str> = operations.iter().map(|operation| operation.tool).collect();
        assert_eq!(tools, ["module_map", "verified_change"]);
    }

    #[test]
    fn regex_metacharacters_in_identifiers_are_escaped() {
        let pattern = search_pattern(&["a.b".to_owned(), "c::d".to_owned()]);
        assert_eq!(pattern, "a\\.b|c\\:\\:d");
    }

    #[test]
    fn a_tiny_budget_still_produces_usable_operation_budgets() {
        for operation in plan("touch `alpha_one`", None, 1) {
            let budget = operation.arguments["token_budget"].as_u64().unwrap();
            assert!(budget > 0, "{} received a zero budget", operation.tool);
        }
    }
}
