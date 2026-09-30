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
