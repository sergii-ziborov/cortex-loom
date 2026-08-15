use cortex_domain::RiskLevel;

use crate::fold::fold_words;
use crate::lexicon::{
    ADVISORY, AUTH, COMPRESSION, CONCURRENCY, DEPLOYMENT, DETERMINISTIC, EXTRACTION, INJECTION,
    MIGRATION, MUTATION, PUBLICATION, RELEASE, REPOSITORY, SECURITY, negated,
};
use crate::{Classification, TaskClass};

/// Classify work using stable lexical rules, without calling a model.
#[must_use]
pub fn classify(task: &str) -> Classification {
    let normalized = fold_words(task);
    let mutation_likely = mutation_likely(&normalized);
    let class = high_risk_class(&normalized)
        .unwrap_or_else(|| ordinary_class(&normalized, mutation_likely));
    let risk = if class.is_high_risk() {
        RiskLevel::High
    } else if matches!(
        class,
        TaskClass::Implementation | TaskClass::RepositoryAnalysis
    ) {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    };
    Classification {
        class,
        risk,
        mutation_likely,
    }
}

fn mutation_likely(normalized: &str) -> bool {
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    tokens.iter().enumerate().any(|(index, token)| {
        MUTATION
            .iter()
            .any(|cue| *token == *cue || token.starts_with(cue) && cue.chars().count() >= 4)
            && !negated(&tokens, index)
    })
}

fn high_risk_class(normalized: &str) -> Option<TaskClass> {
    if has_phrase(normalized, INJECTION) {
        Some(TaskClass::Ambiguous)
    } else if has_phrase(normalized, AUTH) || contains_word_from(normalized, &["auth"]) {
        Some(TaskClass::Authentication)
    } else if has_phrase(normalized, SECURITY) {
        Some(TaskClass::Security)
    } else if has_phrase(normalized, CONCURRENCY) {
        Some(TaskClass::Concurrency)
    } else if has_phrase(normalized, MIGRATION)
        || contains_word_from(normalized, &["migration", "migrations"])
    {
        Some(TaskClass::Migration)
    } else if has_phrase(normalized, RELEASE) {
        Some(TaskClass::Release)
    } else if has_phrase(normalized, DEPLOYMENT) {
        Some(TaskClass::Deployment)
    } else if has_phrase(normalized, PUBLICATION) {
        Some(TaskClass::Publication)
    } else {
        None
    }
}

fn ordinary_class(normalized: &str, mutation_likely: bool) -> TaskClass {
    if is_ambiguous(normalized) {
        TaskClass::Ambiguous
    } else if has_phrase(normalized, REPOSITORY) {
        TaskClass::RepositoryAnalysis
    } else if has_phrase(normalized, DETERMINISTIC) && !mutation_likely {
        TaskClass::Deterministic
    } else if has_phrase(normalized, EXTRACTION) && !mutation_likely {
        TaskClass::StructuredExtraction
    } else if has_phrase(normalized, COMPRESSION) && !mutation_likely {
        TaskClass::ContextCompression
    } else if has_phrase(normalized, ADVISORY) && !mutation_likely {
        TaskClass::AdvisoryDraft
    } else if mutation_likely {
        TaskClass::Implementation
    } else {
        TaskClass::Ambiguous
    }
}

fn has_phrase(normalized: &str, phrases: &[&str]) -> bool {
    let padded = format!(" {normalized} ");
    phrases
        .iter()
        .any(|phrase| padded.contains(&format!(" {phrase} ")))
}

fn contains_word_from(normalized: &str, words: &[&str]) -> bool {
    normalized
        .split_whitespace()
        .any(|word| words.contains(&word))
}

fn is_ambiguous(normalized: &str) -> bool {
    normalized.is_empty()
        || matches!(
            normalized,
            "fix it" | "do it" | "handle this" | "make it better" | "update this" | "change this"
        )
}
