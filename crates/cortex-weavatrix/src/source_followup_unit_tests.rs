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
fn language_samples_outrank_the_lang_task_list() {
    let hits = vec![
        hit("crates/cortex-bench/src/lang_tasks.rs", 12),
        hit("crates/cortex-bench/fixtures/langs/retry.py", 4),
    ];
    let unique = unique_paths_for_patterns(&hits, 1, &[], "How does schedule_py_retry cap?");
    assert_eq!(
        unique[0].path, "crates/cortex-bench/fixtures/langs/retry.py",
        "lang fixture source must beat the task list: {unique:?}"
    );
}

#[test]
fn a_bench_caller_is_kept_when_fixture_lists_are_not() {
    let mut caller = hit("crates/cortex-bench/src/main.rs", 330);
    caller.text =
        "match compile_probe_bundle(bundle, task.prompt, settings.budget, None) {".to_owned();
    let hits = vec![
        hit("crates/cortex-bench/src/tasks.rs", 2),
        hit("crates/cortex-bench/src/probe_tasks.rs", 2),
        caller,
        hit("crates/cortex-weavatrix/src/context.rs", 34),
    ];
    let unique = unique_paths_for_patterns(
        &hits,
        2,
        &["compile_probe_bundle".to_owned()],
        "Who calls compile_probe_bundle",
    );
    assert_eq!(unique[0].path, "crates/cortex-bench/src/main.rs");
    assert_eq!(unique[1].path, "crates/cortex-weavatrix/src/context.rs");
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
fn an_uncovered_preferred_term_displaces_the_lowest_window() {
    let mut config = hit("crates/cortex-llm/src/config.rs", 10);
    config.text = "enabled: non_empty(\"CORTEX_LLM\")".to_owned();
    let mut extra = hit("crates/cortex-mcp/src/lib.rs", 20);
    extra.text = "mod llm_route;".to_owned();
    let mut merge = hit("crates/cortex-mcp/src/llm_route.rs", 99);
    merge.text = "let classification = merge_tiers(lexical, tier);".to_owned();
    let chosen = unique_paths_for_patterns(
        &[config, extra, merge],
        2,
        &["merge_".to_owned()],
        "How does CORTEX_LLM wire route_work?",
    );
    assert!(
        chosen.iter().any(|hit| hit.path.ends_with("llm_route.rs")),
        "merge_tiers must keep a window: {chosen:?}"
    );
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

#[test]
fn a_small_per_file_budget_still_keeps_the_above_window() {
    let hit = hit("crates/cortex-bench/src/main.rs", 330);
    let args = read_arguments_with(&hit, 216, SourceWindow::default());
    let start = args["start_line"].as_u64().unwrap();
    assert!(
        (322..=324).contains(&start),
        "start must sit just above cortex_arm (324): {args}"
    );
}
