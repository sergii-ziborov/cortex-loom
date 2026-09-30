use super::{SEARCH_HEADER, extract_text};
use serde_json::json;

#[test]
fn search_results_render_as_grep_lines_and_stay_detectable() {
    let value = json!({
        "matches": [
            {"path": "src/multiline/mod.rs", "line": 56, "text": "    let (lines, matches) = finish_block("},
            {"path": "src/multiline/mod.rs", "line": 76, "text": "    let (lines, matches) = finish_block("}
        ],
        "truncated": false
    });

    let text = extract_text(&value);

    assert!(text.starts_with(SEARCH_HEADER));
    assert!(text.contains("src/multiline/mod.rs:56: let (lines, matches) = finish_block("));
    assert!(!text.contains("\"path\""));
}

#[test]
fn a_relationship_renders_as_one_line_keeping_every_fact() {
    let value = json!({
        "inspection": {
            "node": {
                "kind": "function",
                "label": "finish_block",
                "span": {"file": "src/multiline/mod.rs", "start": {"line": 142}, "end": {"line": 221}}
            },
            "relationships": {
                "neighbors": [{
                    "direction": "outgoing",
                    "relation": "references",
                    "node": {
                        "kind": "struct",
                        "label": "Collector",
                        "span": {"file": "src/collector/mod.rs", "start": {"line": 25}}
                    },
                    "provenance": {
                        "confidence": "high",
                        "extractor": "weavatrix.rust.syn",
                        "span": {"file": "src/multiline/mod.rs", "start": {"line": 149}}
                    }
                }]
            }
        }
    });

    let text = extract_text(&value);

    assert!(text.contains("symbol finish_block (function) src/multiline/mod.rs:142-221"));
    assert!(text.contains(
        "-> references Collector (struct) src/collector/mod.rs:25 via src/multiline/mod.rs:149"
    ));
    // The saving is the point: this used to be ~123 tokens of JSON.
    assert!(text.len() < 220, "rendered too large: {}", text.len());
}

#[test]
fn an_incoming_edge_keeps_its_direction() {
    let value = json!({
        "neighbors": [{
            "direction": "incoming",
            "relation": "calls",
            "node": {
                "kind": "function",
                "label": "search",
                "span": {"file": "src/multiline/mod.rs", "start": {"line": 12}}
            }
        }]
    });

    let text = extract_text(&value);

    assert!(text.contains("<- calls search (function) src/multiline/mod.rs:12"));
}

#[test]
fn source_reads_still_win_over_every_other_rendering() {
    let value = json!({
        "lines": [{"text": "pub struct ArchiveOptions {"}, {"text": "}"}],
        "matches": [{"path": "x.rs", "line": 1, "text": "noise"}]
    });

    assert_eq!(extract_text(&value), "pub struct ArchiveOptions {\n}");
}

#[test]
fn git_history_renders_commit_summaries_before_analytics() {
    let value = json!({
        "analytics": {
            "cochange_pairs": [{"left": "a.rs", "right": "b.rs", "commits": 9}],
            "commits": [{
                "id": "e32b6c87f180727d83f211a109ba6fff64db41d5",
                "summary": "Stop minting Verified and keep probe mechanisms off the engine"
            }]
        }
    });
    let text = extract_text(&value);
    assert!(text.starts_with("commits: 1\n"));
    assert!(text.contains("e32b6c87f180 Stop minting Verified"));
    assert!(!text.contains("cochange_pairs"));
}

#[test]
fn git_blob_renders_path_revision_and_body() {
    let value = json!({
        "path": "crates/sweeploom-cli/src/api.rs",
        "revision": "HEAD~1",
        "kind": "utf8-text",
        "lines": ["pub fn route() {}"],
        "truncated": false
    });
    assert_eq!(
        extract_text(&value),
        "crates/sweeploom-cli/src/api.rs@HEAD~1\npub fn route() {}"
    );
}
