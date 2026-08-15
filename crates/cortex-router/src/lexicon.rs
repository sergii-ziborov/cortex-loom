//! Language-independent cue tables for the lexical classifier.
//!
//! The source phrase may be English, Russian, Ukrainian, Hebrew, German, or
//! mixed. Intent is the enum; these strings are only how it is recognised.

pub const MUTATION: &[&str] = &[
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
    "добав",
    "добавь",
    "добавить",
    "обнови",
    "обновить",
    "исправь",
    "исправить",
    "удали",
    "удалить",
    "измени",
    "изменить",
    "переименуй",
    "переименовать",
    "реализуй",
    "реализовать",
    "напиши",
    "написать",
    "поправь",
    "внедри",
    "додай",
    "онови",
    "виправ",
    "видали",
    "зміни",
    "перейменуй",
    "реалізуй",
    "hinzufugen",
    "hinzufügen",
    "andern",
    "ändern",
    "aktualisieren",
    "loschen",
    "löschen",
    "implementieren",
    "umbenennen",
    "entfernen",
    "הוסף",
    "עדכן",
    "תקן",
    "מחק",
    "שנה",
];

pub const NEGATION: &[&str] = &[
    "not",
    "dont",
    "don't",
    "without",
    "no",
    "не",
    "без",
    "ні",
    "немає",
    "ohne",
    "nicht",
    "kein",
    "אל",
    "בלי",
];

pub const AUTH: &[&str] = &[
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
    "аутентификац",
    "авторизац",
    "аутентифікац",
    "авторизаці",
];

pub const SECURITY: &[&str] = &[
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
    "уязвим",
    "вразлив",
    "инъекц",
    "ін'єкц",
    "безопасность",
    "безпека",
];

pub const CONCURRENCY: &[&str] = &[
    "concurrency",
    "concurrent",
    "race condition",
    "deadlock",
    "thread safety",
    "atomic update",
    "cancellation race",
    "parallel mutation",
    "гонка",
    "взаимн блокир",
    "взаємн блокув",
];

pub const MIGRATION: &[&str] = &[
    "database migration",
    "schema migration",
    "data migration",
    "backfill",
    "migrate database",
    "миграц",
    "міграц",
];

pub const RELEASE: &[&str] = &[
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
    "релиз",
    "реліз",
];

pub const DEPLOYMENT: &[&str] = &[
    "deployment",
    "deploy",
    "production rollout",
    "kubernetes",
    "terraform",
    "helm chart",
    "деплой",
    "разверт",
    "розгорт",
];

pub const PUBLICATION: &[&str] = &[
    "publication",
    "publish",
    "public registry",
    "cargo publish",
    "npm publish",
    "package registry",
    "опублик",
    "опублік",
];

pub const REPOSITORY: &[&str] = &[
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

pub const DETERMINISTIC: &[&str] = &[
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

pub const EXTRACTION: &[&str] = &[
    "classify text",
    "extract fields",
    "extract entities",
    "label evidence",
    "tag evidence",
    "normalize metadata",
];

pub const COMPRESSION: &[&str] = &[
    "summarize",
    "summary",
    "compress evidence",
    "compress context",
    "context digest",
    "condense evidence",
];

pub const ADVISORY: &[&str] = &[
    "draft",
    "explain",
    "outline",
    "brainstorm",
    "suggest wording",
];

pub const INJECTION: &[&str] = &[
    "ignore previous",
    "ignore all instructions",
    "игнорируй инструкции",
    "ігноруй інструкції",
    "disregard the system",
];

/// True when `word` at `index` is negated by the previous token.
#[must_use]
pub fn negated(tokens: &[&str], index: usize) -> bool {
    index
        .checked_sub(1)
        .and_then(|previous| tokens.get(previous).copied())
        .is_some_and(|previous| NEGATION.contains(&previous))
}
