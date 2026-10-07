//! Bounded `read_source` follow-up after `search_code`.
//!
//! Search hits name files and lines; the identifiers a contract/transport
//! question needs often sit a few lines away from the match (`compile_skill`,
//! `fn endpoint`). Reading those files whole is the naive arm. This module
//! asks Weavatrix for a window around each hit under a shared token budget.

use serde_json::{Value, json};

use crate::plan::PlanPolicy;

mod coding;
mod definition;
mod owner;
mod window;
pub use definition::{definition_head_index, definition_is_complete};
pub(crate) use owner::{coding_owner_hit, owner_budget};
use window::schema_key_mentioned;
pub use window::{SOURCE_BEFORE, SourceWindow};

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

#[cfg(test)]
mod coding_tests;
mod hits;
pub use hits::already_a_test_path as is_test_path;
#[cfg(test)]
pub use hits::sibling_test_hits;
pub use hits::{
    hits_from_health_report, hits_from_json_paths, hits_from_stack_text, prepend_named_source_hits,
    prepend_sibling_test_hits, retain_product_search_matches,
};

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
    let hits = hits::keep_product_hits(hits, task);
    let distinct_files = crate::plan::is_new_feature_without_owner(task);
    let schema_in_mcp = distinct_files
        && task.to_ascii_lowercase().contains("mcp")
        && task.to_ascii_lowercase().contains("schema");
    let mut preferred_paths: std::collections::HashMap<&str, std::collections::HashSet<&str>> =
        std::collections::HashMap::new();
    for hit in &hits {
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
                    .saturating_add(
                        if distinct_files
                            && !hits::already_a_test_path(&hit.path)
                            && declaration_hit(&hit.text)
                        {
                            500
                        } else {
                            0
                        },
                    )
                    .saturating_add(
                        if schema_in_mcp
                            && hit.path.contains("mcp")
                            && schema_key_mentioned(task, &hit.text)
                        {
                            300
                        } else {
                            0
                        },
                    )
                    .saturating_add(affinity)
                    .saturating_add(preference_score(&hit.text, preferred_patterns))
                    .saturating_add(hits::test_suite_head_bonus(hit, task))
                    .saturating_add(hits::same_crate_test_bonus(hit, &hits, task))
                    .saturating_add(hits::coding_test_bonus(hit, &hits, task)),
                index,
                hit,
            )
        })
        .collect();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut chosen: Vec<SearchHit> = Vec::new();
    for (_, _, hit) in &ranked {
        if distinct_files && chosen.iter().any(|seen| seen.path == hit.path) {
            continue;
        }
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
    reserve_uncovered_patterns(
        &mut chosen,
        &ranked,
        preferred_patterns,
        max_files,
        distinct_files,
    );
    hits::keep_suite_head_and_one_later(&mut chosen, task);
    coding::open_coding_tests_from_head(&mut chosen, task);
    chosen
}

fn declaration_hit(text: &str) -> bool {
    let line = text.trim_start();
    [
        "enum ",
        "struct ",
        "class ",
        "type ",
        "trait ",
        "interface ",
        "fn ",
        "def ",
    ]
    .iter()
    .any(|head| line.starts_with(head) || line.starts_with(&format!("pub {head}")))
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
    distinct_files: bool,
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
                    seen.path == hit.path
                        && (distinct_files || seen.line.abs_diff(hit.line) <= SOURCE_BEFORE)
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
    let wants_ui = crate::plan_intent::asks_for_ui(task);
    let wants_tests = crate::plan_intent::detect(task)
        == crate::plan_intent::TaskIntent::TestSelection
        || crate::plan_intent::asks_for_test_source(task);
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
    if task_fold.contains("mcp")
        && task_fold.contains("schema")
        && lower.split('/').any(|part| part.contains("mcp"))
    {
        score += 10;
    }
    if (lower.starts_with("benchmarks/") || lower.split('/').any(|part| part.ends_with("-bench")))
        && !task_fold.contains("bench")
        && crate::plan_intent::is_coding_change(task)
        && crate::plan::extract_identifiers(task).is_empty()
    {
        score -= 60;
    }
    if lower.starts_with("config/") || lower.ends_with("/.env") || lower == ".env" {
        score += 35;
    }
    if lower.starts_with("ui/") || lower.contains("/ui/") {
        score += if wants_ui { 65 } else { -5 };
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
    // Ask catalogs and docs, not the bench binary. Language samples under
    // `fixtures/langs/` stay; Markdown skill fixtures and `agent_cases.rs`
    // do not.
    let markdown_fixture = lower.contains("/fixtures/")
        && !matches!(
            extension,
            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "cs"
        );
    if hits::fixture_task_list(&normalized)
        || markdown_fixture
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
    let mut per_file = (pool / count).max(200);
    if window.max_per_file > 0 {
        per_file = per_file.min(window.max_per_file).max(200);
    }
    per_file
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod live_tests;

#[cfg(test)]
mod contract_tests;
