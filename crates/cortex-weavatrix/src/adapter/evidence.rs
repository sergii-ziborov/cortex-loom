use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weavatrix_rust::{Weavatrix, operations};

use super::WeavatrixError;
use super::render::extract_text;

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
    /// Second-hop definition of a type the source windows reference.
    ///
    /// Deliberately not `SourceReads`: every source read is critical and a
    /// broad gather can approach the whole budget with them, so a critical
    /// expansion could tip the compile into a fail-closed refusal. Breadth
    /// is best-effort; the budget may drop it.
    TypeExpansion,
    /// Bounded Git history, churn, or co-change from `git_history`.
    GitHistory,
    /// Frames mapped onto files and symbols by `map_stacktrace`.
    StackTrace,
    /// Static test suites selected by `select_tests`.
    TestSelection,
    /// Temporal facts from prior Cortex run events via `memory_context`.
    Memory,
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
    pub window: crate::source_followup::SourceWindow,
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
        plan.window.max_files,
        plan.preferred_patterns,
    );
    if paths.is_empty() {
        warnings.push("source follow-up skipped: search returned no file paths".to_owned());
        return;
    }
    let per_file =
        crate::source_followup::per_file_budget_with(budget, paths.len(), policy, plan.window);
    warnings.push(format!(
        "source follow-up: {} file(s), ~{per_file} tokens each",
        paths.len()
    ));
    for (index, hit) in paths.iter().enumerate() {
        let arguments = crate::source_followup::read_arguments_with(hit, per_file, plan.window);
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

/// Read the named symbol's defining item in full.
///
/// Search windows and the graph's symbol context both clip long bodies; the
/// measured consequence was a packet carrying four of a struct's six fields
/// while sufficiency passed. This read starts at the definition head and
/// widens once when the returned text still does not balance its braces. The
/// fragment lands as `SourceReads`, whose head is critical in the compiler,
/// so a budget that cannot carry the definition fails closed instead of
/// shipping a truncated one.
pub(super) fn append_definition_read(
    engine: &mut Weavatrix,
    evidence: &mut Vec<EvidenceFragment>,
    warnings: &mut Vec<String>,
    search_hits: &[crate::source_followup::SearchHit],
    symbol: &str,
    budget: u32,
    widen: bool,
) {
    append_definition_read_as(
        engine,
        evidence,
        warnings,
        search_hits,
        symbol,
        budget,
        widen,
        "WX-DEF",
        EvidenceKind::SourceReads,
    );
}

/// Returns whether a definition fragment was actually added, so a caller
/// spending a bounded number of expansion slots does not burn one on a name
/// that resolves to nothing.
#[allow(clippy::too_many_arguments)]
pub(super) fn append_definition_read_as(
    engine: &mut Weavatrix,
    evidence: &mut Vec<EvidenceFragment>,
    warnings: &mut Vec<String>,
    search_hits: &[crate::source_followup::SearchHit],
    symbol: &str,
    budget: u32,
    widen: bool,
    id: &str,
    kind: EvidenceKind,
) -> bool {
    if evidence.iter().any(|fragment| {
        fragment.id.starts_with(id)
            && crate::source_followup::definition_is_complete(&fragment.content, symbol)
                == Some(true)
    }) {
        return true;
    }
    let definition_hit = search_hits
        .iter()
        .find(|hit| crate::source_followup::definition_head_index(&hit.text, symbol).is_some())
        .cloned()
        .or_else(|| locate_definition(engine, symbol, warnings));
    let Some(hit) = definition_hit else {
        warnings.push(format!(
            "definition read skipped: no defining hit for {symbol}"
        ));
        return false;
    };
    let token_budget = (budget / 5).max(600);
    let mut after = if widen { 224 } else { 96 };
    for attempt in 0..2 {
        let arguments = json!({
            "path": hit.path,
            "start_line": hit.line.saturating_sub(4).max(1),
            "before": 0,
            "after": after,
            "token_budget": token_budget,
        });
        match native_call(engine, "read_source", arguments) {
            Ok(value) => {
                let text = extract_text(&value);
                let complete =
                    crate::source_followup::definition_is_complete(&text, symbol) == Some(true);
                if complete || attempt == 1 {
                    evidence.retain(|fragment| !fragment.id.starts_with(id));
                    // The label is for the model, not the ledger: packets
                    // render the source into each section header, and a
                    // measured T3 answer left `enabled` unused because
                    // nothing said the fragment WAS the definition of the
                    // type the question turns on.
                    evidence.extend(fragments(
                        id,
                        kind,
                        &format!("weavatrix:read_source definition:{symbol}"),
                        &value,
                    ));
                    if !complete {
                        warnings.push(format!(
                            "definition of {symbol} still unbalanced after a {after}-line window"
                        ));
                    }
                    return true;
                }
                after *= 2;
            }
            Err(error) => {
                warnings.push(format!("definition read unavailable for {symbol}: {error}"));
                return false;
            }
        }
    }
    false
}

/// Find where a symbol is defined when the planned search never hit it.
fn locate_definition(
    engine: &mut Weavatrix,
    symbol: &str,
    warnings: &mut Vec<String>,
) -> Option<crate::source_followup::SearchHit> {
    let escaped = regex_escape(symbol);
    let arguments = json!({
        "query": format!(r"(fn|struct|enum|trait)\s+{escaped}\b"),
        "is_regex": true,
        "max_results": 8,
        "token_budget": 400,
    });
    match native_call(engine, "search_code", arguments) {
        Ok(value) => crate::source_followup::hits_from_search(&value)
            .into_iter()
            .find(|hit| crate::source_followup::definition_head_index(&hit.text, symbol).is_some()),
        Err(error) => {
            warnings.push(format!(
                "definition search unavailable for {symbol}: {error}"
            ));
            None
        }
    }
}

fn regex_escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            let escaped = !character.is_ascii_alphanumeric() && character != '_';
            escaped
                .then_some('\\')
                .into_iter()
                .chain(std::iter::once(character))
        })
        .collect()
}
