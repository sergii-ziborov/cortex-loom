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
}

/// Classify `task` from stable structural cues in the prose.
#[must_use]
pub fn detect(task: &str) -> TaskIntent {
    let lower = task.to_ascii_lowercase();
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
    use super::{TaskIntent, detect};

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
    }
}
