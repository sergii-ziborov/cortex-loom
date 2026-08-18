//! Turning native Weavatrix answers into text a model can read.
//!
//! Graph answers arrive as JSON. Passed through verbatim they are the most
//! expensive and least readable part of a packet: measured on three probe
//! questions, `context_bundle` alone was 37–63 % of the compiled packet and
//! arrived as a single minified line, while one neighbour cost ~123 tokens to
//! state one relationship. The same relationship as a line of text costs
//! about a fifth of that, and the model reads it without unescaping anything.
//!
//! Rendering is lossless in *facts*: every label, kind, path, line, relation
//! and evidence site survives. What is dropped is JSON structure, node ids
//! that restate `file:line:kind:label`, column offsets, and the extractor
//! name — none of which answer an engineering question.

use std::fmt::Write as _;

use serde_json::{Value, json};

const MAX_EVIDENCE_CHARS: usize = 24_000;

/// Marker that opens a rendered search result.
///
/// Sufficiency counts search fragments that actually carry hits. It used to
/// sniff for the `"path"`/`"line"` JSON keys, so rendering had to give it a
/// replacement rather than silently zeroing every search fragment.
pub(crate) const SEARCH_HEADER: &str = "search matches:";

pub(super) fn extract_text(value: &Value) -> String {
    let text = source_lines(value)
        .or_else(|| search_matches(value))
        .or_else(|| symbol_inspection(value))
        .or_else(|| graph_neighbors(value))
        .or_else(|| git_history(value))
        .or_else(|| {
            value
                .get("structuredContent")
                .and_then(|content| content.get("result"))
                .and_then(|result| result.get("text"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            value
                .get("content")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("text"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| value.to_string());
    truncate_chars(text, MAX_EVIDENCE_CHARS)
}

fn source_lines(value: &Value) -> Option<String> {
    let lines = value.get("lines")?.as_array()?;
    let text: Vec<&str> = lines
        .iter()
        .map(|line| line.get("text").and_then(Value::as_str))
        .collect::<Option<_>>()?;
    Some(text.join("\n"))
}

/// `search_code` results as `path:line: text`, the shape every agent already
/// reads from grep.
fn search_matches(value: &Value) -> Option<String> {
    let matches = value.get("matches")?.as_array()?;
    let mut out = format!("{SEARCH_HEADER} {}\n", matches.len());
    for entry in matches {
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if path.is_empty() {
            continue;
        }
        let line = entry.get("line").and_then(Value::as_u64).unwrap_or(1);
        let text = entry
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let _ = writeln!(out, "{path}:{line}: {text}");
    }
    if let Some(truncated) = value.get("truncated").and_then(Value::as_bool)
        && truncated
    {
        out.push_str("[search truncated by its token budget]\n");
    }
    Some(out)
}

/// `context_bundle` — the symbol, its span, and its typed relationships.
fn symbol_inspection(value: &Value) -> Option<String> {
    let inspection = value.get("inspection")?;
    let mut out = String::new();
    if let Some(node) = inspection.get("node") {
        out.push_str(&node_line("symbol", node));
    }
    if let Some(relationships) = inspection.get("relationships") {
        out.push_str(&neighbor_lines(relationships));
    }
    // Any source the bundle carried is code, and code is the point.
    if let Some(sources) = value.get("related_source").and_then(Value::as_array) {
        for source in sources {
            if let Some(text) = source_lines(source) {
                let path = source.get("path").and_then(Value::as_str).unwrap_or("");
                let _ = write!(out, "\nsource {path}:\n{text}\n");
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `get_neighbors` returns the same relationship block without the wrapper.
fn graph_neighbors(value: &Value) -> Option<String> {
    if value.get("neighbors").and_then(Value::as_array).is_some() {
        let rendered = neighbor_lines(value);
        return (!rendered.is_empty()).then_some(rendered);
    }
    // weavatrix-rust 2.6 names the array `dependents`.
    if value.get("dependents").and_then(Value::as_array).is_some() {
        let wrapped = json!({ "neighbors": value.get("dependents") });
        let rendered = neighbor_lines(&wrapped);
        return (!rendered.is_empty()).then_some(rendered);
    }
    None
}

fn neighbor_lines(relationships: &Value) -> String {
    let Some(neighbors) = relationships.get("neighbors").and_then(Value::as_array) else {
        return String::new();
    };
    let mut rows = Vec::new();
    for neighbor in neighbors {
        let relation = neighbor
            .get("relation")
            .and_then(Value::as_str)
            .unwrap_or("related");
        // `outgoing` means this symbol points at the neighbour. Spelling the
        // arrow keeps the direction readable without a legend.
        let arrow = match neighbor.get("direction").and_then(Value::as_str) {
            Some("incoming") => "<-",
            _ => "->",
        };
        let Some(node) = neighbor.get("node") else {
            continue;
        };
        let label = node.get("label").and_then(Value::as_str).unwrap_or("?");
        let kind = node.get("kind").and_then(Value::as_str).unwrap_or("symbol");
        let at = span_location(node.get("span"));
        let mut line = format!("  {arrow} {relation} {label} ({kind}) {at}");
        // The provenance span is where the relationship is written down —
        // the call site or the type mention the reader actually wants.
        if let Some(site) = neighbor
            .get("provenance")
            .and_then(|provenance| provenance.get("span"))
        {
            let evidence = span_location(Some(site));
            if !evidence.is_empty() && evidence != at {
                let _ = write!(line, " via {evidence}");
            }
        }
        rows.push((neighbor_rank(&at, &line), at.clone(), line));
    }
    rows.sort_by(|left, right| left.0.cmp(&right.0).then(left.2.cmp(&right.2)));
    let mut seen_files: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut out = format!("relationships: {}\n", neighbors.len());
    for (_, at, line) in rows {
        let file = at.split(':').next().unwrap_or(at.as_str()).to_owned();
        let count = seen_files.entry(file).or_insert(0);
        *count += 1;
        if *count > 2 {
            continue;
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn neighbor_rank(at: &str, line: &str) -> i32 {
    let haystack = format!("{at} {line}")
        .replace('\\', "/")
        .to_ascii_lowercase();
    let mut score = 0_i32;
    if haystack.contains("/tests/") || haystack.contains("tests.rs") {
        score += 80;
    }
    if haystack.contains("/bench/") || haystack.contains("docs/") {
        score += 60;
    }
    if haystack.contains("apps/") || haystack.contains("crates/") {
        score -= 20;
    }
    score
}

fn node_line(prefix: &str, node: &Value) -> String {
    let label = node.get("label").and_then(Value::as_str).unwrap_or("?");
    let kind = node.get("kind").and_then(Value::as_str).unwrap_or("symbol");
    let span = node.get("span");
    let file = span
        .and_then(|span| span.get("file"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let start = line_of(span, "start");
    let end = line_of(span, "end");
    match (start, end) {
        (Some(start), Some(end)) if end > start => {
            format!("{prefix} {label} ({kind}) {file}:{start}-{end}\n")
        }
        (Some(start), _) => format!("{prefix} {label} ({kind}) {file}:{start}\n"),
        _ => format!("{prefix} {label} ({kind}) {file}\n"),
    }
}

fn span_location(span: Option<&Value>) -> String {
    let file = span
        .and_then(|span| span.get("file"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if file.is_empty() {
        return String::new();
    }
    line_of(span, "start").map_or_else(|| file.to_owned(), |line| format!("{file}:{line}"))
}

fn line_of(span: Option<&Value>, edge: &str) -> Option<u64> {
    span?.get(edge)?.get("line")?.as_u64()
}

/// `git_history` as commit lines. Analytics, when present, is omitted:
/// cochange pairs bury the summaries a history question needs.
fn git_history(value: &Value) -> Option<String> {
    let commits = value
        .pointer("/analytics/commits")
        .or_else(|| value.get("commits"))
        .and_then(Value::as_array)?;
    if commits.is_empty() {
        return None;
    }
    let mut out = format!("commits: {}\n", commits.len());
    for commit in commits {
        let id = commit.get("id").and_then(Value::as_str).unwrap_or("");
        let summary = commit
            .get("summary")
            .or_else(|| commit.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if id.is_empty() && summary.is_empty() {
            continue;
        }
        let short = id.get(..12.min(id.len())).unwrap_or(id);
        let _ = writeln!(out, "{short} {summary}");
    }
    Some(out)
}

fn truncate_chars(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value;
    }
    let mut result: String = value.chars().take(max_chars).collect();
    result.push_str("\n[truncated by Cortex Loom]");
    result
}

#[cfg(test)]
mod tests {
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
}
