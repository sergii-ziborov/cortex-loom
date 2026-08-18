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
        } else if crate::plan_intent::detect(task) == crate::plan_intent::TaskIntent::TestSelection
        {
            // Suite names sit at the top of tests.rs; a 48-line / 216-token
            // slice around the first `compile_context` call starts at line
            // 17 and never reaches either test head.
            Self {
                max_files: 4,
                before: SOURCE_BEFORE,
                after: 80,
                pool_fifths: 3,
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

#[path = "source_hits.rs"]
mod extra_hits;
#[cfg(test)]
pub use extra_hits::sibling_test_hits;
pub use extra_hits::{hits_from_json_paths, hits_from_stack_text, prepend_sibling_test_hits};

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
                    .saturating_add(preference_score(&hit.text, preferred_patterns))
                    .saturating_add(extra_hits::test_suite_head_bonus(hit, task))
                    .saturating_add(extra_hits::same_crate_test_bonus(hit, hits, task)),
                index,
                hit,
            )
        })
        .collect();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut chosen: Vec<SearchHit> = Vec::new();
    for (_, _, hit) in &ranked {
        if let Some(index) = chosen.iter().position(|seen: &SearchHit| {
            seen.path == hit.path && seen.line.abs_diff(hit.line) <= SOURCE_BEFORE
        }) {
            // Overlapping windows: keep the earlier line so a suite file
            // opens from `fn` heads, not from a mid-file call.
            if hit.line < chosen[index].line {
                chosen[index] = (*hit).clone();
            }
            continue;
        }
        chosen.push((*hit).clone());
        if chosen.len() == max_files {
            break;
        }
    }
    reserve_uncovered_patterns(&mut chosen, &ranked, preferred_patterns, max_files);
    chosen
}

/// Keep one window for each preferred term no selected hit carries.
///
/// Ranked gather otherwise spends every slot on `CORTEX_LLM` config/wiring
/// files and never opens `merge_tiers` / `LlmRouter` (measured: contract
/// retry stayed thin after the source pool grew).
fn reserve_uncovered_patterns(
    chosen: &mut Vec<SearchHit>,
    ranked: &[(i32, usize, &SearchHit)],
    preferred_patterns: &[String],
    max_files: usize,
) {
    for pattern in preferred_patterns {
        let needle = pattern.to_ascii_lowercase();
        if needle.is_empty()
            || chosen
                .iter()
                .any(|hit| hit.text.to_ascii_lowercase().contains(&needle))
        {
            continue;
        }
        let Some((_, _, hit)) = ranked.iter().find(|(_, _, hit)| {
            hit.text.to_ascii_lowercase().contains(&needle)
                && !chosen.iter().any(|seen| {
                    seen.path == hit.path && seen.line.abs_diff(hit.line) <= SOURCE_BEFORE
                })
        }) else {
            continue;
        };
        if chosen.len() == max_files {
            chosen.pop();
        }
        chosen.push((*hit).clone());
    }
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
    let wants_tests = crate::plan_intent::detect(task)
        == crate::plan_intent::TaskIntent::TestSelection
        || task_fold.contains("test");
    let extension = std::path::Path::new(&lower)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");
    let mut score = 0_i32;
    if matches!(
        extension,
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "cs"
    ) {
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
    // Fixture lists and docs, not the bench binary itself. `main.rs` is
    // the `compile_probe_bundle` / `cortex_arm` caller the compile-bundle
    // probe has to open; penalising the whole crate drops those facts.
    // Language samples under `fixtures/langs/` *are* the answering source
    // — do not treat them like Markdown skill fixtures or `lang_tasks.rs`.
    let fixture_list = lower.contains("/fixtures/")
        && !matches!(
            extension,
            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "cs"
        );
    if lower.contains("/bench/")
        || lower.ends_with("/probe_tasks.rs")
        || lower.ends_with("/lang_tasks.rs")
        || lower.ends_with("/tasks.rs")
        || fixture_list
        || lower.contains("plan_tests.rs")
        || lower.contains("plan_intent.rs")
        || lower.contains("source_followup.rs")
        || lower.contains("verify_coverage.rs")
        || lower.starts_with("docs/")
        || lower.starts_with("readme")
    {
        score -= 50;
    }
    score
}

/// Arguments for one bounded `read_source` call around a hit.
///
/// A 216-token slice cannot pay for 24 lines of preamble: Weavatrix trims
/// from `start_line`, so the enclosing `fn cortex_arm` (six lines above
/// `match compile_probe_bundle`) never arrived. Spend the slice around the
/// hit, keeping at least eight lines above.
#[must_use]
pub fn read_arguments_with(hit: &SearchHit, token_budget: u32, window: SourceWindow) -> Value {
    let affordable_before = token_budget.saturating_div(48).clamp(8, window.before);
    let start_line = hit.line.saturating_sub(affordable_before).max(1);
    json!({
        "path": hit.path,
        "start_line": start_line,
        "before": 0,
        "after": hit.line.saturating_sub(start_line) + window.after,
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
#[path = "source_followup_unit_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "source_followup_live_tests.rs"]
mod live_tests;

#[cfg(test)]
#[path = "source_followup_contract_tests.rs"]
mod contract_tests;
