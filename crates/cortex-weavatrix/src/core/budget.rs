//! Task-shaped packet budgets.
//!
//! A pin (`tight` / `normal` / `wide`) is a band. The default is `auto`:
//! pointed single-identifier work stays cheap; enumerating or mixed-language
//! work is allowed to spend more so the packet stays complete.

use crate::plan::extract_identifiers;
use crate::plan_intent::{TaskIntent, detect, is_broad};

pub const MIN_BUDGET: u32 = 1_200;
pub const MAX_BUDGET: u32 = 10_000;

/// Caller-supplied band. `Auto` looks at the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetPin {
    Tight,
    Normal,
    Wide,
    Auto,
}

impl BudgetPin {
    #[must_use]
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("tight") => Self::Tight,
            Some("wide") => Self::Wide,
            Some("normal") => Self::Normal,
            _ => Self::Auto,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tight => "tight",
            Self::Normal => "normal",
            Self::Wide => "wide",
            Self::Auto => "auto",
        }
    }
}

/// Tokens the compiler may spend on this task.
#[must_use]
pub fn adaptive_budget(task: &str, pin: BudgetPin) -> u32 {
    let identifiers = extract_identifiers(task);
    let extra = identifiers.len().saturating_sub(1);
    let extra_tokens = u32::try_from(extra).unwrap_or(u32::MAX).saturating_mul(400);
    let mut tokens = 2_400_u32.saturating_add(extra_tokens.min(1_600));
    if is_broad(task) {
        tokens = tokens.saturating_add(1_800);
    }
    match detect(task) {
        TaskIntent::IdentifierChange => {}
        TaskIntent::BlastRadius
        | TaskIntent::ApiContract
        | TaskIntent::TestSelection
        | TaskIntent::GitHistory
        | TaskIntent::StackTrace => tokens = tokens.saturating_add(600),
        TaskIntent::ModuleTopology | TaskIntent::RuntimeConfig | TaskIntent::PriorAttempt => {
            tokens = tokens.saturating_add(400);
        }
    }
    if mixed_script(task) {
        tokens = tokens.saturating_add(500);
    }
    clamp(tokens, pin)
}

fn clamp(tokens: u32, pin: BudgetPin) -> u32 {
    let (lo, hi) = match pin {
        BudgetPin::Tight => (1_200, 2_000),
        BudgetPin::Normal => (2_000, 5_000),
        BudgetPin::Wide => (5_000, 10_000),
        BudgetPin::Auto => (MIN_BUDGET, MAX_BUDGET),
    };
    tokens.clamp(lo, hi)
}

fn mixed_script(task: &str) -> bool {
    let mut latin = false;
    let mut other = false;
    for ch in task.chars() {
        if ch.is_ascii_alphabetic() {
            latin = true;
        } else if ch.is_alphabetic() {
            other = true;
        }
        if latin && other {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{BudgetPin, adaptive_budget};

    #[test]
    fn a_pointed_rename_stays_cheaper_than_an_enumerating_question() {
        let pointed = adaptive_budget("Rename `read_limited`", BudgetPin::Auto);
        let broad = adaptive_budget(
            "List every mechanism that can silently cause an archive miss",
            BudgetPin::Auto,
        );
        assert!(pointed < 3_200, "{pointed}");
        assert!(broad > pointed, "{broad} vs {pointed}");
        assert!(broad >= 4_000, "{broad}");
    }

    #[test]
    fn a_tight_pin_cannot_grow_into_a_wide_packet() {
        let tokens = adaptive_budget(
            "List every mechanism that can silently cause an archive miss",
            BudgetPin::Tight,
        );
        assert!(tokens <= 2_000, "{tokens}");
    }
}
