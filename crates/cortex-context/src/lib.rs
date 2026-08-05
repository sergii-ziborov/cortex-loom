#![doc = include_str!("../README.md")]

pub mod ranking;

use std::collections::HashSet;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

pub const MAX_EVIDENCE_ITEMS: usize = 4_096;
pub const MAX_EVIDENCE_CHARS: usize = 262_144;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidencePriority {
    Critical,
    High,
    Normal,
    Low,
}

impl EvidencePriority {
    const fn rank(self) -> u8 {
        match self {
            Self::Critical => 0,
            Self::High => 1,
            Self::Normal => 2,
            Self::Low => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Verified,
    Unverified,
    Contradictory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceItem {
    pub id: String,
    pub source: String,
    pub content: String,
    pub priority: EvidencePriority,
    pub state: EvidenceState,
    /// Optional relevance score, typically from a retrieval ranking.
    ///
    /// It only reorders items **within** the same trust/priority band:
    /// contradiction handling, priority, and fail-closed criticality always
    /// dominate. Higher scores come first. Within a band, every scored item
    /// precedes every unscored one, and unscored items keep their submission
    /// order relative to each other â€” so scoring part of a band promotes
    /// those items over the rest of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance: Option<f64>,
}

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
    validate(request)?;
    let raw_estimated_tokens = request
        .items
        .iter()
        .map(|item| estimate_tokens(&render_item(item, &item.content)))
        .fold(0_u32, u32::saturating_add);
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

    // Deduplicate in priority order so the first, most trusted occurrence of
    // a line is the one that survives.
    let (bodies, deduplicated_lines, deduplicated_estimated_tokens) = if request.deduplicate {
        deduplicate(&ordered)
    } else {
        (
            ordered
                .iter()
                .map(|(_, item)| item.content.clone())
                .collect(),
            0,
            0,
        )
    };

    let mut content = String::new();
    let mut included_ids = Vec::new();
    let mut omitted_ids = Vec::new();
    let mut selected_estimated_tokens = 0_u32;
    for ((_, item), body) in ordered.iter().zip(bodies) {
        let rendered = render_item(item, &body);
        let tokens = estimate_tokens(&rendered);
        if selected_estimated_tokens.saturating_add(tokens) <= request.max_tokens {
            content.push_str(&rendered);
            included_ids.push(item.id.clone());
            selected_estimated_tokens = selected_estimated_tokens.saturating_add(tokens);
        } else if item.priority == EvidencePriority::Critical {
            return Err(ContextError::CriticalItemExceedsBudget {
                id: item.id.clone(),
                tokens,
                budget: request.max_tokens,
            });
        } else {
            omitted_ids.push(item.id.clone());
        }
    }

    let requires_upstream = request
        .items
        .iter()
        .any(|item| item.state != EvidenceState::Verified);
    Ok(ContextPacket {
        content: content.trim_end().to_owned(),
        included_ids,
        omitted_ids,
        raw_estimated_tokens,
        selected_estimated_tokens,
        omitted_estimated_tokens: raw_estimated_tokens.saturating_sub(selected_estimated_tokens),
        requires_upstream,
        deduplicated_lines,
        deduplicated_estimated_tokens,
    })
}

#[must_use]
pub fn estimate_tokens(value: &str) -> u32 {
    let tokens = value.chars().count().div_ceil(4).max(1);
    u32::try_from(tokens).unwrap_or(u32::MAX)
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
    format!("## [{}] {}\n{}\n\n", item.id, item.source, body.trim())
}

/// Drop lines an earlier, higher-priority item already carried.
///
/// Returns one body per ordered item, the number of lines removed, and what
/// those lines would have cost. An item whose every substantial line is a
/// repeat keeps its original body: a citation that renders as nothing is
/// worse than a citation that repeats something.
fn deduplicate(ordered: &[(usize, &EvidenceItem)]) -> (Vec<String>, u32, u32) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut bodies = Vec::with_capacity(ordered.len());
    let mut removed_lines = 0_u32;
    let mut removed_chars = 0_usize;
    for (_, item) in ordered {
        let mut kept = Vec::new();
        let mut dropped = Vec::new();
        for line in item.content.lines() {
            let trimmed = line.trim();
            if trimmed.chars().count() < MIN_DEDUPLICATED_LINE_CHARS {
                kept.push(line);
                continue;
            }
            if seen.insert(trimmed.to_owned()) {
                kept.push(line);
            } else {
                dropped.push(line);
            }
        }
        if kept.iter().all(|line| line.trim().is_empty()) {
            bodies.push(item.content.clone());
            continue;
        }
        removed_lines =
            removed_lines.saturating_add(u32::try_from(dropped.len()).unwrap_or(u32::MAX));
        removed_chars += dropped
            .iter()
            .map(|line| line.chars().count() + 1)
            .sum::<usize>();
        bodies.push(kept.join("\n"));
    }
    let removed_tokens = u32::try_from(removed_chars.div_ceil(4)).unwrap_or(u32::MAX);
    (bodies, removed_lines, removed_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, content: &str, priority: EvidencePriority) -> EvidenceItem {
        EvidenceItem {
            id: id.to_owned(),
            source: format!("src/{id}.rs:1"),
            content: content.to_owned(),
            priority,
            state: EvidenceState::Verified,
            relevance: None,
        }
    }

    #[test]
    fn selects_priority_order_and_reports_token_savings() {
        let request = ContextRequest {
            items: vec![
                item("low", &"x".repeat(80), EvidencePriority::Low),
                item("high", "important", EvidencePriority::High),
            ],
            max_tokens: 10,
            deduplicate: true,
        };
        let packet = compile_context(&request).unwrap();
        assert_eq!(packet.included_ids, ["high"]);
        assert_eq!(packet.omitted_ids, ["low"]);
        assert!(packet.omitted_estimated_tokens > 0);
        assert!(!packet.requires_upstream);
    }

    #[test]
    fn contradictory_evidence_is_first_and_forces_upstream_review() {
        let mut contradiction = item("conflict", "A conflicts with B", EvidencePriority::Low);
        contradiction.state = EvidenceState::Contradictory;
        let request = ContextRequest {
            items: vec![
                item("normal", "normal evidence", EvidencePriority::Normal),
                contradiction,
            ],
            max_tokens: 100,
            deduplicate: true,
        };
        let packet = compile_context(&request).unwrap();
        assert_eq!(packet.included_ids[0], "conflict");
        assert!(packet.requires_upstream);
    }

    #[test]
    fn critical_evidence_never_disappears_silently() {
        let request = ContextRequest {
            items: vec![item(
                "critical",
                &"x".repeat(200),
                EvidencePriority::Critical,
            )],
            max_tokens: 1,
            deduplicate: true,
        };
        assert!(matches!(
            compile_context(&request),
            Err(ContextError::CriticalItemExceedsBudget { .. })
        ));
    }

    #[test]
    fn relevance_reorders_only_within_a_priority_band() {
        // Two Normal items under a budget that fits one: the scored,
        // more relevant later item survives instead of the earlier one.
        let mut early = item("early", &"x".repeat(80), EvidencePriority::Normal);
        early.relevance = Some(0.2);
        let mut late = item("late", &"y".repeat(80), EvidencePriority::Normal);
        late.relevance = Some(0.9);
        let request = ContextRequest {
            items: vec![early.clone(), late.clone()],
            max_tokens: 30,
            deduplicate: true,
        };
        let packet = compile_context(&request).unwrap();
        assert_eq!(packet.included_ids, ["late"]);
        assert_eq!(packet.omitted_ids, ["early"]);

        // A High-priority item with low relevance still beats a highly
        // relevant Normal item: policy dominates semantics.
        let mut high = item("high", &"h".repeat(80), EvidencePriority::High);
        high.relevance = Some(0.01);
        let request = ContextRequest {
            items: vec![late, high],
            max_tokens: 30,
            deduplicate: true,
        };
        let packet = compile_context(&request).unwrap();
        assert_eq!(packet.included_ids, ["high"]);

        // Unscored items keep submission order after scored ones.
        let scored = {
            let mut scored = item("scored", "short", EvidencePriority::Normal);
            scored.relevance = Some(0.1);
            scored
        };
        let request = ContextRequest {
            items: vec![
                item("first", "short", EvidencePriority::Normal),
                item("second", "short", EvidencePriority::Normal),
                scored,
            ],
            max_tokens: 100,
            deduplicate: true,
        };
        let packet = compile_context(&request).unwrap();
        assert_eq!(packet.included_ids, ["scored", "first", "second"]);
    }

    #[test]
    fn overlapping_evidence_is_sent_once() {
        // The saving no single tool can find: `search_code` and
        // `inspect_symbol` quote the same source lines, and neither knows the
        // other ran. The higher-priority item keeps the line.
        let shared = "pub const MAX_RETRY_ATTEMPTS: u32 = 20; // the shared line";
        let mut symbol = item(
            "WX-SYMBOL",
            &format!("fn apply_command() {{\n{shared}\n}}"),
            EvidencePriority::Critical,
        );
        symbol.source = "weavatrix:inspect_symbol".to_owned();
        let hits = item(
            "WX-SEARCH",
            &format!("crates/cortex-run/src/lib.rs:32\n{shared}\nanother distinct line of context"),
            EvidencePriority::High,
        );
        let request = ContextRequest {
            items: vec![hits, symbol],
            max_tokens: 1_000,
            deduplicate: true,
        };
        let packet = compile_context(&request).unwrap();
        assert_eq!(packet.included_ids, ["WX-SYMBOL", "WX-SEARCH"]);
        assert_eq!(packet.deduplicated_lines, 1);
        assert!(packet.deduplicated_estimated_tokens > 0);
        assert_eq!(
            packet.content.matches(shared).count(),
            1,
            "the shared line survives exactly once"
        );
        assert!(
            packet.content.contains("another distinct line of context"),
            "the rest of the lower-priority item is untouched"
        );
    }

    #[test]
    fn deduplication_never_empties_a_citation_and_can_be_turned_off() {
        let repeated = "an identical substantial line of evidence";
        let items = vec![
            item("first", repeated, EvidencePriority::High),
            item("second", repeated, EvidencePriority::Normal),
        ];
        let packet = compile_context(&ContextRequest {
            items: items.clone(),
            max_tokens: 1_000,
            deduplicate: true,
        })
        .unwrap();
        assert_eq!(packet.included_ids, ["first", "second"]);
        assert_eq!(
            packet.content.matches(repeated).count(),
            2,
            "an item that would render empty keeps its content instead"
        );
        assert_eq!(packet.deduplicated_lines, 0);

        let off = compile_context(&ContextRequest {
            items,
            max_tokens: 1_000,
            deduplicate: false,
        })
        .unwrap();
        assert_eq!(off.deduplicated_lines, 0);
        assert_eq!(off.content, packet.content);
    }

    #[test]
    fn short_lines_are_never_deduplicated() {
        // Removing every repeated `}` would corrupt an excerpt and save
        // nothing worth having.
        let brace = "}\n}\n}";
        let request = ContextRequest {
            items: vec![
                item(
                    "a",
                    &format!("fn one() {{\n{brace}"),
                    EvidencePriority::High,
                ),
                item(
                    "b",
                    &format!("fn two() {{\n{brace}"),
                    EvidencePriority::Normal,
                ),
            ],
            max_tokens: 1_000,
            deduplicate: true,
        };
        let packet = compile_context(&request).unwrap();
        assert_eq!(packet.deduplicated_lines, 0);
        assert_eq!(packet.content.matches("fn two()").count(), 1);
    }

    #[test]
    fn rejects_duplicate_evidence_ids() {
        let request = ContextRequest {
            items: vec![
                item("same", "one", EvidencePriority::Normal),
                item("same", "two", EvidencePriority::Normal),
            ],
            max_tokens: 100,
            deduplicate: true,
        };
        assert!(matches!(
            compile_context(&request),
            Err(ContextError::DuplicateId(id)) if id == "same"
        ));
    }
}
