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
    /// order relative to each other — so scoring part of a band promotes
    /// those items over the rest of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextRequest {
    pub items: Vec<EvidenceItem>,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextPacket {
    pub content: String,
    pub included_ids: Vec<String>,
    pub omitted_ids: Vec<String>,
    pub raw_estimated_tokens: u32,
    pub selected_estimated_tokens: u32,
    pub saved_estimated_tokens: u32,
    pub requires_upstream: bool,
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
        .map(|item| estimate_tokens(&render_item(item)))
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

    let mut content = String::new();
    let mut included_ids = Vec::new();
    let mut omitted_ids = Vec::new();
    let mut selected_estimated_tokens = 0_u32;
    for (_, item) in ordered {
        let rendered = render_item(item);
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
        saved_estimated_tokens: raw_estimated_tokens.saturating_sub(selected_estimated_tokens),
        requires_upstream,
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

fn render_item(item: &EvidenceItem) -> String {
    format!(
        "## [{}] {}\n{}\n\n",
        item.id,
        item.source,
        item.content.trim()
    )
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
        };
        let packet = compile_context(&request).unwrap();
        assert_eq!(packet.included_ids, ["high"]);
        assert_eq!(packet.omitted_ids, ["low"]);
        assert!(packet.saved_estimated_tokens > 0);
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
        };
        let packet = compile_context(&request).unwrap();
        assert_eq!(packet.included_ids, ["scored", "first", "second"]);
    }

    #[test]
    fn rejects_duplicate_evidence_ids() {
        let request = ContextRequest {
            items: vec![
                item("same", "one", EvidencePriority::Normal),
                item("same", "two", EvidencePriority::Normal),
            ],
            max_tokens: 100,
        };
        assert!(matches!(
            compile_context(&request),
            Err(ContextError::DuplicateId(id)) if id == "same"
        ));
    }
}
