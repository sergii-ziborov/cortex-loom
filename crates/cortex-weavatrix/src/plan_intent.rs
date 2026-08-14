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

/// Classify `task` from stable structural cues in the prose.
#[must_use]
pub fn detect(task: &str) -> TaskIntent {
    let lower = task.to_ascii_lowercase();
    // Specific evidence shapes win over structural ones: a pasted panic is
    // not an API-contract question even if a path contains `/api/`.
    if stack_trace_cue(&lower) {
        return TaskIntent::StackTrace;
    }
    if test_selection_cue(&lower) {
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
    if blast_radius_cue(&lower) {
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

/// Whether the question is about a previous attempt, not a first look.
#[must_use]
pub fn asks_for_prior_attempts(task: &str) -> bool {
    prior_attempt_cue(&task.to_ascii_lowercase())
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
    ];
    let lower = task.to_ascii_lowercase();
    CUES.iter().any(|cue| lower.contains(cue))
        || (lower.contains("silently") && (lower.contains("nothing") || lower.contains("miss")))
}

/// Whether the task asks to introduce code that may not exist yet.
///
/// This is intentionally narrow. It is used only to avoid treating a named
/// future member such as `ArchiveOptions::disabled` as evidence that must
/// already exist; the owning symbol's complete definition remains required.
#[must_use]
pub(crate) fn is_creation(task: &str) -> bool {
    let lower = task.to_ascii_lowercase();
    lower
        .split(|character: char| !character.is_ascii_alphabetic())
        .take(6)
        .any(|word| matches!(word, "implement" | "add" | "create" | "introduce"))
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
mod tests {
    use super::{TaskIntent, detect, is_broad, is_creation};

    #[test]
    fn enumerating_questions_are_broad_and_pointed_ones_are_not() {
        assert!(is_broad(
            "A regex matches a file on disk but returns nothing inside a .tar.gz. \
             List every mechanism in this crate that can silently cause that."
        ));
        assert!(is_broad("What can cause the collector to drop matches?"));
        assert!(!is_broad("Rename `read_limited` in containers.rs"));
        assert!(!is_broad(
            "Who depends on `route` if its signature changes?"
        ));
    }

    #[test]
    fn creation_cues_are_limited_to_the_task_opening() {
        assert!(is_creation("Implement `ArchiveOptions::disabled()`"));
        assert!(is_creation("Please add `ArchiveOptions::disabled()`"));
        assert!(!is_creation(
            "Who calls `ArchiveOptions::disabled()` after the change?"
        ));
    }

    #[test]
    fn blast_contract_and_topology_cues_are_recognised() {
        assert_eq!(
            detect("Who depends on compile_context if its signature changes?"),
            TaskIntent::BlastRadius
        );
        assert_eq!(
            detect("What breaks if the POST /api/skills/compile HTTP contract changes?"),
            TaskIntent::ApiContract
        );
        assert_eq!(
            detect("Which services read the Streamable HTTP MCP transport at `/mcp`?"),
            TaskIntent::ApiContract
        );
        assert_eq!(
            detect("Which module owns compile_context, and where does the crate layout put it?"),
            TaskIntent::ModuleTopology
        );
        assert_eq!(
            detect("Rename RetryLimitTooLarge in retry.rs"),
            TaskIntent::IdentifierChange
        );
        assert_eq!(
            detect("How does CORTEX_LLM read config/llm-profiles.json?"),
            TaskIntent::RuntimeConfig
        );
        assert_eq!(
            detect("Which env flag enables ShadowHandle?"),
            TaskIntent::RuntimeConfig
        );
        assert_eq!(
            detect("Who changed `compile_context` last?"),
            TaskIntent::GitHistory
        );
        assert_eq!(
            detect("Which tests should I run after changing compile_context?"),
            TaskIntent::TestSelection
        );
        assert_eq!(
            detect("thread 'main' panicked at src/retry.rs:12:1:\nstack backtrace:"),
            TaskIntent::StackTrace
        );
        assert_eq!(
            detect("Fix the failing unit test in the graph store"),
            TaskIntent::IdentifierChange
        );
        assert_eq!(
            detect("Still failing compile_context after the last attempt"),
            TaskIntent::PriorAttempt
        );
    }
}
