use super::{PlanPolicy, PlannedOperation, operations, search_pattern};

/// A new feature often names no existing symbol. Search two concrete task
/// terms instead of falling back to a module map or a repository-wide dump.
#[must_use]
pub fn coding_focus_query(task: &str) -> String {
    let terms = focus_terms(task);
    if terms.is_empty() {
        String::new()
    } else {
        format!("(?i)\\b(?:{})\\b", search_pattern(&terms))
    }
}

#[must_use]
pub fn coding_declaration_query(task: &str) -> String {
    let terms = focus_terms(task);
    if terms.is_empty() {
        String::new()
    } else {
        format!(
            r"(?i)\b(?:fn|struct|enum|trait|class|def|type|interface)\s+\w*(?:{})\w*\b",
            search_pattern(&terms)
        )
    }
}

pub(super) fn planned_ops(
    task: &str,
    search_budget: u32,
    policy: PlanPolicy,
    inventory_glob: Option<&str>,
) -> Vec<PlannedOperation> {
    let query = coding_focus_query(task);
    if query.is_empty() {
        return Vec::new();
    }
    // Weavatrix treats a positive glob as a scan override. An unscoped
    // include can pull ignored build output and dependencies into the first
    // 40 hits, then trim away every product hit under a small token budget.
    let glob = inventory_glob.unwrap_or("");
    let mut planned = vec![
        operations::search_pattern_op(
            "WX-FOCUS-DECL",
            &coding_declaration_query(task),
            operations::share(search_budget, 1, 3),
            policy,
            glob,
        ),
        operations::search_pattern_op(
            "WX-FOCUS",
            &query,
            operations::share(search_budget, 2, 3),
            policy,
            glob,
        ),
    ];
    let lower = crate::fold::fold_text(task);
    if lower.contains("mcp") && lower.contains("schema") {
        planned.push(operations::search_pattern_op(
            "WX-FOCUS-MCP",
            &query,
            400,
            policy,
            "**/*mcp*/**",
        ));
    }
    if crate::plan_intent::asks_for_ui(task) {
        planned.push(operations::search_pattern_op(
            "WX-FOCUS-UI",
            &query,
            800,
            policy,
            "ui/src/**/*.{ts,tsx,js,jsx}",
        ));
    }
    planned
}

fn focus_terms(task: &str) -> Vec<String> {
    const NOISE: &[&str] = &[
        "about",
        "across",
        "after",
        "again",
        "before",
        "behavior",
        "change",
        "changes",
        "code",
        "cortex",
        "existing",
        "files",
        "implement",
        "include",
        "including",
        "loom",
        "preserve",
        "relevant",
        "remove",
        "source",
        "tests",
        "update",
        "using",
        "without",
    ];
    let mut terms: Vec<String> = Vec::new();
    for word in task.split(|character: char| !character.is_alphabetic()) {
        let lower = word.to_lowercase();
        if !(5..=40).contains(&lower.len())
            || NOISE.contains(&lower.as_str())
            || terms.iter().any(|seen| seen == &lower)
        {
            continue;
        }
        terms.push(lower);
        if terms.len() == 2 {
            break;
        }
    }
    terms
}
