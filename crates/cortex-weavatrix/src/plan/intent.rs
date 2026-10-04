//! Lightweight task-intent cues for evidence planning.
//!
//! Identifier shape alone cannot tell a blast-radius question from a rename.
//! These cues are deterministic keyword checks on the task text — not a model —
//! so the planner can ask for dependents or endpoints when the question is
//! structural, and still fall back to the identifier-driven default otherwise.

/// What kind of evidence the task is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskIntent {
    /// Default: identifiers named in the task drive search and symbol context.
    IdentifierChange,
    /// Callers, dependents, blast radius, or "what breaks if this changes".
    BlastRadius,
    /// HTTP/API/transport contracts and who reads them.
    ApiContract,
    /// Module / crate ownership and repository topology.
    ModuleTopology,
    /// Runtime configuration, environment flags, profiles, and policy gates.
    RuntimeConfig,
    /// Commit history, churn, blame, or "who introduced this".
    GitHistory,
    /// A panic, backtrace, or pasted stack frames to map onto the graph.
    StackTrace,
    /// Which tests a change should run.
    TestSelection,
    /// Prior run failures, rejections, or retries for this work.
    PriorAttempt,
}

impl TaskIntent {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdentifierChange => "identifier_change",
            Self::BlastRadius => "blast_radius",
            Self::ApiContract => "api_contract",
            Self::ModuleTopology => "module_topology",
            Self::RuntimeConfig => "runtime_config",
            Self::GitHistory => "git_history",
            Self::StackTrace => "stack_trace",
            Self::TestSelection => "test_selection",
            Self::PriorAttempt => "prior_attempt",
        }
    }
}

/// Classify `task` from stable structural cues in the prose.
#[must_use]
pub fn detect(task: &str) -> TaskIntent {
    let lower = crate::fold::fold_text(task);
    // Specific evidence shapes win over structural ones: a pasted panic is
    // not an API-contract question even if a path contains `/api/`.
    if stack_trace_cue(&lower) {
        return TaskIntent::StackTrace;
    }
    if test_selection_cue(&lower) && (direct_test_question(&lower) || !is_coding_change(task)) {
        return TaskIntent::TestSelection;
    }
    if git_history_cue(&lower) {
        return TaskIntent::GitHistory;
    }
    if prior_attempt_cue(&lower) {
        return TaskIntent::PriorAttempt;
    }
    // Contract cues win over blast-radius "what breaks" when both appear.
    if api_contract_cue(&lower) {
        return TaskIntent::ApiContract;
    }
    if blast_radius_cue(&lower) || asks_for_caller_impact(task) {
        return TaskIntent::BlastRadius;
    }
    if module_topology_cue(&lower) {
        return TaskIntent::ModuleTopology;
    }
    if runtime_config_cue(&lower) {
        return TaskIntent::RuntimeConfig;
    }
    TaskIntent::IdentifierChange
}

/// A change-impact question can name callers without using the usual
/// "who calls" wording. Keep this independent of the selected intent so a
/// task asking about callers and endpoints can require both evidence classes.
#[must_use]
pub fn asks_for_caller_impact(task: &str) -> bool {
    let lower = crate::fold::fold_text(task);
    [
        "who calls",
        "what calls",
        "call chain",
        "call graph",
        "callers of",
        "who depends",
        "what depends",
        "dependents of",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
        || ((lower.contains("caller") || lower.contains("call site"))
            && ["chang", "affect", "impact", "break"]
                .iter()
                .any(|cue| lower.contains(cue)))
}

/// Whether the question explicitly includes transport entry points in its
/// change impact, even when caller impact is the primary intent.
#[must_use]
pub fn asks_for_endpoint_impact(task: &str) -> bool {
    let lower = crate::fold::fold_text(task);
    (lower.contains("endpoint") || lower.contains("entry point") || lower.contains("transport"))
        && (lower.contains("mcp") || lower.contains("http") || lower.contains("api"))
}

#[must_use]
pub fn asks_for_ui(task: &str) -> bool {
    crate::fold::fold_text(task)
        .split(|character: char| !character.is_alphanumeric())
        .any(|word| matches!(word, "ui" | "frontend" | "react" | "tsx" | "css"))
}

/// Coding verbs take precedence over incidental instructions to run tests.
#[must_use]
pub fn is_coding_change(task: &str) -> bool {
    const VERBS: &[&str] = &[
        "add",
        "implement",
        "fix",
        "change",
        "update",
        "remove",
        "refactor",
        "rename",
        "split",
        "migrate",
        "modify",
        "extend",
        "patch",
        "create",
        "добавь",
        "добавить",
        "исправь",
        "исправить",
        "измени",
        "изменить",
        "обнови",
        "удали",
        "реализуй",
        "додай",
        "виправ",
        "зміни",
    ];
    crate::fold::fold_text(task)
        .split(|character: char| !character.is_alphabetic())
        .any(|word| VERBS.contains(&word))
}

/// Coding work that explicitly asks for tests needs the owning suite as
/// editable context, even when "run tests" makes test selection incidental.
#[must_use]
pub fn asks_for_test_source(task: &str) -> bool {
    if !is_coding_change(task) {
        return false;
    }
    crate::fold::fold_text(task)
        .split(|character: char| !character.is_alphanumeric())
        .any(|word| matches!(word, "test" | "tests" | "regression"))
}

fn direct_test_question(lower: &str) -> bool {
    let start = lower.trim_start_matches("please ").trim_start();
    [
        "which tests",
        "what tests",
        "what should i test",
        "what should we test",
        "tests to run",
        "tests should",
        "select tests",
        "какие тесты",
        "які тести",
    ]
    .iter()
    .any(|cue| start.starts_with(cue))
}

/// Whether the question is about a previous attempt, not a first look.
#[must_use]
pub fn asks_for_prior_attempts(task: &str) -> bool {
    prior_attempt_cue(&crate::fold::fold_text(task))
}

fn prior_attempt_cue(lower: &str) -> bool {
    const CUES: &[&str] = &[
        "already tried",
        "already failed",
        "previous attempt",
        "prior attempt",
        "last run",
        "last attempt",
        "still failing",
        "still fails",
        "same error",
        "same failure",
        "we already",
        "tried this",
        "tried again",
        "last time",
        "once more",
        "предыдущая попытка",
        "прошлый раз",
        "ещё раз",
        "еще раз",
        "всё ещё падает",
        "все еще падает",
        "попередня спроба",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
}

/// Whether the task enumerates ("list every mechanism that can…") rather
/// than pointing at one identifier.
///
/// A broad question answered by a thin packet was the measured failure mode:
/// on the live-model benchmark the cross-cutting probe compiled 2 347 of a
/// 4 000-token budget and scored 0/5, while the arm that read whole modules
/// scored 3/5. Breadth is a property of the question, so it is detected from
/// the question, not inferred from an after-the-fact token count.
#[must_use]
pub fn is_broad(task: &str) -> bool {
    const CUES: &[&str] = &[
        "every mechanism",
        "all mechanisms",
        "list every",
        "list all",
        "all the ways",
        "every way",
        "all places",
        "everywhere",
        "each mechanism",
        "each place",
        "what can cause",
        "can silently",
        "silently cause",
        "silently fail",
        "silently drop",
        "all reasons",
        "every reason",
        "exhaustive",
        "каждый механизм",
        "все механизмы",
        "перечисли все",
        "перелічи всі",
        "alle mechanismen",
        "כל מנגנון",
    ];
    let lower = crate::fold::fold_text(task);
    CUES.iter().any(|cue| lower.contains(cue))
        || (lower.contains("silently") && (lower.contains("nothing") || lower.contains("miss")))
        || (lower.contains("молча") && (lower.contains("пропуст") || lower.contains("ничего")))
}

/// Whether the task asks for a dead or unused production path.
#[must_use]
pub fn asks_for_dead_production(task: &str) -> bool {
    const CUES: &[&str] = &[
        "dead production path",
        "dead production",
        "dead path",
        "unused production",
        "unreferenced",
        "find and fix one real bug",
        "find and fix a real bug",
        "find_fix_bug",
        "мёртвый путь",
        "мертвый путь",
        "мёртвый production",
    ];
    let lower = crate::fold::fold_text(task);
    CUES.iter().any(|cue| lower.contains(cue))
}

/// Whether the task asks to share one implementation without merging baselines.
#[must_use]
pub fn asks_for_duplicate_share(task: &str) -> bool {
    let lower = crate::fold::fold_text(task);
    let duplicate = [
        "duplicate",
        "duplicates",
        "shared implementation",
        "one shared",
        "убрать дубл",
        "устрани дубл",
        "дублкод",
    ]
    .iter()
    .any(|cue| lower.contains(cue));
    let classifier = ["classifier", "classify", "классификатор", "класифікатор"]
        .iter()
        .any(|cue| lower.contains(cue));
    duplicate && classifier
}

/// Whether the task asks to introduce code that may not exist yet.
///
/// This is intentionally narrow. It is used only to avoid treating a named
/// future member such as `ArchiveOptions::disabled` as evidence that must
/// already exist; the owning symbol's complete definition remains required.
#[must_use]
pub(crate) fn is_creation(task: &str) -> bool {
    let lower = crate::fold::fold_text(task);
    lower
        .split(|character: char| !character.is_alphabetic())
        .take(6)
        .any(|word| {
            matches!(
                word,
                "implement"
                    | "add"
                    | "create"
                    | "introduce"
                    | "добав"
                    | "добавь"
                    | "добавить"
                    | "создай"
                    | "реализуй"
                    | "додай"
                    | "створи"
            )
        })
}

fn stack_trace_cue(lower: &str) -> bool {
    const CUES: &[&str] = &[
        "stacktrace",
        "stack trace",
        "stack-trace",
        "backtrace",
        "back trace",
        "panicked at",
        "called `result::unwrap()`",
        "called `option::unwrap()`",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
        || (lower.contains("thread '") && lower.contains("panicked"))
        || (lower.contains(".rs:")
            && (lower.contains(" at ")
                || lower.contains("at src/")
                || lower.contains("at crates/")
                || lower.contains("\tat ")))
}

fn test_selection_cue(lower: &str) -> bool {
    const CUES: &[&str] = &[
        "which tests",
        "what tests",
        "select tests",
        "tests to run",
        "tests should i run",
        "tests should we run",
        "which test suite",
        "which suites to run",
        "relevant tests",
        "what should i test",
        "what should we test",
        "какие тесты",
        "які тести",
        "какие тесты запустить",
        "welche tests",
        "אילו בדיקות",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
}

fn git_history_cue(lower: &str) -> bool {
    const CUES: &[&str] = &[
        "git history",
        "commit history",
        "git log",
        "git blame",
        "who changed",
        "who last edited",
        "who last touched",
        "who introduced",
        "who added",
        "last commit",
        "recent commits",
        "co-change",
        "cochange",
        "when was this added",
        "when was this introduced",
        "when did we add",
        "when did we introduce",
        "кто менял",
        "хто змінював",
        "кто последний правил",
        "git verlauf",
        "מי שינה",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
        || (lower.contains("churn") && (lower.contains("commit") || lower.contains("file")))
        || (lower.contains(" blame ") && (lower.contains("line") || lower.contains("file")))
}

fn blast_radius_cue(lower: &str) -> bool {
    const CUES: &[&str] = &[
        "blast radius",
        "who depends",
        "what depends",
        "dependents of",
        "who calls",
        "what calls",
        "callers of",
        "what breaks",
        "who breaks",
        "impact of changing",
        "if its signature",
        "if the signature",
        "кто вызывает",
        "хто викликає",
        "что сломается",
        "що зламається",
        "wer ruft",
        "מי קורא",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
}

fn api_contract_cue(lower: &str) -> bool {
    const CUES: &[&str] = &[
        "http contract",
        "api contract",
        "endpoint contract",
        "transport contract",
        "wire contract",
        "who reads",
        "which service",
        "which services",
        "/api/",
        "/mcp",
        "streamable http",
        "list endpoints",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
        || (lower.contains("endpoint") && lower.contains("contract"))
        || (lower.contains("transport") && (lower.contains("read") || lower.contains("serve")))
}

fn module_topology_cue(lower: &str) -> bool {
    const CUES: &[&str] = &[
        "module map",
        "module topology",
        "which module",
        "which crate",
        "owning module",
        "owning crate",
        "where does",
        "where is",
        "crate layout",
        "package layout",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
}

fn runtime_config_cue(lower: &str) -> bool {
    const CUES: &[&str] = &[
        "environment variable",
        "env variable",
        "env var",
        "env flag",
        "feature flag",
        "runtime config",
        "configuration",
        "config/",
        ".json",
        ".yaml",
        ".yml",
        ".toml",
        "profile gate",
        "policy gate",
        "gatepassed",
        "gate_passed",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
        || lower
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .any(|word| word.starts_with("cortex_") || word.ends_with("_enabled"))
}

#[cfg(test)]
#[path = "intent_tests.rs"]
mod tests;
