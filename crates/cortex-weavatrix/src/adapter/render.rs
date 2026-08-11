use serde_json::Value;

const MAX_EVIDENCE_CHARS: usize = 24_000;

pub(super) fn extract_text(value: &Value) -> String {
    let source = source_lines(value);
    let structured = value
        .get("structuredContent")
        .and_then(|content| content.get("result"))
        .and_then(|result| result.get("text"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let text = source
        .or(structured)
        .or(content)
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

fn truncate_chars(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value;
    }
    let mut result: String = value.chars().take(max_chars).collect();
    result.push_str("\n[truncated by Cortex Loom]");
    result
}
