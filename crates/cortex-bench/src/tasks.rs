//! Benchmark fixtures.
//!
//! Every task is a real change someone could be asked to make in this
//! repository, and every anchor is a literal that exists in it today. A
//! fixture whose anchors the generous naive arm cannot satisfy is a broken
//! fixture, not an interesting result — `fixture_anchors_exist` in the test
//! module enforces that.
//!
//! Prompts name code the way an engineer actually writes a ticket, because
//! that is the input the planner receives in production. It is not a way to
//! smuggle the answer in: the compiled packet echoes the prompt back, and the
//! scoring in [`crate::measure_scoped`] excludes that echo, so an anchor
//! counts only when the evidence system retrieved it from the repository.

use crate::Anchor;

/// One engineering task, with the evidence a sweep would open and the facts
/// the change cannot be made without.
#[derive(Debug, Clone, Copy)]
pub struct BenchTask {
    pub id: &'static str,
    pub prompt: &'static str,
    /// Symbol handed to Weavatrix `context_bundle`, when the task has an
    /// obvious entry point. `None` means the arms run without symbol
    /// evidence, which is the harder case for the graph tools.
    pub symbol: Option<&'static str>,
    /// Directories a keyword sweep would open, as `/`-separated globs.
    pub naive_globs: &'static [&'static str],
    pub anchors: &'static [Anchor],
}

/// The fixture set.
#[must_use]
pub const fn tasks() -> &'static [BenchTask] {
    &[
        BenchTask {
            id: "retry-exhaustion",
            prompt: "Change bounded retry so a target that reached its `maxAttempts` \
resolves the run normally instead of leaving it retryable.",
            symbol: Some("apply_command"),
            naive_globs: &["crates/cortex-run/src/*.rs"],
            anchors: &[
                Anchor {
                    id: "retry-limit-constant",
                    any_of: &["MAX_RETRY_ATTEMPTS"],
                },
                Anchor {
                    id: "attempt-config-key",
                    any_of: &["maxAttempts", "max_attempts"],
                },
                Anchor {
                    id: "target-config-key",
                    any_of: &["targetNodeId", "target_node_id"],
                },
                Anchor {
                    id: "limit-error-variant",
                    any_of: &["RetryLimitTooLarge"],
                },
                Anchor {
                    id: "command-entry-point",
                    any_of: &["apply_command"],
                },
                Anchor {
                    id: "owning-module",
                    any_of: &["retry.rs", "cortex-run"],
                },
            ],
        },
        BenchTask {
            id: "evidence-priority-band",
            prompt: "Add a new band to `EvidencePriority` between High and Normal in the \
deterministic context compiler, keeping critical evidence fail-closed.",
            symbol: Some("compile_context"),
            naive_globs: &[
                "crates/cortex-context/src/*.rs",
                "crates/cortex-weavatrix/src/context.rs",
            ],
            anchors: &[
                Anchor {
                    id: "priority-enum",
                    any_of: &["EvidencePriority"],
                },
                Anchor {
                    id: "priority-ordering",
                    any_of: &["fn rank"],
                },
                Anchor {
                    id: "fail-closed-error",
                    any_of: &["CriticalItemExceedsBudget"],
                },
                Anchor {
                    id: "token-estimator",
                    any_of: &["estimate_tokens"],
                },
                Anchor {
                    id: "compiler-entry-point",
                    any_of: &["compile_context"],
                },
                Anchor {
                    id: "owning-module",
                    any_of: &["cortex-context"],
                },
            ],
        },
        BenchTask {
            id: "skill-frontmatter-lists",
            prompt: "Support a list-valued frontmatter key in `import_skill_markdown` \
and export, without breaking the export fixpoint.",
            symbol: Some("import_skill_markdown"),
            naive_globs: &[
                "crates/cortex-skills/src/*.rs",
                "crates/cortex-skills/fixtures/*.md",
            ],
            anchors: &[
                Anchor {
                    id: "frontmatter-parser",
                    any_of: &["split_frontmatter"],
                },
                Anchor {
                    id: "scalar-decoder",
                    any_of: &["unquote"],
                },
                Anchor {
                    id: "export-side",
                    any_of: &["export_skill_markdown"],
                },
                Anchor {
                    id: "title-invariant",
                    any_of: &["heading_text"],
                },
                Anchor {
                    id: "step-dependencies",
                    any_of: &["dependency_numbers", "[depends:"],
                },
                Anchor {
                    id: "owning-module",
                    any_of: &["cortex-skills"],
                },
            ],
        },
        BenchTask {
            id: "usage-quality-tool",
            prompt: "Expose the token-accounting `quality_summary` as a bounded MCP \
tool alongside the existing `usage_read` and `usage_report` tools.",
            symbol: None,
            naive_globs: &["crates/cortex-mcp/src/*.rs"],
            anchors: &[
                Anchor {
                    id: "existing-read-tool",
                    any_of: &["usage_read"],
                },
                Anchor {
                    id: "quality-join",
                    any_of: &["quality_summary"],
                },
                Anchor {
                    id: "tool-registry",
                    any_of: &["tools/list"],
                },
                Anchor {
                    id: "report-tool",
                    any_of: &["usage_report"],
                },
                Anchor {
                    id: "compile-tool",
                    any_of: &["weavatrix_context_compile"],
                },
                Anchor {
                    id: "owning-module",
                    any_of: &["cortex-mcp"],
                },
            ],
        },
    ]
}

/// Look one fixture up by id.
#[must_use]
pub fn find(id: &str) -> Option<&'static BenchTask> {
    tasks().iter().find(|task| task.id == id)
}
