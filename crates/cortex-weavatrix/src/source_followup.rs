//! Bounded `read_source` follow-up after `search_code`.
//!
//! Search hits name files and lines; the identifiers a contract/transport
//! question needs often sit a few lines away from the match (`compile_skill`,
//! `fn endpoint`). Reading those files whole is the naive arm. This module
//! asks Weavatrix for a window around each hit under a shared token budget.

use serde_json::{Value, json};

use crate::plan::PlanPolicy;

/// Most distinct files to open after a search. Beyond this the follow-up
/// starts to resemble a directory sweep.
pub const MAX_SOURCE_FILES: usize = 6;

/// Lines kept above and below each hit.
///
/// `serve_http` sits ~20 lines above the `/mcp` route registration; keep a
/// generous above-window so the entry point lands in the same read.
pub const SOURCE_BEFORE: u32 = 24;
pub const SOURCE_AFTER: u32 = 48;

/// One search match that can be turned into a `read_source` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: String,
    pub line: u32,
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
        });
    }
    hits
}

/// Deduplicate by path, keeping the earliest line, capped at `max_files`.
///
/// Product source (`.rs` under `apps/` / `crates/`) is preferred over docs,
/// fixtures, and the benchmark's own task list — otherwise the first hits are
/// README noise and the server route that answers the contract question never
/// opens.
#[must_use]
pub fn unique_paths(hits: &[SearchHit], max_files: usize) -> Vec<SearchHit> {
    let mut ranked: Vec<(i32, usize, &SearchHit)> = hits
        .iter()
        .enumerate()
        .map(|(index, hit)| (path_rank(&hit.path), index, hit))
        .collect();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut chosen = Vec::new();
    for (_, _, hit) in ranked {
        if chosen.iter().any(|seen: &SearchHit| seen.path == hit.path) {
            continue;
        }
        chosen.push(hit.clone());
        if chosen.len() == max_files {
            break;
        }
    }
    chosen
}

fn path_rank(path: &str) -> i32 {
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    let extension = std::path::Path::new(&lower)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");
    let mut score = 0_i32;
    if matches!(extension, "rs" | "ts" | "tsx") {
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
    if lower.contains("/bench/")
        || lower.ends_with("/tasks.rs")
        || lower.contains("/fixtures/")
        || lower.contains("plan_tests.rs")
        || lower.contains("plan_intent.rs")
        || lower.contains("source_followup.rs")
        || lower.contains("/tests/")
        || lower.starts_with("docs/")
        || lower.starts_with("ui/")
        || lower.starts_with("readme")
    {
        score -= 50;
    }
    score
}

/// Arguments for one bounded `read_source` call around a hit.
#[must_use]
pub fn read_arguments(hit: &SearchHit, token_budget: u32) -> Value {
    let start_line = hit.line.saturating_sub(SOURCE_BEFORE).max(1);
    json!({
        "path": hit.path,
        "start_line": start_line,
        "before": 0,
        "after": SOURCE_BEFORE + SOURCE_AFTER,
        "token_budget": token_budget.max(200),
    })
}

/// Share of the caller's budget spent on source windows, and the per-file
/// slice once the path list is known.
#[must_use]
pub fn per_file_budget(budget: u32, file_count: usize, policy: PlanPolicy) -> u32 {
    if file_count == 0 {
        return 0;
    }
    let pool = policy
        .source_tokens
        .min(budget.saturating_mul(2) / 5)
        .max(200);
    let count = u32::try_from(file_count).unwrap_or(u32::MAX).max(1);
    (pool / count).max(200)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hits_are_deduplicated_by_path_keeping_first_line() {
        let value = json!({
            "matches": [
                {"path": "crates/a/src/a.rs", "line": 10},
                {"path": "crates/b/src/b.rs", "line": 2},
                {"path": "crates/a/src/a.rs", "line": 99},
                {"path": "crates/c/src/c.rs", "line": 1},
            ]
        });
        let hits = hits_from_search(&value);
        let unique = unique_paths(&hits, 2);
        assert_eq!(
            unique,
            vec![
                SearchHit {
                    path: "crates/a/src/a.rs".to_owned(),
                    line: 10
                },
                SearchHit {
                    path: "crates/b/src/b.rs".to_owned(),
                    line: 2
                },
            ]
        );
    }

    #[test]
    fn product_rust_outranks_docs_ui_and_bench_fixtures() {
        let hits = vec![
            SearchHit {
                path: "README.md".to_owned(),
                line: 1,
            },
            SearchHit {
                path: "crates/cortex-bench/src/tasks.rs".to_owned(),
                line: 2,
            },
            SearchHit {
                path: "docs/benchmark.md".to_owned(),
                line: 3,
            },
            SearchHit {
                path: "ui/src/api/client.ts".to_owned(),
                line: 22,
            },
            SearchHit {
                path: "apps/cortex-server/src/main.rs".to_owned(),
                line: 207,
            },
            SearchHit {
                path: "apps/cortex-server/src/library.rs".to_owned(),
                line: 39,
            },
        ];
        let unique = unique_paths(&hits, 2);
        assert_eq!(unique[0].path, "apps/cortex-server/src/main.rs");
        assert_eq!(unique[1].path, "apps/cortex-server/src/library.rs");
    }

    #[test]
    fn runtime_config_outranks_docs_and_ui() {
        let hits = vec![
            SearchHit {
                path: "docs/local-models.md".to_owned(),
                line: 20,
            },
            SearchHit {
                path: "ui/src/types.ts".to_owned(),
                line: 10,
            },
            SearchHit {
                path: "config/llm-profiles.json".to_owned(),
                line: 15,
            },
        ];
        let unique = unique_paths(&hits, 1);
        assert_eq!(unique[0].path, "config/llm-profiles.json");
    }

    #[test]
    fn read_arguments_open_a_window_around_the_hit() {
        let hit = SearchHit {
            path: "crates/cortex-mcp/src/http.rs".to_owned(),
            line: 63,
        };
        let args = read_arguments(&hit, 400);
        assert_eq!(args["path"], "crates/cortex-mcp/src/http.rs");
        assert_eq!(args["start_line"], 39);
        assert_eq!(args["token_budget"], 400);
    }

    /// Live probe: the skills-compile contract lives on the Rust server route.
    #[test]
    fn source_followup_opens_the_rust_server_for_skills_compile() {
        use crate::plan::PlanPolicy;
        use crate::{WeavatrixAdapter, WeavatrixConfig};
        use std::path::Path;

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        if !root.join("apps/cortex-server/src/main.rs").exists() {
            return;
        }
        let task = "What breaks if the `/api/skills/compile` HTTP contract changes?";
        let planned = crate::plan::plan(task, None, 4_000);
        let search = planned
            .iter()
            .find(|operation| operation.tool == "search_code")
            .expect("contract plan searches");
        let query = search.arguments["query"].as_str().unwrap_or("");
        assert!(
            query == "/api/skills/compile" || query.starts_with("/api/skills/compile|"),
            "search query was {query}"
        );
        assert!(
            !query.split('|').any(|part| part == "HTTP"),
            "HTTP acronym leaked into search: {query}"
        );
        let adapter = WeavatrixAdapter::new(WeavatrixConfig::discover().expect("config"));
        let bundle = adapter
            .prepare_targeted_context_with_source_reads(
                &root,
                task,
                None,
                4_000,
                PlanPolicy::default(),
            )
            .expect("source follow-up bundle");
        assert!(
            bundle
                .evidence
                .iter()
                .all(|fragment| fragment.kind != crate::EvidenceKind::ChangePlan),
            "gathering evidence must not add an unverified change plan"
        );
        let haystack: String = bundle
            .evidence
            .iter()
            .map(|fragment| fragment.content.as_str())
            .collect();
        assert!(
            haystack.contains("compile_skill") || haystack.contains("/api/skills/compile"),
            "expected Rust server contract evidence; query={query}; warnings={:?}; search_head={}",
            bundle.warnings,
            bundle
                .evidence
                .iter()
                .find(|fragment| fragment.id.starts_with("WX-SEARCH"))
                .map(|fragment| fragment.content.chars().take(400).collect::<String>())
                .unwrap_or_default(),
        );
        let compiled = crate::compile_evidence_bundle(bundle, task, 4_000, None)
            .expect("source bundle compiles");
        assert!(
            compiled
                .context
                .included_ids
                .iter()
                .any(|id| id.starts_with("WX-SOURCE")),
            "source evidence must survive the compiler: {:?}",
            compiled.context.omitted_ids
        );
        assert!(!compiled.context.requires_upstream);
    }
}
