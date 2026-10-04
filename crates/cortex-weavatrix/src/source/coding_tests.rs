use super::{SearchHit, SourceWindow, unique_paths_for_patterns};

#[test]
fn coding_task_keeps_the_owning_test_file_among_five_surfaces() {
    let task = "Add Elevated priority; update MCP schema, UI help, and tests";
    let hits = [
        ("crates/core/src/evidence.rs", "pub enum EvidencePriority"),
        ("crates/core/src/lib.rs", "CriticalItemExceedsBudget"),
        ("crates/mcp/src/tools.rs", "\"priority\": {}"),
        ("ui/src/help.ts", "priority help"),
        ("crates/core/src/tests.rs", "fn priority_order()"),
        ("crates/mcp/src/tests.rs", "fn context_compile()"),
        (
            "crates/mcp/src/runtime/http_tests.rs",
            "fn context_compile_http()",
        ),
    ]
    .map(|(path, text)| SearchHit {
        path: path.to_owned(),
        line: 3,
        text: text.to_owned(),
    });
    let chosen = unique_paths_for_patterns(&hits, 5, &[], task);
    assert!(
        chosen
            .iter()
            .any(|hit| hit.path == "crates/core/src/tests.rs"),
        "{chosen:?}"
    );
    assert_eq!(SourceWindow::for_task(task).max_files, 5);
    assert!(
        !chosen
            .iter()
            .any(|hit| hit.path == "crates/mcp/src/tests.rs"),
        "{chosen:?}"
    );
    assert_eq!(SourceWindow::for_task(task).after, 96);
}

#[test]
fn owning_test_head_wins_over_a_later_matching_test() {
    let task = "Add priority and update tests";
    let hits = vec![
        SearchHit {
            path: "crates/core/src/evidence.rs".into(),
            line: 8,
            text: "pub enum EvidencePriority".into(),
        },
        SearchHit {
            path: "crates/core/src/tests.rs".into(),
            line: 62,
            text: "fn priority_rank_test()".into(),
        },
    ];
    let chosen = unique_paths_for_patterns(&hits, 2, &[], task);
    assert!(
        chosen
            .iter()
            .any(|hit| hit.path.ends_with("tests.rs") && hit.line == 1),
        "{chosen:?}"
    );
}
