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
fn append_definition_read_as(
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

/// Second-hop type expansion for enumerating questions.
///
/// A broad question answered from one symbol's neighbourhood stays blind to
/// the types that neighbourhood *uses*: the measured cross-cutting probe had
/// `options.archives.max_expanded_bytes` in every window and still knew
/// nothing about `ArchiveOptions`, because no window contained its
/// definition. This pass scans the source windows already gathered for
/// user-defined type names and reads the definitions of the most-used ones
/// that are not already present in full.
pub(super) fn append_type_expansion_reads(
    engine: &mut Weavatrix,
    evidence: &mut Vec<EvidenceFragment>,
    warnings: &mut Vec<String>,
    task: &str,
    budget: u32,
) {
    const MAX_EXPANSIONS: usize = 4;
    const READS_PER_ROUND: usize = 2;
    const CANDIDATES_PER_ROUND: usize = 6;
    // Two rounds, because the answering type is often visible only through
    // an intermediate: the probe's windows never name `ArchiveOptions` — it
    // surfaces as a field inside `SearchOptions`, whose definition the first
    // round reads. The per-round *read* cap is what reserves slots for that
    // second hop; letting one round spend them all reproduced the original
    // failure with better-looking candidates.
    let mut read = 0_usize;
    let mut tried: Vec<String> = Vec::new();
    for _round in 0..2 {
        if read >= MAX_EXPANSIONS {
            break;
        }
        let picks = expansion_candidates(evidence, task, &tried, CANDIDATES_PER_ROUND);
        if picks.is_empty() {
            break;
        }
        let mut this_round = 0_usize;
        for name in picks {
            if this_round >= READS_PER_ROUND || read >= MAX_EXPANSIONS {
                break;
            }
            tried.push(name.clone());
            // Candidate extraction reads names out of prose and code, so it
            // also proposes fragments of names: a measured run spent a slot
            // on `Archive`, which defines nothing, and never reached
            // `ArchiveOptions`. A name that resolves to no definition costs
            // a lookup, not a slot.
            if append_definition_read_as(
                engine,
                evidence,
                warnings,
                &[],
                &name,
                budget,
                false,
                &format!("WX-TYPE-{}", read + 1),
                EvidenceKind::TypeExpansion,
            ) {
                read += 1;
                this_round += 1;
            }
        }
        if this_round == 0 {
            break;
        }
    }
}

/// The next batch of type names worth a definition read.
///
/// Frequency alone elects the framework types that appear in every
/// signature. A type whose name shares a word with the question outranks
/// them: on the measured probe, `ArchiveOptions` is the one that answers and
/// `SearchOptions` is the one that merely appears.
fn expansion_candidates(
    evidence: &[EvidenceFragment],
    task: &str,
    tried: &[String],
    limit: usize,
) -> Vec<String> {
    let source_text: String = evidence
        .iter()
        .filter(|fragment| {
            matches!(
                fragment.kind,
                EvidenceKind::SourceReads | EvidenceKind::TypeExpansion
            )
        })
        .map(|fragment| fragment.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for candidate in pascal_case_words(&source_text) {
        *counts.entry(candidate).or_default() += 1;
    }
    let task_words: Vec<String> = task
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 5)
        .map(ToOwned::to_owned)
        .collect();
    let mut ranked: Vec<(bool, usize, String)> = counts
        .into_iter()
        .filter(|(name, _)| !tried.iter().any(|seen| seen == name))
        .filter(|(name, _)| {
            !evidence.iter().any(|fragment| {
                crate::source_followup::definition_is_complete(&fragment.content, name)
                    == Some(true)
            })
        })
        .map(|(name, count)| {
            let lower = name.to_ascii_lowercase();
            let affinity = task_words.iter().any(|word| lower.contains(word.as_str()));
            (affinity, count, name)
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            // Longest first on a tie: `Archive` and `ArchiveOptions` both
            // match the question, and only the specific one is a type.
            .then_with(|| right.2.len().cmp(&left.2.len()))
            .then_with(|| left.2.cmp(&right.2))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, _, name)| name)
        .collect()
}

/// User-defined `PascalCase` type names in a code text.
///
/// Standard-library and derive-macro names are noise, not evidence targets.
fn pascal_case_words(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "Self",
        "String",
        "Option",
        "Result",
        "Some",
        "None",
        "Err",
        "Vec",
        "Box",
        "Arc",
        "Rc",
        "Cell",
        "RefCell",
        "PathBuf",
        "Path",
        "HashMap",
        "HashSet",
        "BTreeMap",
        "BTreeSet",
        "Value",
        "Debug",
        "Clone",
        "Copy",
        "Default",
        "PartialEq",
        "Serialize",
        "Deserialize",
        "Read",
        "Write",
        "Cursor",
        "Iterator",
        "Into",
        "From",
        "TryFrom",
        "Send",
        "Sync",
        "Sized",
        "Ord",
        "Eq",
        "Hash",
        "Display",
        "Error",
        "Instant",
        "Duration",
    ];
    let mut words = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            current.push(character);
        } else {
            if is_pascal_case(&current) && !STOP.contains(&current.as_str()) {
                words.push(std::mem::take(&mut current));
            }
            current.clear();
        }
    }
    if is_pascal_case(&current) && !STOP.contains(&current.as_str()) {
        words.push(current);
    }
    words
}

fn is_pascal_case(word: &str) -> bool {
    word.len() >= 6
        && word.chars().next().is_some_and(char::is_uppercase)
        && word.chars().any(char::is_lowercase)
        && !word.contains('_')
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
