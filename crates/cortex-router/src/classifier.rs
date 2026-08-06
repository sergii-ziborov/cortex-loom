use cortex_domain::RiskLevel;

use crate::{Classification, TaskClass};

const MUTATION_WORDS: &[&str] = &[
    "add",
    "apply",
    "change",
    "delete",
    "edit",
    "fix",
    "implement",
    "modify",
    "remove",
    "rename",
    "replace",
    "rewrite",
    "update",
    "write",
];
const AUTH: &[&str] = &[
    "authentication",
    "authorization",
    "oauth",
    "openid",
    "login",
    "jwt",
    "access token",
    "refresh token",
    "role based access",
    "tenant isolation",
];
const SECURITY: &[&str] = &[
    "security",
    "vulnerability",
    "sql injection",
    "cross site scripting",
    "csrf",
    "xss",
    "credential",
    "secret rotation",
    "permission boundary",
    "threat model",
];
const CONCURRENCY: &[&str] = &[
    "concurrency",
    "concurrent",
    "race condition",
    "deadlock",
    "thread safety",
    "atomic update",
    "cancellation race",
    "parallel mutation",
];
const MIGRATION: &[&str] = &[
    "database migration",
    "schema migration",
    "data migration",
    "backfill",
    "migrate database",
];
const RELEASE: &[&str] = &[
    "release",
    "version bump",
    "bump the version",
    "bump version",
    "git tag",
    "tag the version",
    "release tag",
    "changelog release",
    "semver",
    "cut a release",
];
const DEPLOYMENT: &[&str] = &[
    "deployment",
    "deploy",
    "production rollout",
    "kubernetes",
    "terraform",
    "helm chart",
];
const PUBLICATION: &[&str] = &[
    "publication",
    "publish",
    "public registry",
    "cargo publish",
    "npm publish",
    "package registry",
];
const REPOSITORY: &[&str] = &[
    "dependency graph",
    "call graph",
    "repository graph",
    "repo graph",
    "dead code",
    "reachability",
    "impact analysis",
    "weavatrix",
    "analyze repository",
    "inspect dependencies",
];
const DETERMINISTIC: &[&str] = &[
    "format",
    "sort",
    "parse",
    "validate json",
    "validate schema",
    "count",
    "exact match",
    "canonicalize",
    "deterministic",
];
const EXTRACTION: &[&str] = &[
    "classify text",
    "extract fields",
    "extract entities",
    "label evidence",
    "tag evidence",
    "normalize metadata",
];
const COMPRESSION: &[&str] = &[
    "summarize",
    "summary",
    "compress evidence",
    "compress context",
    "context digest",
    "condense evidence",
];
const ADVISORY: &[&str] = &[
    "draft",
    "explain",
    "outline",
    "brainstorm",
    "suggest wording",
];

/// Classify work using stable lexical rules, without calling a model.
#[must_use]
pub fn classify(task: &str) -> Classification {
    let normalized = normalize(task);
    let mutation_likely = contains_word_from(&normalized, MUTATION_WORDS);
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

fn high_risk_class(normalized: &str) -> Option<TaskClass> {
    if has_phrase(normalized, AUTH) || contains_word_from(normalized, &["auth"]) {
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

fn normalize(task: &str) -> String {
    task.chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
