//! Bounded `read_source` follow-up after `search_code`.
//!
//! Search hits name files and lines; the identifiers a contract/transport
//! question needs often sit a few lines away from the match (`compile_skill`,
//! `fn endpoint`). Reading those files whole is the naive arm. This module
//! asks Weavatrix for a window around each hit under a shared token budget.

use serde_json::{Value, json};

use crate::plan::PlanPolicy;

/// Most distinct source windows to open after a search. Beyond this the
/// follow-up starts to resemble a directory sweep.
pub const MAX_SOURCE_FILES: usize = 6;

/// Lines kept above and below each hit.
///
/// `serve_http` sits ~20 lines above the `/mcp` route registration; keep a
/// generous above-window so the entry point lands in the same read.
pub const SOURCE_BEFORE: u32 = 24;
pub const SOURCE_AFTER: u32 = 48;

/// Window shape for one gather pass.
///
/// A broad, enumerating question needs more and larger windows than an
/// identifier question: the measured cross-cutting probe compiled barely half
/// its budget and lost every fact that lived one file away from the hits.
/// Breadth widens the follow-up deterministically; the compiler budget is
/// still the ceiling, so a widened gather can never overrun the packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceWindow {
    /// Distinct files to open.
    pub max_files: usize,
    /// Lines above each hit.
    pub before: u32,
    /// Lines below each hit.
    pub after: u32,
    /// Numerator of the budget share the source pool may use (denominator 5).
    pub pool_fifths: u32,
}

impl SourceWindow {
    /// Window for one task: enumerating questions get the wide shape.
    ///
    /// The pool grew from three fifths to four once graph answers were
    /// rendered as text: `context_bundle` fell from ~1 500 tokens of JSON to
    /// ~250 of prose, and a broad question was then compiling 2 518 of a
    /// 4 000-token budget. Under-spending a granted budget on the one intent
    /// that asks for breadth is the opposite of what this window exists for.
    #[must_use]
    pub fn for_task(task: &str) -> Self {
        if crate::plan_intent::is_broad(task) {
            Self {
                max_files: 9,
                before: SOURCE_BEFORE,
                after: 120,
                pool_fifths: 4,
            }
        } else {
            Self::default()
        }
    }
}

impl Default for SourceWindow {
    fn default() -> Self {
        Self {
            max_files: MAX_SOURCE_FILES,
            before: SOURCE_BEFORE,
            after: SOURCE_AFTER,
            pool_fifths: 2,
        }
    }
}

/// Where a symbol's definition head sits in a text, if it is there at all.
///
/// Matches a language-agnostic definition head (`fn`, `class`, `def`,
/// `func`, …) with a word boundary after the name. Brace balance remains
/// a last-resort completeness check; prefer Weavatrix span metadata when
/// the graph already knows the exact extent.
#[must_use]
pub fn definition_head_index(text: &str, symbol: &str) -> Option<usize> {
    // Source is almost always ASCII; ascii-lowercase keeps byte indices so
    // completeness can slice the original text. Do not NFKC-fold the haystack.
    let lower = text.to_ascii_lowercase();
    let symbol = symbol.to_ascii_lowercase();
    for keyword in [
        "fn ",
        "struct ",
        "enum ",
        "trait ",
        "type ",
        "class ",
        "interface ",
        "function ",
        "def ",
        "func ",
        "record ",
    ] {
        let mut from = 0;
        while let Some(relative) = lower[from..].find(keyword) {
            let head = from + relative;
            let name_start = head + keyword.len();
            let after_name = name_start + symbol.len();
            if lower[name_start..].starts_with(symbol.as_str())
                && !lower[after_name..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Some(head);
            }
            from = name_start;
        }
    }
    None
}

/// Whether a text carries the symbol's **complete** definition.
///
/// `None` when the definition head is absent. `Some(true)` when, from the
/// head, the braces balance back to zero (or a `;` ends a bodiless item)
/// before the text runs out. The measured failure this guards against: a
/// window cut a six-field struct after four fields, the packet passed
/// sufficiency, and the model faithfully implemented the four fields it was
/// shown.
#[must_use]
pub fn definition_is_complete(text: &str, symbol: &str) -> Option<bool> {
    let head = definition_head_index(text, symbol)?;
    let mut depth = 0_i32;
    let mut opened = false;
    let mut mode = BraceMode::Code;
    let mut previous = '\0';
    for character in text[head..].chars() {
        mode = advance_brace_mode(mode, previous, character);
        if mode == BraceMode::Code {
            match character {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => {
                    depth -= 1;
                    if opened && depth == 0 {
                        return Some(true);
                    }
                }
                ';' if !opened => return Some(true),
                _ => {}
            }
        }
        previous = character;
    }
    Some(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BraceMode {
    Code,
    LineComment,
    BlockComment,
    String,
    Char,
}

fn advance_brace_mode(mode: BraceMode, previous: char, character: char) -> BraceMode {
    match mode {
        BraceMode::LineComment if character == '\n' => BraceMode::Code,
        BraceMode::LineComment => BraceMode::LineComment,
        BraceMode::BlockComment if previous == '*' && character == '/' => BraceMode::Code,
        BraceMode::BlockComment => BraceMode::BlockComment,
        BraceMode::String if previous != '\\' && character == '"' => BraceMode::Code,
        BraceMode::String => BraceMode::String,
        BraceMode::Char if previous != '\\' && character == '\'' => BraceMode::Code,
        BraceMode::Char => BraceMode::Char,
        BraceMode::Code if previous == '/' && character == '/' => BraceMode::LineComment,
        BraceMode::Code if previous == '/' && character == '*' => BraceMode::BlockComment,
        BraceMode::Code if character == '"' => BraceMode::String,
        BraceMode::Code if character == '\'' => BraceMode::Char,
        BraceMode::Code => BraceMode::Code,
    }
}

/// One search match that can be turned into a `read_source` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: String,
    pub line: u32,
    pub text: String,
}

/// Collect path/line pairs from a `search_code` result, first-seen order.
#[must_use]
pub fn hits_from_search(value: &Value) -> Vec<SearchHit> {
    let Some(matches) = value.get("matches").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for entry in matches {
        let Some(path) = entry.get("path").and_then(Value::as_str) else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        let line = u32::try_from(
            entry
                .get("line")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, u64::from(u32::MAX)),
        )
        .unwrap_or(1);
        hits.push(SearchHit {
            path: path.to_owned(),
            line,
            text: entry
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        });
    }
    hits
}

/// Deduplicate overlapping windows, capped at `max_files`.
///
/// Product source (`.rs` under `apps/` / `crates/`) is preferred over docs,
/// fixtures, and the benchmark's own task list — otherwise the first hits are
/// README noise and the server route that answers the contract question never
/// opens.
/// Prefer hits that carry a missing semantic term, plus other hits in the
/// same file. This keeps a broad recovery query from spending every source
/// window on generic matches such as framework `Router` imports.
#[must_use]
pub fn unique_paths_for_patterns(
    hits: &[SearchHit],
    max_files: usize,
    preferred_patterns: &[String],
    task: &str,
) -> Vec<SearchHit> {
    let mut preferred_paths: std::collections::HashMap<&str, std::collections::HashSet<&str>> =
        std::collections::HashMap::new();
    for hit in hits {
        let lower = hit.text.to_ascii_lowercase();
        for pattern in preferred_patterns {
            if !pattern.is_empty() && lower.contains(pattern.as_str()) {
                preferred_paths
                    .entry(hit.path.as_str())
                    .or_default()
                    .insert(pattern.as_str());
            }
        }
    }
    let mut ranked: Vec<(i32, usize, &SearchHit)> = hits
        .iter()
        .enumerate()
        .map(|(index, hit)| {
            let affinity = preferred_paths
                .get(hit.path.as_str())
                .map_or(0, |patterns| {
                    patterns
                        .iter()
                        .map(|pattern| i32::try_from(pattern.len()).unwrap_or(i32::MAX) * 5)
                        .fold(0, i32::saturating_add)
                });
            (
                path_rank(&hit.path, task)
                    .saturating_mul(10)
                    .saturating_add(affinity)
                    .saturating_add(preference_score(&hit.text, preferred_patterns)),
                index,
                hit,
            )
        })
        .collect();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut chosen = Vec::new();
    for (_, _, hit) in ranked {
        if chosen.iter().any(|seen: &SearchHit| {
            seen.path == hit.path && seen.line.abs_diff(hit.line) <= SOURCE_BEFORE
        }) {
            continue;
        }
        chosen.push(hit.clone());
        if chosen.len() == max_files {
            break;
        }
    }
    chosen
}

fn preference_score(text: &str, patterns: &[String]) -> i32 {
    let lower = text.to_ascii_lowercase();
    patterns
        .iter()
        .filter(|pattern| !pattern.is_empty() && lower.contains(pattern.as_str()))
        .map(|pattern| {
            i32::try_from(pattern.len())
                .unwrap_or(i32::MAX)
                .saturating_mul(10)
        })
        .fold(0, i32::saturating_add)
}

fn path_rank(path: &str, task: &str) -> i32 {
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    let task_fold = crate::fold::fold_text(task);
    let wants_ui = task_fold.contains("ui")
        || task_fold.contains("frontend")
        || task_fold.contains("tsx")
        || task_fold.contains("react")
        || task_fold.contains("css");
    let wants_tests = crate::plan_intent::detect(task) == crate::plan_intent::TaskIntent::TestSelection
        || task_fold.contains("test");
    let extension = std::path::Path::new(&lower)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");
    let mut score = 0_i32;
    if matches!(extension, "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "cs") {
        score += 40;
    } else if matches!(extension, "json" | "toml" | "yaml" | "yml") {
        score += 25;
    } else if extension == "md" {
        score -= 30;
    }
    if lower.starts_with("apps/") || lower.starts_with("crates/") {
        score += 20;
    }
    if lower.starts_with("config/") || lower.ends_with("/.env") || lower == ".env" {
        score += 35;
    }
    if lower.starts_with("ui/") || lower.contains("/ui/") {
        score += if wants_ui { 35 } else { -5 };
    }
    let is_test_path = lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.contains(".test.")
        || lower.ends_with("tests.rs")
        || lower.ends_with("_test.py")
        || lower.ends_with("_test.go");
    if is_test_path {
        score += if wants_tests { 35 } else { -15 };
    }
    if lower.contains("/bench/")
        || lower.contains("/cortex-bench/")
        || lower.ends_with("/probe_tasks.rs")
        || lower.ends_with("/tasks.rs")
        || lower.contains("/fixtures/")
        || lower.contains("plan_tests.rs")
        || lower.contains("plan_intent.rs")
        || lower.contains("source_followup.rs")
        || lower.starts_with("docs/")
        || lower.starts_with("readme")
    {
        score -= 50;
    }
    score
}

/// Arguments for one bounded `read_source` call around a hit.
#[must_use]
pub fn read_arguments_with(hit: &SearchHit, token_budget: u32, window: SourceWindow) -> Value {
    let bounded_before = window.before.min(token_budget / 48);
    let start_line = hit.line.saturating_sub(bounded_before).max(1);
    json!({
        "path": hit.path,
        "start_line": start_line,
        "before": 0,
        "after": bounded_before + window.after,
        "token_budget": token_budget.max(200),
    })
}

/// Share of the caller's budget spent on source windows, and the per-file
/// slice once the path list is known.
#[must_use]
pub fn per_file_budget_with(
    budget: u32,
    file_count: usize,
    policy: PlanPolicy,
    window: SourceWindow,
) -> u32 {
    if file_count == 0 {
        return 0;
    }
    let share = budget
        .saturating_mul(window.pool_fifths.clamp(1, 4))
        .wrapping_div(5);
    let mut pool = policy.source_tokens.min(share).max(200);
    if window.pool_fifths > 2 {
        // A widened gather may exceed the policy's normal source allowance —
        // that is the point of widening — but never the budget share itself.
        pool = share.max(200);
    }
    let count = u32::try_from(file_count).unwrap_or(u32::MAX).max(1);
    (pool / count).max(200)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit(path: &str, line: u32) -> SearchHit {
        SearchHit {
            path: path.to_owned(),
            line,
            text: String::new(),
        }
    }

    #[test]
    fn hits_are_deduplicated_by_path_keeping_first_line() {
        let value = json!({
            "matches": [
                {"path": "crates/a/src/a.rs", "line": 10},
                {"path": "crates/b/src/b.rs", "line": 2},
                {"path": "crates/a/src/a.rs", "line": 20},
                {"path": "crates/c/src/c.rs", "line": 1},
            ]
        });
        let hits = hits_from_search(&value);
        let unique = unique_paths_for_patterns(&hits, 2, &[], "");
        assert_eq!(
            unique,
            vec![hit("crates/a/src/a.rs", 10), hit("crates/b/src/b.rs", 2),]
        );
    }

    #[test]
    fn distant_hits_in_one_file_keep_separate_source_windows() {
        let hits = vec![
            hit("crates/a/src/lib.rs", 10),
            hit("crates/a/src/lib.rs", 200),
        ];
        assert_eq!(unique_paths_for_patterns(&hits, 6, &[], ""), hits);
    }

    #[test]
    fn product_rust_outranks_docs_ui_and_bench_fixtures() {
        let hits = vec![
            hit("README.md", 1),
            hit("crates/cortex-bench/src/tasks.rs", 2),
            hit("crates/cortex-bench/src/probe_tasks.rs", 2),
            hit("docs/benchmark.md", 3),
            hit("ui/src/api/client.ts", 22),
            hit("apps/cortex-server/src/main.rs", 207),
            hit("apps/cortex-server/src/library.rs", 39),
        ];
        let unique = unique_paths_for_patterns(&hits, 2, &[], "rename compile_context");
        assert_eq!(unique[0].path, "apps/cortex-server/src/main.rs");
        assert_eq!(unique[1].path, "apps/cortex-server/src/library.rs");
    }

    #[test]
    fn runtime_config_outranks_docs_and_ui() {
        let hits = vec![
            hit("docs/local-models.md", 20),
            hit("ui/src/types.ts", 10),
            hit("config/llm-profiles.json", 15),
        ];
        let unique = unique_paths_for_patterns(&hits, 1, &[], "How does CORTEX_LLM read config?");
        assert_eq!(unique[0].path, "config/llm-profiles.json");
    }

    #[test]
    fn a_frontend_task_keeps_ui_and_a_test_task_keeps_tests() {
        let hits = vec![
            hit("docs/benchmark.md", 1),
            hit("ui/src/api/client.ts", 22),
            hit("crates/cortex-run/src/tests.rs", 10),
        ];
        let ui = unique_paths_for_patterns(&hits, 1, &[], "fix the React frontend in ui/");
        assert_eq!(ui[0].path, "ui/src/api/client.ts");
        let tests = unique_paths_for_patterns(&hits, 1, &[], "which tests should I run?");
        assert_eq!(tests[0].path, "crates/cortex-run/src/tests.rs");
    }

    #[test]
    fn missing_contract_term_outranks_generic_router_matches() {
        let mut generic = hit("apps/cortex-server/src/docs.rs", 10);
        generic.text = "use axum::{Json, Router};".to_owned();
        let mut contract = hit("crates/cortex-mcp/src/llm_route.rs", 99);
        contract.text = "let classification = merge_tiers(lexical, tier);".to_owned();
        let chosen = unique_paths_for_patterns(&[generic, contract], 1, &["merge_".to_owned()], "");
        assert_eq!(chosen[0].path, "crates/cortex-mcp/src/llm_route.rs");
    }

    #[test]
    fn definition_completeness_tracks_brace_balance_not_head_presence() {
        let complete = "pub struct ArchiveOptions {\n  a: u64,\n  b: usize,\n}\n";
        let truncated = "pub struct ArchiveOptions {\n  a: u64,\n";
        let absent = "let options = ArchiveOptions::default();";
        let bodiless = "pub struct Marker;\n";
        let nested = "fn outer() {\n  if x {\n    y();\n  }\n}\nmore text";
        assert_eq!(
            definition_is_complete(complete, "ArchiveOptions"),
            Some(true)
        );
        assert_eq!(
            definition_is_complete(truncated, "ArchiveOptions"),
            Some(false)
        );
        assert_eq!(definition_is_complete(absent, "ArchiveOptions"), None);
        assert_eq!(definition_is_complete(bodiless, "Marker"), Some(true));
        assert_eq!(definition_is_complete(nested, "outer"), Some(true));
    }

    #[test]
    fn definition_head_requires_a_word_boundary() {
        assert!(definition_head_index("pub fn permits(&self)", "permits").is_some());
        assert!(definition_head_index("pub fn permits_all()", "permits").is_none());
        assert!(definition_head_index("permits(&self)", "permits").is_none());
        assert!(
            definition_head_index(
                "export function formatGroupedResult()",
                "formatGroupedResult"
            )
            .is_some()
        );
        assert!(definition_head_index("class Handler {", "Handler").is_some());
        assert!(definition_head_index("def load_rows(self):", "load_rows").is_some());
        assert_eq!(
            definition_is_complete(r#"fn foo() { let s = "{"; }"#, "foo"),
            Some(true),
            "braces inside strings must not unbalance a definition"
        );
    }

    #[test]
    fn broad_tasks_widen_the_source_window() {
        let broad = SourceWindow::for_task(
            "List every mechanism in this crate that can silently cause a miss.",
        );
        let narrow = SourceWindow::for_task("Rename `read_limited` in containers.rs");
        assert!(broad.max_files > narrow.max_files);
        assert!(broad.after > narrow.after);
        assert!(broad.pool_fifths > narrow.pool_fifths);
        assert_eq!(narrow, SourceWindow::default());
    }

    #[test]
    fn widened_pool_may_exceed_policy_source_tokens_but_not_the_budget_share() {
        let policy = PlanPolicy::default();
        let narrow = per_file_budget_with(4_000, 4, policy, SourceWindow::default());
        let wide = per_file_budget_with(
            4_000,
            4,
            policy,
            SourceWindow {
                max_files: 9,
                before: SOURCE_BEFORE,
                after: 84,
                pool_fifths: 3,
            },
        );
        assert!(wide >= narrow);
        assert!(wide <= 4_000 * 3 / 5 / 4 + 200);
    }

    #[test]
    fn read_arguments_open_a_window_around_the_hit() {
        let hit = hit("crates/cortex-mcp/src/http.rs", 63);
        let args = read_arguments_with(&hit, 400, SourceWindow::default());
        assert_eq!(args["path"], "crates/cortex-mcp/src/http.rs");
        assert_eq!(args["start_line"], 55);
        assert_eq!(args["token_budget"], 400);
    }
}

#[cfg(test)]
#[path = "source_followup_live_tests.rs"]
mod live_tests;

#[cfg(test)]
#[path = "source_followup_contract_tests.rs"]
mod contract_tests;
