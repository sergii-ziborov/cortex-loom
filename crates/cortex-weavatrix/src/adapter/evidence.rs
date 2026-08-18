use serde::{Deserialize, Serialize};
use serde_json::Value;
use weavatrix_rust::{Weavatrix, operations};

use cortex_context::{EvidenceFacet, EvidenceLocator, evidence_id};

use super::WeavatrixError;
use super::locator::{apply_blob_hash, locator_from};
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceBundle {
    pub repository: String,
    pub evidence: Vec<EvidenceFragment>,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
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
    #[serde(default)]
    pub facet: EvidenceFacet,
    #[serde(default)]
    pub locator: EvidenceLocator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_complete: Option<bool>,
}

const fn default_head() -> bool {
    true
}

impl Default for EvidenceFragment {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: EvidenceKind::SearchHits,
            source: String::new(),
            content: String::new(),
            head: true,
            facet: EvidenceFacet::Unspecified,
            locator: EvidenceLocator::default(),
            group_id: None,
            declared_complete: None,
        }
    }
}

impl EvidenceFragment {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        kind: EvidenceKind,
        source: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let id = id.into();
        let source = source.into();
        let locator = EvidenceLocator::from_source(&source);
        Self {
            facet: facet_for(&id, kind),
            id,
            kind,
            locator,
            source,
            content: content.into(),
            ..Self::default()
        }
    }
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
    /// Deliberately not a required definition facet: surrounding windows
    /// stay normal/high, so a broad expansion cannot block compile.
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
    let content = {
        let mut raw = extract_text(value);
        if kind == EvidenceKind::Dependents {
            raw = cap_dependents(&raw, 48);
        }
        // A source window without its path cannot name the crate or file
        // (measured: probe-store crate-name vanished when module_map was
        // omitted). Search already greps as `path:line:`; keep that shape.
        if matches!(
            kind,
            EvidenceKind::SourceReads | EvidenceKind::TypeExpansion
        ) && let Some(path) = value.get("path").and_then(Value::as_str)
            && !path.is_empty()
            && !raw.contains(path)
        {
            raw = format!("{path}\n{raw}");
        }
        raw
    };
    let mut locator = locator_from(source, value);
    apply_blob_hash(&mut locator, &content);
    let facet = facet_for(id, kind);
    let group_id = evidence_id(&[
        id,
        locator.path.as_deref().unwrap_or(source),
        &line_token(locator.start_line),
        &line_token(locator.end_line),
        locator.snapshot_id.as_deref().unwrap_or_default(),
        &content,
    ]);
    let parts = split_content(&content, MAX_FRAGMENT_CHARS);
    let single = parts.len() == 1;
    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| {
            let mut part_locator = locator.clone();
            apply_blob_hash(&mut part_locator, &part);
            EvidenceFragment {
                id: if single {
                    group_id.clone()
                } else {
                    evidence_id(&[&group_id, &index.to_string(), &part])
                },
                kind,
                source: source.to_owned(),
                content: part,
                head: index == 0,
                facet,
                locator: part_locator,
                group_id: Some(group_id.clone()),
                declared_complete: None,
            }
        })
        .collect()
}

fn line_token(line: Option<u32>) -> String {
    line.map_or_else(String::new, |line| line.to_string())
}

pub(super) fn stamp_bundle(bundle: &mut EvidenceBundle, snapshot: &str) {
    bundle.snapshot_id = Some(snapshot.to_owned());
    for fragment in &mut bundle.evidence {
        if fragment.locator.snapshot_id.is_none() {
            fragment.locator.snapshot_id = Some(snapshot.to_owned());
        }
        apply_blob_hash(&mut fragment.locator, &fragment.content);
    }
}

fn facet_for(id: &str, kind: EvidenceKind) -> EvidenceFacet {
    if id.starts_with("WX-DEF") {
        return EvidenceFacet::Definition;
    }
    match kind {
        EvidenceKind::ChangePlan => EvidenceFacet::Plan,
        EvidenceKind::SourceReads => EvidenceFacet::SourceWindow,
        EvidenceKind::SymbolContext => EvidenceFacet::Definition,
        EvidenceKind::Dependents => EvidenceFacet::CallerSignature,
        EvidenceKind::SearchHits | EvidenceKind::TypeExpansion => EvidenceFacet::References,
        EvidenceKind::Memory => EvidenceFacet::Memory,
        EvidenceKind::GraphStats
        | EvidenceKind::ModuleMap
        | EvidenceKind::Endpoints
        | EvidenceKind::GitHistory
        | EvidenceKind::StackTrace
        | EvidenceKind::TestSelection => EvidenceFacet::Structure,
    }
}

fn cap_dependents(content: &str, max_lines: usize) -> String {
    let mut rows: Vec<(i32, String, String)> = content
        .lines()
        .map(|line| {
            let file = line
                .split([' ', ':'])
                .find(|part| part.contains('/') || part.contains('\\'))
                .unwrap_or("")
                .replace('\\', "/");
            let rank = if file.contains("/tests/") || file.ends_with("tests.rs") {
                2
            } else {
                i32::from(file.contains("/bench/") || file.contains("docs/"))
            };
            (rank, file, line.to_owned())
        })
        .collect();
    rows.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut kept = Vec::new();
    for (_, file, line) in rows {
        if kept.len() >= max_lines {
            break;
        }
        let count = seen.entry(file).or_insert(0);
        *count += 1;
        if *count > 2 {
            continue;
        }
        kept.push(line);
    }
    if kept.is_empty() {
        return content.to_owned();
    }
    kept.join("\n")
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
