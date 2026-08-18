#![doc = include_str!("../README.md")]

mod certificate;
mod evidence;
pub mod ranking;
mod tokens;

pub use certificate::{
    ContradictionGroup, CoverageCertificate, FACET_CALLERS, FACET_CONFIG, FACET_DEFAULTS,
    FACET_DEFINITION, FACET_ERRORS, FACET_EXPANSION_ORDER, FACET_GIT, FACET_GUARDS, FACET_MEMORY,
    FACET_PUBLIC_API, FACET_SIGNATURES, FACET_TESTS, FacetClaim, is_critical_facet,
    render_decision_map, render_expansions,
};
pub use evidence::{
    EvidenceDerivation, EvidenceFacet, EvidenceItem, EvidenceLocator, EvidencePriority,
    EvidenceState, blob_id, evidence_id, packet_id, snapshot_is_stale,
};
pub use tokens::{
    CharDiv4Counter, ChatMessage, ConservativeCounter, RUNTIME_COUNTER, TokenBreakdown,
    TokenCounter, estimate_tokens,
};

use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

pub const MAX_EVIDENCE_ITEMS: usize = 4_096;
pub const MAX_EVIDENCE_CHARS: usize = 262_144;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextRequest {
    pub items: Vec<EvidenceItem>,
    pub max_tokens: u32,
    /// Remove lines that a higher-priority item already carried.
    ///
    /// Evidence assembled from several tools overlaps: a search hit and a
    /// symbol excerpt quote the same source lines, and each tool budgets its
    /// own answer without knowing what the others returned. Only the layer
    /// holding every fragment can see the repetition, so this is the one
    /// saving that is not available inside any single tool.
    ///
    /// Conservative by construction: only substantial lines are compared, the
    /// first (highest-priority) occurrence is always the one kept, and an
    /// item that would be emptied is left untouched instead.
    #[serde(default = "enabled")]
    pub deduplicate: bool,
}

const fn enabled() -> bool {
    true
}

/// Lines shorter than this are never deduplicated: a brace, a blank line, or
/// `}` repeats everywhere and removing it would corrupt an excerpt without
/// saving anything worth having.
pub const MIN_DEDUPLICATED_LINE_CHARS: usize = 24;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextPacket {
    pub content: String,
    pub included_ids: Vec<String>,
    pub omitted_ids: Vec<String>,
    /// Estimated tokens of every candidate item, selected or not.
    pub raw_estimated_tokens: u32,
    /// Estimated tokens actually in [`ContextPacket::content`].
    pub selected_estimated_tokens: u32,
    /// Estimated tokens of the candidates left out, i.e.
    /// `raw_estimated_tokens - selected_estimated_tokens`.
    ///
    /// This is **not** a measure of tokens saved. It counts evidence that was
    /// assembled and then dropped to fit the budget, which says nothing about
    /// what a consumer would otherwise have sent. It is zero whenever the
    /// budget fits everything, and it grows as the budget shrinks. Treat it
    /// as an omission volume; to claim a saving you need a measured baseline
    /// of what the alternative actually cost.
    pub omitted_estimated_tokens: u32,
    /// True when any candidate was unverified or contradictory, so the packet
    /// must not be treated as settled.
    pub requires_upstream: bool,
    /// Lines removed because a higher-priority item already carried them.
    ///
    /// Unlike [`ContextPacket::omitted_estimated_tokens`] this **is** a
    /// saving: the content still reaches the consumer, once instead of twice.
    #[serde(default)]
    pub deduplicated_lines: u32,
    /// Estimated tokens those repeated lines would have cost.
    #[serde(default)]
    pub deduplicated_estimated_tokens: u32,
    /// Split accounting for the counter that produced the numbers above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_breakdown: Option<TokenBreakdown>,
    /// Revision-stable handle: `pk_<hash>` of the selected citations and snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_id: Option<String>,
    /// Tree the packet was compiled against, e.g. `git:<commit>+dirty:<digest>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextError {
    EmptyBudget,
    TooManyItems {
        count: usize,
        limit: usize,
    },
    EmptyField {
        index: usize,
        field: &'static str,
    },
    DuplicateId(String),
    ItemTooLarge {
        id: String,
        chars: usize,
        limit: usize,
    },
    CriticalItemExceedsBudget {
        id: String,
        tokens: u32,
        budget: u32,
    },
}

impl Display for ContextError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyBudget => {
                formatter.write_str("context token budget must be greater than zero")
            }
            Self::TooManyItems { count, limit } => {
                write!(
                    formatter,
                    "context has {count} evidence items; limit is {limit}"
                )
            }
            Self::EmptyField { index, field } => {
                write!(formatter, "evidence item {index} has an empty {field}")
            }
            Self::DuplicateId(id) => write!(formatter, "duplicate evidence id: {id}"),
            Self::ItemTooLarge { id, chars, limit } => {
                write!(
                    formatter,
                    "evidence {id} has {chars} characters; limit is {limit}"
                )
            }
            Self::CriticalItemExceedsBudget { id, tokens, budget } => write!(
                formatter,
                "critical evidence {id} needs {tokens} tokens; context budget is {budget}"
            ),
        }
    }
}

impl std::error::Error for ContextError {}

/// Select verified evidence by explicit priority without asking a model to decide what matters.
pub fn compile_context(request: &ContextRequest) -> Result<ContextPacket, ContextError> {
    compile_context_with(request, &RUNTIME_COUNTER)
}

/// Wire-facing compile: a caller cannot mint `Verified`.
///
/// Host-gathered fragments (Weavatrix adapter) still go through
/// [`compile_context`] after the adapter assigned trust from the source.
pub fn distrust_caller_verified(request: &mut ContextRequest) {
    for item in &mut request.items {
        if item.state == EvidenceState::Verified {
            item.state = EvidenceState::Unverified;
            if item.derivation == Some(EvidenceDerivation::ExactSource) {
                item.derivation = Some(EvidenceDerivation::Inferred);
            }
        }
    }
}

/// Same compile, with an explicit counter. Benches that need the historical
/// four-character unit pass [`CharDiv4Counter`].
pub fn compile_context_with(
    request: &ContextRequest,
    counter: &dyn TokenCounter,
) -> Result<ContextPacket, ContextError> {
    validate(request)?;
    let mut ordered: Vec<_> = request.items.iter().enumerate().collect();
    ordered.sort_by(|(left_index, left), (right_index, right)| {
        let state = u8::from(left.state != EvidenceState::Contradictory)
            .cmp(&u8::from(right.state != EvidenceState::Contradictory));
        let priority = left.priority.rank().cmp(&right.priority.rank());
        // Higher relevance first within a band; unscored items keep
        // submission order after scored ones.
        let relevance = right
            .relevance
            .unwrap_or(f64::NEG_INFINITY)
            .total_cmp(&left.relevance.unwrap_or(f64::NEG_INFINITY));
        state
            .then(priority)
            .then(relevance)
            .then(left_index.cmp(right_index))
    });

    let mut content = String::new();
    let mut included_ids = Vec::new();
    let mut omitted_ids = Vec::new();
    let mut seen = HashMap::new();
    let mut deduplicated_lines = 0_u32;
    let mut breakdown = TokenBreakdown::for_counter(counter);
    for (_, item) in &ordered {
        let full = render_item(item, &item.content);
        let candidate = counter.count(&full);
        breakdown.candidate_tokens = breakdown.candidate_tokens.saturating_add(candidate);
        let (body, removed_lines, _) = if request.deduplicate {
            deduplicate_body(item, &seen)
        } else {
            (item.content.clone(), 0, 0)
        };
        let rendered = render_item(item, &body);
        let delivered = counter.count(&rendered);
        if breakdown.delivered_tokens.saturating_add(delivered) <= request.max_tokens {
            content.push_str(&rendered);
            included_ids.push(item.id.clone());
            breakdown.selected_before_dedup_tokens = breakdown
                .selected_before_dedup_tokens
                .saturating_add(candidate);
            breakdown.delivered_tokens = breakdown.delivered_tokens.saturating_add(delivered);
            breakdown.rendering_overhead_tokens = breakdown
                .rendering_overhead_tokens
                .saturating_add(delivered.saturating_sub(counter.count(body.trim())));
            if request.deduplicate {
                record_substantial_lines(item, &mut seen);
                deduplicated_lines = deduplicated_lines.saturating_add(removed_lines);
            }
        } else if item.priority == EvidencePriority::Critical {
            return Err(ContextError::CriticalItemExceedsBudget {
                id: item.id.clone(),
                tokens: delivered,
                budget: request.max_tokens,
            });
        } else {
            omitted_ids.push(item.id.clone());
            breakdown.budget_omitted_tokens =
                breakdown.budget_omitted_tokens.saturating_add(candidate);
        }
    }
    breakdown.dedup_saved_tokens = breakdown
        .selected_before_dedup_tokens
        .saturating_sub(breakdown.delivered_tokens);
    breakdown.estimated_tokens = breakdown.candidate_tokens;
    breakdown.wire_tokens = breakdown.delivered_tokens;
    breakdown.evidence_tokens = breakdown.selected_before_dedup_tokens;
    breakdown.instruction_tokens = breakdown.rendering_overhead_tokens;
    breakdown.schema_tokens = 0;
    breakdown.output_tokens = 0;

    let requires_upstream = request
        .items
        .iter()
        .any(|item| item.state != EvidenceState::Verified);
    let snapshot = shared_snapshot(&request.items);
    let assigned_packet_id = Some(packet_id(&[
        snapshot.as_deref().unwrap_or_default(),
        &included_ids.join("\n"),
    ]));
    Ok(ContextPacket {
        content: content.trim_end().to_owned(),
        included_ids,
        omitted_ids,
        raw_estimated_tokens: breakdown.candidate_tokens,
        selected_estimated_tokens: breakdown.delivered_tokens,
        omitted_estimated_tokens: breakdown.budget_omitted_tokens,
        requires_upstream,
        deduplicated_lines,
        deduplicated_estimated_tokens: breakdown.dedup_saved_tokens,
        token_breakdown: Some(breakdown),
        snapshot_id: snapshot,
        packet_id: assigned_packet_id,
    })
}

fn shared_snapshot(items: &[EvidenceItem]) -> Option<String> {
    let mut shared: Option<&str> = None;
    for item in items {
        let Some(snapshot) = item
            .locator
            .as_ref()
            .and_then(|locator| locator.snapshot_id.as_deref())
        else {
            continue;
        };
        match shared {
            None => shared = Some(snapshot),
            Some(existing) if existing == snapshot => {}
            Some(_) => return None,
        }
    }
    shared.map(ToOwned::to_owned)
}

fn validate(request: &ContextRequest) -> Result<(), ContextError> {
    if request.max_tokens == 0 {
        return Err(ContextError::EmptyBudget);
    }
    if request.items.len() > MAX_EVIDENCE_ITEMS {
        return Err(ContextError::TooManyItems {
            count: request.items.len(),
            limit: MAX_EVIDENCE_ITEMS,
        });
    }
    let mut ids = HashSet::with_capacity(request.items.len());
    for (index, item) in request.items.iter().enumerate() {
        for (field, value) in [
            ("id", item.id.as_str()),
            ("source", item.source.as_str()),
            ("content", item.content.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ContextError::EmptyField { index, field });
            }
        }
        if !ids.insert(item.id.as_str()) {
            return Err(ContextError::DuplicateId(item.id.clone()));
        }
        let chars = item.content.chars().count();
        if chars > MAX_EVIDENCE_CHARS {
            return Err(ContextError::ItemTooLarge {
                id: item.id.clone(),
                chars,
                limit: MAX_EVIDENCE_CHARS,
            });
        }
    }
    Ok(())
}

fn render_item(item: &EvidenceItem, body: &str) -> String {
    format!(
        "<evidence id=\"{}\" trust=\"{}\" source=\"{}\">\n<![CDATA[{}]]>\n</evidence>\n\n",
        xml_escape(&item.id),
        xml_escape(&item.heading_label()),
        xml_escape(&item.source),
        cdata_escape(body.trim())
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn cdata_escape(value: &str) -> String {
    value.replace("]]>", "]]]]><![CDATA[>")
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DedupKey {
    line: String,
    state: EvidenceState,
    derivation: Option<EvidenceDerivation>,
    path: Option<String>,
    start_line: Option<u32>,
    end_line: Option<u32>,
    snapshot_id: Option<String>,
    blob_hash: Option<String>,
}

fn dedup_key(item: &EvidenceItem, line: &str) -> DedupKey {
    let locator = item.locator.as_ref();
    DedupKey {
        line: line.to_owned(),
        state: item.state,
        derivation: item.derivation,
        path: locator.and_then(|locator| locator.path.clone()),
        start_line: locator.and_then(|locator| locator.start_line),
        end_line: locator.and_then(|locator| locator.end_line),
        snapshot_id: locator.and_then(|locator| locator.snapshot_id.clone()),
        blob_hash: locator.and_then(|locator| locator.blob_hash.clone()),
    }
}

/// Drop a line only when span, trust, derivation, and snapshot all match.
/// A match becomes a provenance pointer, not a silent deletion.
fn deduplicate_body(item: &EvidenceItem, seen: &HashMap<DedupKey, String>) -> (String, u32, usize) {
    let mut kept = Vec::new();
    let mut dropped = 0_u32;
    let mut removed_chars = 0_usize;
    let mut cited = HashSet::new();
    for line in item.content.lines() {
        let trimmed = line.trim();
        if trimmed.chars().count() < MIN_DEDUPLICATED_LINE_CHARS {
            kept.push(line.to_owned());
            continue;
        }
        if let Some(first) = seen.get(&dedup_key(item, trimmed)) {
            if cited.insert(first.clone()) {
                kept.push(format!("same source span as [{first}]"));
            }
            dropped = dropped.saturating_add(1);
            removed_chars = removed_chars.saturating_add(line.chars().count().saturating_add(1));
        } else {
            kept.push(line.to_owned());
        }
    }
    if kept.iter().all(|line| line.trim().is_empty()) {
        return (item.content.clone(), 0, 0);
    }
    (kept.join("\n"), dropped, removed_chars)
}

fn record_substantial_lines(item: &EvidenceItem, seen: &mut HashMap<DedupKey, String>) {
    for line in item.content.lines() {
        let trimmed = line.trim();
        if trimmed.chars().count() >= MIN_DEDUPLICATED_LINE_CHARS {
            seen.entry(dedup_key(item, trimmed))
                .or_insert_with(|| item.id.clone());
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
