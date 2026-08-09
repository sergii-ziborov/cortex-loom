use std::collections::HashMap;

use cortex_domain::{NodeKind, Provenance};
use serde_json::Value;

/// The node kind an author declared with `[kind: review_gate]`.
pub(crate) fn kind_annotation(text: &str) -> Option<NodeKind> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("[kind:")?;
    let tail = &lower[start + 6..];
    let end = tail.find(']')?;
    NodeKind::parse(&tail[..end])
}

/// Remove every `[…]` annotation this compiler owns from a label.
///
/// Import and export both go through here, so a label is the same text in
/// the graph, on the canvas, and after a round trip.
pub(crate) fn strip_annotations(label: &str) -> String {
    let mut result = label.to_owned();
    loop {
        let lower = result.to_ascii_lowercase();
        let Some(start) = ["[depends:", "[kind:"]
            .iter()
            .filter_map(|marker| lower.find(marker))
            .min()
        else {
            break;
        };
        let Some(relative_end) = lower[start..].find(']') else {
            break;
        };
        result.replace_range(start..=start + relative_end, "");
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn dependency_numbers(text: &str) -> Vec<usize> {
    let lower = text.to_ascii_lowercase();
    let mut numbers = Vec::new();
    for prefix in ["depends on step ", "after step "] {
        let mut rest = lower.as_str();
        while let Some(position) = rest.find(prefix) {
            let tail = &rest[position + prefix.len()..];
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(number) = digits.parse() {
                numbers.push(number);
            }
            rest = tail;
        }
    }
    let mut rest = lower.as_str();
    while let Some(position) = rest.find("[depends:") {
        let tail = &rest[position + 9..];
        let Some(end) = tail.find(']') else { break };
        for value in tail[..end].split(',') {
            let digits: String = value
                .trim()
                .trim_start_matches("step")
                .trim()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(number) = digits.parse() {
                numbers.push(number);
            }
        }
        rest = &tail[end + 1..];
    }
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

pub(crate) fn semantic_config(role: &str, order: usize, line: usize) -> HashMap<String, Value> {
    HashMap::from([
        ("role".to_owned(), Value::String(role.to_owned())),
        ("order".to_owned(), Value::from(order)),
        ("sourceLine".to_owned(), Value::from(line)),
    ])
}

pub(crate) fn provenance(source: &str, line: usize, content: &str) -> Provenance {
    Provenance {
        source: source.to_owned(),
        locator: format!("line:{line}"),
        digest: Some(format!("fnv1a64:{:016x}", stable_hash(content))),
    }
}

pub(crate) fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

pub(crate) fn slug(value: &str) -> String {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "skill".to_owned()
    } else {
        slug
    }
}

pub(crate) fn source_stem(source: &str) -> String {
    source
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(source)
        .trim_end_matches(".md")
        .trim_end_matches(".MD")
        .replace(['-', '_'], " ")
}

/// Read one frontmatter scalar.
///
/// A double-quoted value is a JSON-compatible string literal — that is how
/// [`crate::export_skill_markdown`] writes it — so it is parsed back the same
/// way. Stripping the quotes without unescaping would leave the escapes in
/// the value and the next export would escape them again, so each round trip
/// would add a layer (`say "hi"` becoming `say \"hi\"`, then `say \\\"hi\\\"`).
/// Single-quoted values carry no backslash escapes and only lose their
/// delimiters.
pub(crate) fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if value.len() < 2 {
        return value.to_owned();
    }
    match (bytes[0], bytes[value.len() - 1]) {
        (b'"', b'"') => serde_json::from_str::<String>(value)
            .unwrap_or_else(|_| value[1..value.len() - 1].to_owned()),
        (b'\'', b'\'') => value[1..value.len() - 1].to_owned(),
        _ => value.to_owned(),
    }
}
