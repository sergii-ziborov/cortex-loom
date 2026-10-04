use super::*;

#[test]
fn dependent_cap_reports_every_omitted_row() {
    let raw = (1..=4)
        .map(|line| format!("  <- calls target (function) src/caller.rs:{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let items = fragments(
        "WX-CALL",
        EvidenceKind::Dependents,
        "weavatrix:find_references",
        &json!({"content": [{"text": raw}]}),
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].omitted_rows, 2);
    assert_eq!(items[0].declared_complete, Some(false));
    assert_eq!(items[0].content.lines().count(), 2);
}

#[test]
fn only_delivered_resolved_repository_frames_are_counted() {
    let value = json!({"frames": [
        {"resolved": true, "classification": "repository", "file": "src/app.ts", "line": 8},
        {"resolved": false, "classification": "external", "file": "src/other.rs", "line": 1}
    ]});
    let items = fragments(
        "WX-STACK",
        EvidenceKind::StackTrace,
        "map_stacktrace",
        &value,
    );
    assert_eq!(items[0].resolved_frames, 1);
    assert_eq!(
        resolved_frame_paths(
            &json!({"frames": [{"resolved": false, "file": "src/a.rs", "line": 1}]})
        ),
        Vec::<String>::new()
    );
}

#[test]
fn upstream_page_continuation_keeps_callers_incomplete() {
    let items = fragments(
        "WX-CALL",
        EvidenceKind::Dependents,
        "weavatrix:find_references",
        &json!({
            "state": "RESOLVED",
            "references": [{"role": "reference", "relation": "calls", "node": {"label": "target", "kind": "function"}, "span": {"path": "src/a.rs", "start": {"line": 1}}}],
            "page": {"offset": 0, "returned": 1, "total": 4, "has_more": true, "next_cursor": "v1:1"}
        }),
    );
    assert_eq!(items[0].declared_complete, Some(false));
    assert_eq!(items[0].total_known, Some(4));
    assert_eq!(items[0].continuation.as_deref(), Some("v1:1"));
}

#[test]
fn scoped_ui_search_preserves_hits_under_a_small_budget() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut engine = weavatrix_rust::Weavatrix::open(&root).unwrap();
    let value = native_call(
        &mut engine,
        &root,
        "search_code",
        json!({
            "query": "(?i)\\bpriority\\b", "is_regex": true,
            "glob": "ui/src/**/*.{ts,tsx,js,jsx}", "max_results": 40,
            "before": 1, "after": 1, "token_budget": 800
        }),
    )
    .unwrap();
    let matches = value["matches"].as_array().unwrap();
    assert!(
        matches
            .iter()
            .any(|hit| hit["path"] == "ui/src/components/helpContent.ts"),
        "{value}"
    );
    assert!(
        matches
            .iter()
            .all(|hit| !hit["path"].as_str().unwrap_or("").contains("node_modules"))
    );
}

#[test]
fn unscoped_focus_search_keeps_product_hits_under_a_small_budget() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut engine = weavatrix_rust::Weavatrix::open(&root).unwrap();
    let value = native_call(
        &mut engine,
        &root,
        "search_code",
        json!({
            "query": "(?i)\\bpriority\\b", "is_regex": true,
            "max_results": 40, "before": 1, "after": 1,
            "token_budget": 800
        }),
    )
    .unwrap();
    let matches = value["matches"].as_array().unwrap();
    assert!(
        matches.iter().any(|hit| {
            hit["path"]
                .as_str()
                .is_some_and(|path| path.starts_with("crates/") || path.starts_with("ui/src/"))
        }),
        "{value}"
    );
    assert!(
        matches
            .iter()
            .all(|hit| { !hit["path"].as_str().unwrap_or("").contains("node_modules") })
    );
}
