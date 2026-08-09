use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weavatrix_rust::{Weavatrix, operations};

use super::WeavatrixError;

const MAX_EVIDENCE_CHARS: usize = 24_000;
pub(super) const MAX_FRAGMENT_CHARS: usize = 4_096;

/// Remove per-process telemetry that is not repository evidence.
///
/// Native `graph_stats` reports the cold graph build duration. Including it
/// in an evidence packet makes identical repositories produce different
/// context bytes and token counts on every process start.
pub(super) fn normalize_graph_stats(value: &mut Value) {
    if let Value::Object(fields) = value {
        fields.remove("build_ms");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceBundle {
    pub repository: String,
    pub evidence: Vec<EvidenceFragment>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceFragment {
    pub id: String,
    pub kind: EvidenceKind,
    pub source: String,
    pub content: String,
    #[serde(default = "default_head")]
    pub head: bool,
}

const fn default_head() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    GraphStats,
    ModuleMap,
    ChangePlan,
    SymbolContext,
    SearchHits,
    Dependents,
    Endpoints,
    SourceReads,
}

pub(super) fn fragments(
    id: &str,
    kind: EvidenceKind,
    source: &str,
    value: &Value,
) -> Vec<EvidenceFragment> {
    let content = extract_text(value);
    let parts = split_content(&content, MAX_FRAGMENT_CHARS);
    let single = parts.len() == 1;
    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| EvidenceFragment {
            id: if single {
                id.to_owned()
            } else {
                format!("{id}-{}", index + 1)
            },
            kind,
            source: source.to_owned(),
            content: part,
            head: index == 0,
        })
        .collect()
}

pub(super) fn split_content(content: &str, max_chars: usize) -> Vec<String> {
    if content.chars().count() <= max_chars {
        return vec![content.to_owned()];
    }
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0_usize;
    for paragraph in content.split("\n\n") {
        let mut remaining = paragraph;
        loop {
            let remaining_chars = remaining.chars().count();
            let separator = usize::from(current_chars > 0) * 2;
            if current_chars + separator + remaining_chars <= max_chars {
                if current_chars > 0 {
                    current.push_str("\n\n");
                    current_chars += 2;
                }
                current.push_str(remaining);
                current_chars += remaining_chars;
                break;
            }
            if current_chars > 0 {
                parts.push(std::mem::take(&mut current));
                current_chars = 0;
                continue;
            }
            let boundary = remaining
                .char_indices()
                .nth(max_chars)
                .map_or(remaining.len(), |(offset, _)| offset);
            parts.push(remaining[..boundary].to_owned());
            remaining = &remaining[boundary..];
            if remaining.is_empty() {
                break;
            }
        }
    }
    if current_chars > 0 {
        parts.push(current);
    }
    parts.retain(|part| !part.trim().is_empty());
    if parts.is_empty() {
        parts.push(content.to_owned());
    }
    parts
}

pub(super) fn budget_overrun(tool: &str, value: &Value) -> Option<String> {
    let report = value.get("token_budget")?;
    if report.get("fit").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let requested = report.get("requested").and_then(Value::as_u64)?;
    let estimated = report.get("estimated_tokens").and_then(Value::as_u64)?;
    let dropped = report
        .get("dropped_items")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(format!(
        "{tool} could not fit its budget: asked {requested} tokens, returned \
         {estimated} after dropping {dropped} items; the remainder is lossless \
         evidence it will not discard"
    ))
}

pub(super) fn native_call(
    engine: &mut Weavatrix,
    name: &str,
    arguments: Value,
) -> Result<Value, WeavatrixError> {
    operations::call(engine, name, arguments).map_err(WeavatrixError::Engine)
}

#[derive(Clone, Copy)]
pub(super) struct SourceReadPlan<'a> {
    pub id_prefix: &'a str,
    pub preferred_patterns: &'a [String],
}

pub(super) fn append_source_reads(
    engine: &mut Weavatrix,
    evidence: &mut Vec<EvidenceFragment>,
    warnings: &mut Vec<String>,
    search_hits: &[crate::source_followup::SearchHit],
    budget: u32,
    policy: crate::plan::PlanPolicy,
    plan: SourceReadPlan<'_>,
) {
    let paths = crate::source_followup::unique_paths_for_patterns(
        search_hits,
        crate::source_followup::MAX_SOURCE_FILES,
        plan.preferred_patterns,
    );
    if paths.is_empty() {
        warnings.push("source follow-up skipped: search returned no file paths".to_owned());
        return;
    }
    let per_file = crate::source_followup::per_file_budget(budget, paths.len(), policy);
    warnings.push(format!(
        "source follow-up: {} file(s), ~{per_file} tokens each",
        paths.len()
    ));
    for (index, hit) in paths.iter().enumerate() {
        let arguments = crate::source_followup::read_arguments(hit, per_file);
        match native_call(engine, "read_source", arguments) {
            Ok(value) => {
                if let Some(overrun) = budget_overrun("read_source", &value) {
                    warnings.push(overrun);
                }
                evidence.extend(fragments(
                    &format!("{}-{}", plan.id_prefix, index + 1),
                    EvidenceKind::SourceReads,
                    "weavatrix:read_source",
                    &value,
                ));
            }
            Err(error) => {
                warnings.push(format!("read_source unavailable for {}: {error}", hit.path));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn retry_wide_search(
    engine: &mut Weavatrix,
    evidence: &mut Vec<EvidenceFragment>,
    warnings: &mut Vec<String>,
    search_hits: &mut Vec<crate::source_followup::SearchHit>,
    task: &str,
    symbol: Option<&str>,
    hints: crate::PlanHints,
    missing: &[String],
    budget: u32,
    policy: crate::plan::PlanPolicy,
) {
    let queries = crate::verify::retry_search_queries(task, symbol, hints, missing);
    if queries.is_empty() {
        warnings.push("wide search retry skipped: no missing semantic terms".to_owned());
        return;
    }
    let token_budget = policy
        .search_tokens
        .min(budget.saturating_mul(2) / 5)
        .max(200);
    let query_count = u32::try_from(queries.len()).unwrap_or(u32::MAX).max(1);
    let per_query_budget = (token_budget / query_count).max(200);
    for (index, query) in queries.into_iter().enumerate() {
        let arguments = json!({
            "query": query,
            "is_regex": true,
            "before": 2,
            "after": 2,
            "max_results": 40,
            "glob": "{apps,crates,ui,config}/**/*",
            "token_budget": per_query_budget,
        });
        match native_call(engine, "search_code", arguments) {
            Ok(value) => {
                search_hits.extend(crate::source_followup::hits_from_search(&value));
                evidence.extend(fragments(
                    &format!("WX-RETRY-SEARCH-{}", index + 1),
                    EvidenceKind::SearchHits,
                    "weavatrix:search_code",
                    &value,
                ));
            }
            Err(error) => warnings.push(format!("wide search retry unavailable: {error}")),
        }
    }
}

pub(super) fn extract_text(value: &Value) -> String {
    let structured = value
        .get("structuredContent")
        .and_then(|content| content.get("result"))
        .and_then(|result| result.get("text"))
        .and_then(Value::as_str);
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str);
    let text = structured
        .or(content)
        .map_or_else(|| value.to_string(), str::to_owned);
    truncate_chars(text, MAX_EVIDENCE_CHARS)
}

fn truncate_chars(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value;
    }
    let mut result: String = value.chars().take(max_chars).collect();
    result.push_str("\n[truncated by Cortex Loom]");
    result
}
