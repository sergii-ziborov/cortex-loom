//! Graph/AST locators and content-addressed blob hashes.

use cortex_context::{EvidenceLocator, blob_id};
use serde_json::Value;

pub(super) fn locator_from(source: &str, value: &Value) -> EvidenceLocator {
    let mut locator = EvidenceLocator::from_source(source);
    if let Some(path) = value.get("path").and_then(Value::as_str) {
        locator.path = Some(path.to_owned());
    }
    if let Some(start) = as_u32(value.get("start_line")) {
        locator.start_line = Some(start);
    }
    if let Some(end) = as_u32(value.get("end_line")) {
        locator.end_line = Some(end);
    }
    if let Some(hash) = value
        .get("blob_hash")
        .or_else(|| value.get("blobHash"))
        .and_then(Value::as_str)
    {
        locator.blob_hash = Some(hash.to_owned());
    }
    apply_span(&mut locator, value.get("span"));
    apply_span(&mut locator, value.pointer("/inspection/node/span"));
    apply_span(&mut locator, value.pointer("/node/span"));
    apply_lines_range(&mut locator, value);
    locator
}

pub(super) fn apply_blob_hash(locator: &mut EvidenceLocator, content: &str) {
    if locator.blob_hash.is_some() {
        return;
    }
    locator.blob_hash = Some(blob_id(&[
        locator.path.as_deref().unwrap_or_default(),
        &line_token(locator.start_line),
        &line_token(locator.end_line),
        content,
    ]));
}

#[must_use]
pub(super) fn range_covers(got_start: u32, got_end: u32, need_start: u32, need_end: u32) -> bool {
    got_start <= need_start && got_end >= need_end
}

#[must_use]
pub(super) fn lines_range(value: &Value) -> Option<(u32, u32)> {
    let lines = value.get("lines")?.as_array()?;
    let start = as_u32(lines.first()?.get("line"))?;
    let end = as_u32(lines.last()?.get("line"))?;
    Some((start, end))
}

fn apply_span(locator: &mut EvidenceLocator, span: Option<&Value>) {
    let Some(span) = span else {
        return;
    };
    if let Some(file) = span.get("file").and_then(Value::as_str) {
        locator.path = Some(file.to_owned());
    }
    if let Some(start) = span_line(span, "start") {
        locator.start_line = Some(start);
    }
    if let Some(end) = span_line(span, "end") {
        locator.end_line = Some(end);
    }
}

fn apply_lines_range(locator: &mut EvidenceLocator, value: &Value) {
    let Some((start, end)) = lines_range(value) else {
        return;
    };
    if locator.start_line.is_none() {
        locator.start_line = Some(start);
    }
    if locator.end_line.is_none() {
        locator.end_line = Some(end);
    }
}

fn span_line(span: &Value, edge: &str) -> Option<u32> {
    as_u32(span.get(edge)?.get("line"))
}

fn as_u32(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .and_then(|line| u32::try_from(line).ok())
}

fn line_token(line: Option<u32>) -> String {
    line.map_or_else(String::new, |line| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::{locator_from, range_covers};
    use serde_json::json;

    #[test]
    fn weavatrix_node_span_becomes_a_locator() {
        let value = json!({
            "inspection": {
                "node": {
                    "span": {
                        "file": "src/archive.rs",
                        "start": {"line": 12},
                        "end": {"line": 40}
                    }
                }
            }
        });
        let locator = locator_from("weavatrix:context_bundle", &value);
        assert_eq!(locator.path.as_deref(), Some("src/archive.rs"));
        assert_eq!(locator.start_line, Some(12));
        assert_eq!(locator.end_line, Some(40));
    }

    #[test]
    fn read_source_lines_fill_the_span() {
        let value = json!({
            "path": "src/lib.rs",
            "lines": [
                {"line": 10, "text": "fn foo() {"},
                {"line": 14, "text": "}"}
            ]
        });
        let locator = locator_from("weavatrix:read_source", &value);
        assert_eq!(locator.path.as_deref(), Some("src/lib.rs"));
        assert_eq!(locator.start_line, Some(10));
        assert_eq!(locator.end_line, Some(14));
        assert!(range_covers(10, 14, 12, 14));
        assert!(!range_covers(10, 12, 12, 14));
    }
}
