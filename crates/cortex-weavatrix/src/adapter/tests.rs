use serde_json::json;

use super::evidence::{
    EvidenceKind, MAX_FRAGMENT_CHARS, fragments, normalize_graph_stats, split_content,
};
use super::render::extract_text;

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
