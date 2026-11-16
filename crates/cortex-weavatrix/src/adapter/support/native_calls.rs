//! Native calls whose `weavatrix-rust` 2.17.17 answer differs from the one
//! Cortex asks for.
//!
//! - `memory_context` left the operation catalog in 2.17.6. The library
//!   (`weavatrix_rust::memory`) still replays the journal and compiles the
//!   same bundle, so Cortex calls it directly.
//! - From 2.17.11 `find_references` answers a label that names several nodes
//!   with `AMBIGUOUS` candidates instead of an error. That answer has no
//!   reference row, so it stays the error it was: the warning lists the
//!   candidate ids and the caller facet stays missing.
//! - From 2.17.9 `search_code` filters the resident index with a glob matcher
//!   that knows `*`, `**`, and `?` but not `{a,b}`. A brace glob matched no
//!   file and returned zero hits. Each alternative is asked separately and the
//!   answers are merged in path order, then fitted to the same budget.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use weavatrix_rust::memory::{
    BytesTokenEstimator, ContextCompiler, ContextRequest, MemoryEvent, StoredEvent, replay_owned,
};
use weavatrix_rust::{Weavatrix, operations};

const MAX_ALTERNATIVES: usize = 32;
const BYTES_PER_TOKEN: usize = 4;
const TOTALS: [&str; 3] = ["matching_lines", "occurrences", "files_with_matches"];

pub(crate) fn call(engine: &mut Weavatrix, name: &str, arguments: Value) -> Result<Value, String> {
    match name {
        "memory_context" => memory_context(&arguments),
        "search_code" => search_code(engine, arguments),
        "find_references" => {
            let report = operations::call(engine, name, arguments)?;
            match ambiguous_label(&report) {
                Some(error) => Err(error),
                None => Ok(report),
            }
        }
        _ => operations::call(engine, name, arguments),
    }
}

fn ambiguous_label(report: &Value) -> Option<String> {
    if report.get("state").and_then(Value::as_str) != Some("AMBIGUOUS") {
        return None;
    }
    let label = report.get("label").and_then(Value::as_str).unwrap_or("?");
    let ids = report
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|candidate| candidate.get("id").and_then(Value::as_str))
        .take(8)
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("ambiguous node label {label:?}; use one of: {ids}"))
}

fn memory_context(arguments: &Value) -> Result<Value, String> {
    let events = serde_json::from_value::<Vec<StoredEvent<MemoryEvent>>>(
        arguments
            .get("events")
            .cloned()
            .ok_or_else(|| "events must be an array".to_owned())?,
    )
    .map_err(|error| format!("invalid memory events: {error}"))?;
    let projection = replay_owned(events).map_err(|error| error.to_string())?;
    let request = serde_json::from_value::<ContextRequest>(
        arguments
            .get("request")
            .cloned()
            .ok_or_else(|| "request must contain a ContextRequest object".to_owned())?,
    )
    .map_err(|error| format!("invalid memory request: {error}"))?;
    let bundle = ContextCompiler::new(BytesTokenEstimator::default())
        .compile(&projection, &request)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "view": bundle.view,
        "graph": bundle.graph,
        "receipt": bundle.receipt,
        "mutation": "NONE"
    }))
}

fn search_code(engine: &mut Weavatrix, arguments: Value) -> Result<Value, String> {
    let alternatives = arguments
        .get("glob")
        .and_then(Value::as_str)
        .and_then(expand_braces)
        .filter(|globs| globs.len() > 1);
    let Some(alternatives) = alternatives else {
        return operations::call(engine, "search_code", arguments);
    };
    let mut parts = Vec::with_capacity(alternatives.len());
    for glob in alternatives {
        let mut asked = arguments.clone();
        asked["glob"] = json!(glob);
        parts.push(operations::call(engine, "search_code", asked)?);
    }
    Ok(merge_search(&parts, &arguments))
}

/// Brace alternatives of `glob`, in order, or `None` when there are more
/// than [`MAX_ALTERNATIVES`]. A glob without a closed group is one item.
pub(crate) fn expand_braces(glob: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    expand_into(glob, &mut out)?;
    let mut seen = BTreeSet::new();
    out.retain(|item| seen.insert(item.clone()));
    Some(out)
}

fn expand_into(glob: &str, out: &mut Vec<String>) -> Option<()> {
    let Some((open, close)) = first_group(glob) else {
        out.push(glob.to_owned());
        return (out.len() <= MAX_ALTERNATIVES).then_some(());
    };
    let (head, body, tail) = (&glob[..open], &glob[open + 1..close], &glob[close + 1..]);
    for choice in split_top_level(body) {
        expand_into(&format!("{head}{choice}{tail}"), out)?;
    }
    Some(())
}

fn first_group(glob: &str) -> Option<(usize, usize)> {
    let open = glob.find('{')?;
    let mut depth = 0_usize;
    for (offset, character) in glob[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, open + offset));
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (offset, character) in body.char_indices() {
        match character {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&body[start..offset]);
                start = offset + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
}

fn match_key(hit: &Value) -> (String, u64) {
    (
        hit.get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        hit.get("line").and_then(Value::as_u64).unwrap_or(0),
    )
}

/// One `search_code` answer from per-alternative answers. Each part is a
/// path-ordered prefix that already fits the budget, so the merged prefix is
/// the one a single brace-aware call would have kept.
pub(crate) fn merge_search(parts: &[Value], arguments: &Value) -> Value {
    let max = arguments
        .get("max_results")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(40);
    let mut merged = parts.first().cloned().unwrap_or_else(|| json!({}));
    let mut matches = Vec::new();
    let mut seen_matches = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut seen_warnings = BTreeSet::new();
    let mut truncated = false;
    let mut dropped = 0_u64;
    let mut totals = [0_u64; 3];
    for part in parts {
        truncated |= part.get("truncated").and_then(Value::as_bool) == Some(true);
        dropped += part
            .pointer("/token_budget/dropped_items")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        for (total, key) in totals.iter_mut().zip(TOTALS) {
            *total += part
                .get("totals")
                .and_then(|totals| totals.get(key))
                .and_then(Value::as_u64)
                .unwrap_or(0);
        }
        for hit in part
            .get("matches")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if seen_matches.insert(match_key(hit)) {
                matches.push(hit.clone());
            }
        }
        for warning in part
            .get("warnings")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if seen_warnings.insert(warning.to_string()) {
                warnings.push(warning.clone());
            }
        }
    }
    matches.sort_by_key(match_key);
    if matches.len() > max {
        matches.truncate(max);
        truncated = true;
    }
    if let Some(object) = merged.as_object_mut() {
        object.remove("token_budget");
        object.insert("warnings".to_owned(), json!(warnings));
    }
    let budget = arguments
        .get("token_budget")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok());
    let mut totals_value = json!({});
    for (total, key) in totals.iter().zip(TOTALS) {
        totals_value[key] = json!(total);
    }
    let receipt = loop {
        totals_value["returned_matches"] = json!(matches.len());
        merged["matches"] = json!(matches);
        merged["totals"] = totals_value.clone();
        merged["truncated"] = json!(truncated);
        let estimated = estimate(&merged);
        let Some(budget) = budget else { break None };
        if estimated <= budget || matches.is_empty() {
            break Some(json!({
                "requested": budget,
                "estimated_tokens": estimated,
                "estimator": "serialized bytes / 4",
                "dropped_items": dropped,
                "fit": estimated <= budget,
                "applied": true
            }));
        }
        matches.pop();
        dropped += 1;
        truncated = true;
    };
    if let (Some(receipt), Some(object)) = (receipt, merged.as_object_mut()) {
        object.insert("token_budget".to_owned(), receipt);
    }
    merged
}

fn estimate(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(0, |bytes| bytes.len().div_ceil(BYTES_PER_TOKEN))
}

#[cfg(test)]
#[path = "native_calls_tests.rs"]
mod tests;
