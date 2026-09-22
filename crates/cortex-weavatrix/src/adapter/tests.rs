use serde_json::json;

use super::evidence::{
    EvidenceKind, MAX_FRAGMENT_CHARS, bind_expected_repository, expected_repository_arg, fragments,
    normalize_graph_stats, split_content,
};
use super::render::extract_text;
use super::source_reads::hit_from_inspect;

#[test]
fn graph_stats_drop_volatile_build_latency_before_context_compilation() {
    let mut first = json!({"nodes": 42, "build_ms": 11.234_567_89});
    let mut second = json!({"nodes": 42, "build_ms": 987.2});
    normalize_graph_stats(&mut first);
    normalize_graph_stats(&mut second);
    assert_eq!(first, second);
    assert_eq!(first, json!({"nodes": 42}));
}

#[test]
fn extracts_structured_text_before_fallback_content() {
    let value = json!({
        "content": [{"type": "text", "text": "fallback"}],
        "structuredContent": {"result": {"text": "structured"}}
    });
    assert_eq!(extract_text(&value), "structured");
}

#[test]
fn read_source_lines_become_plain_source_instead_of_json() {
    let value = json!({
        "path": "src/options/types.rs",
        "lines": [
            {"line": 87, "text": "pub struct ArchiveOptions {"},
            {"line": 88, "text": "    pub enabled: bool,"},
            {"line": 89, "text": "}"}
        ]
    });

    assert_eq!(
        extract_text(&value),
        "pub struct ArchiveOptions {\n    pub enabled: bool,\n}"
    );
    let parts = fragments(
        "WX-SOURCE",
        EvidenceKind::SourceReads,
        "weavatrix:read_source",
        &value,
    );
    assert!(
        parts[0].content.starts_with("src/options/types.rs\n"),
        "source window must name its file: {}",
        parts[0].content
    );
}

#[test]
fn small_results_keep_the_bare_citation_id() {
    let value = json!({"content": [{"type": "text", "text": "short plan"}]});
    let parts = fragments("WX-VERIFY", EvidenceKind::ChangePlan, "weavatrix:v", &value);
    assert_eq!(parts.len(), 1);
    assert!(parts[0].id.starts_with("ev_"));
    assert_eq!(
        parts[0].id,
        fragments("WX-VERIFY", EvidenceKind::ChangePlan, "weavatrix:v", &value)[0].id
    );
}

#[test]
fn oversized_results_split_into_stable_ordered_sub_citations() {
    let paragraphs: Vec<String> = (0..8)
        .map(|index| format!("paragraph {index} {}", "x".repeat(900)))
        .collect();
    let text = paragraphs.join("\n\n");
    let value = json!({"content": [{"type": "text", "text": text}]});
    let parts = fragments("WX-VERIFY", EvidenceKind::ChangePlan, "weavatrix:v", &value);
    assert!(parts.len() > 1, "must split: {}", parts.len());
    let group = parts[0].group_id.clone();
    for part in &parts {
        assert!(part.id.starts_with("ev_"));
        assert_eq!(part.group_id, group);
        assert!(part.content.chars().count() <= MAX_FRAGMENT_CHARS);
        assert_eq!(part.kind, EvidenceKind::ChangePlan);
    }
    let rejoined = parts
        .iter()
        .map(|part| part.content.clone())
        .collect::<Vec<_>>()
        .join("\n\n");
    assert_eq!(rejoined, text, "splitting loses no content");
    assert_eq!(
        parts,
        fragments("WX-VERIFY", EvidenceKind::ChangePlan, "weavatrix:v", &value),
        "splitting is deterministic"
    );
}

#[test]
fn a_single_oversized_paragraph_is_hard_split_without_loss() {
    let text = "y".repeat(MAX_FRAGMENT_CHARS * 2 + 100);
    let parts = split_content(&text, MAX_FRAGMENT_CHARS);
    assert_eq!(parts.len(), 3);
    assert_eq!(parts.concat(), text);
}

#[test]
fn expected_repository_is_bound_once_and_keeps_a_caller_value() {
    use std::path::Path;

    let root = Path::new(r"C:\repo");
    let mut empty = json!({});
    bind_expected_repository(&mut empty, root);
    assert_eq!(
        empty["expected_repository"].as_str(),
        Some(expected_repository_arg(root).as_str())
    );

    let mut pinned = json!({ "expected_repository": "other" });
    bind_expected_repository(&mut pinned, root);
    assert_eq!(pinned["expected_repository"], "other");
}

#[test]
fn inspect_symbol_payload_becomes_a_definition_hit() {
    let value = json!({
        "inspection": {
            "node": {
                "label": "compile_context",
                "span": {
                    "file": "crates/cortex-context/src/lib.rs",
                    "start": { "line": 44 }
                }
            }
        }
    });
    let hit = hit_from_inspect(&value, "compile_context").expect("span");
    assert_eq!(hit.path, "crates/cortex-context/src/lib.rs");
    assert_eq!(hit.line, 44);
}
