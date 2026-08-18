use serde_json::{Value, json};
use weavatrix_rust::Weavatrix;

use cortex_context::EvidenceFacet;

use super::evidence::{EvidenceFragment, EvidenceKind, budget_overrun, fragments, native_call};
use super::locator::{lines_range, range_covers};

#[derive(Clone, Copy)]
pub(super) struct SourceReadPlan<'a> {
    pub id_prefix: &'a str,
    pub preferred_patterns: &'a [String],
    pub window: crate::source_followup::SourceWindow,
    pub task: &'a str,
}

/// Search implied sibling terms and keep only the hits, not the fragments.
///
/// The first-pass identifier query never sees `MAX_RETRY` / `unquote` /
/// `mcp-session-id`. Adding those hits lets preferred windows land on the
/// sibling file without spending compile budget on another search dump.
pub(super) fn append_implied_coverage_hits(
    engine: &mut Weavatrix,
    search_hits: &mut Vec<crate::source_followup::SearchHit>,
    warnings: &mut Vec<String>,
    task: &str,
    symbol: Option<&str>,
    hints: crate::PlanHints,
) {
    let queries = crate::verify::implied_coverage_queries(task, symbol, hints);
    if queries.is_empty() {
        return;
    }
    warnings.push(format!(
        "implied coverage search: {} quer{}",
        queries.len(),
        if queries.len() == 1 { "y" } else { "ies" }
    ));
    for query in queries {
        let arguments = json!({
            "query": query,
            "is_regex": true,
            "before": 2,
            "after": 2,
            "max_results": 24,
            "glob": "{src,apps,crates,ui,config}/**/*",
            "token_budget": 400,
        });
        match native_call(engine, "search_code", arguments) {
            Ok(value) => {
                search_hits.extend(crate::source_followup::hits_from_search(&value));
            }
            Err(error) => warnings.push(format!("implied coverage search unavailable: {error}")),
        }
    }
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
        plan.task,
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
/// Prefer a Weavatrix graph span when one is already in the bundle. Brace
/// balance is only a last-resort completeness check for languages without
/// a span.
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
        fragment.facet == EvidenceFacet::Definition
            && (fragment.declared_complete == Some(true)
                || crate::definition::definition_complete(&fragment.content, symbol, None)
                    == Some(true))
    }) {
        return true;
    }
    let graph_span = definition_span(evidence);
    let definition_hit = graph_span
        .as_ref()
        .and_then(|locator| {
            locator
                .path
                .as_ref()
                .map(|path| crate::source_followup::SearchHit {
                    path: path.clone(),
                    line: locator.start_line.unwrap_or(1),
                    text: symbol.to_owned(),
                })
        })
        .or_else(|| {
            search_hits
                .iter()
                .find(|hit| {
                    crate::source_followup::definition_head_index(&hit.text, symbol).is_some()
                })
                .cloned()
        })
        .or_else(|| locate_definition(engine, symbol, warnings));
    let Some(hit) = definition_hit else {
        warnings.push(format!(
            "definition read skipped: no defining hit for {symbol}"
        ));
        return false;
    };
    let token_budget = if widen {
        (budget / 3).max(1_200)
    } else {
        (budget / 4).max(800)
    };
    let start_line = graph_span
        .as_ref()
        .and_then(|locator| locator.start_line)
        .unwrap_or_else(|| hit.line.saturating_sub(4).max(1));
    let span_after =
        graph_span
            .as_ref()
            .and_then(|locator| match (locator.start_line, locator.end_line) {
                (Some(start), Some(end)) if end >= start => {
                    Some(end.saturating_sub(start).saturating_add(2))
                }
                _ => None,
            });
    // A short graph span (measured: import_skill_markdown reported 126
    // lines, the item is ~280) must not freeze the window. Ask past it
    // on the first read so targeted, which has no retry, still sees the body.
    let mut after = span_after.unwrap_or(if widen { 224 } else { 96 });
    after = after.max(if widen { 280 } else { 224 });
    for attempt in 0..3 {
        let arguments = json!({
            "path": hit.path,
            "start_line": start_line,
            "before": 0,
            "after": after,
            "token_budget": token_budget,
        });
        match native_call(engine, "read_source", arguments) {
            Ok(value) => {
                let text = super::render::extract_text(&value);
                let complete = definition_complete(&value, &text, symbol, graph_span.as_ref());
                if complete || attempt == 2 {
                    evidence.retain(|fragment| fragment.facet != EvidenceFacet::Definition);
                    let mut added = fragments(
                        id,
                        kind,
                        &format!("weavatrix:read_source definition:{symbol}"),
                        &value,
                    );
                    for fragment in &mut added {
                        fragment.facet = EvidenceFacet::Definition;
                        fragment.declared_complete = Some(complete);
                        fragment.locator.path = Some(hit.path.clone());
                        fragment.locator.start_line = Some(start_line);
                        if let Some(end) = graph_span
                            .as_ref()
                            .and_then(|locator| locator.end_line)
                            .or_else(|| lines_range(&value).map(|(_, end)| end))
                        {
                            fragment.locator.end_line = Some(end);
                        }
                    }
                    evidence.extend(added);
                    if !complete {
                        warnings.push(format!(
                            "definition of {symbol} still incomplete after a {after}-line window"
                        ));
                    }
                    return true;
                }
                after = after.saturating_mul(2);
            }
            Err(error) => {
                warnings.push(format!("definition read unavailable for {symbol}: {error}"));
                return false;
            }
        }
    }
    false
}

fn definition_span(evidence: &[EvidenceFragment]) -> Option<cortex_context::EvidenceLocator> {
    evidence.iter().find_map(|fragment| {
        let locator = &fragment.locator;
        let usable = locator.path.is_some()
            && locator.start_line.is_some()
            && locator.end_line.is_some()
            && (fragment.facet == EvidenceFacet::Definition
                || fragment.kind == EvidenceKind::SymbolContext);
        usable.then(|| locator.clone())
    })
}

fn definition_complete(
    value: &Value,
    text: &str,
    symbol: &str,
    span: Option<&cortex_context::EvidenceLocator>,
) -> bool {
    if crate::definition::definition_complete(text, symbol, span) == Some(true) {
        return true;
    }
    // A short Weavatrix span must not freeze a truncated body as complete.
    if let (Some(span), Some((got_start, got_end))) = (span, lines_range(value))
        && let (Some(need_start), Some(need_end)) = (span.start_line, span.end_line)
        && range_covers(got_start, got_end, need_start, need_end)
        && crate::definition::definition_complete(text, symbol, None) != Some(false)
    {
        return true;
    }
    false
}

fn locate_definition(
    engine: &mut Weavatrix,
    symbol: &str,
    warnings: &mut Vec<String>,
) -> Option<crate::source_followup::SearchHit> {
    let escaped = regex_escape(symbol);
    let arguments = json!({
        "query": format!(r"(fn|struct|enum|trait|type|class|interface|function|def|func)\s+{escaped}\b"),
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
