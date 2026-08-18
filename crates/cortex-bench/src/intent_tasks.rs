//! Git history, stack-trace, and test-selection fixtures.
//!
//! These intents already have planner unit tests. This set scores whether
//! the gathered packet actually carries the facts those tools should name.

use crate::Anchor;
use crate::tasks::BenchTask;

/// Fixture set used by `cortex-bench --set intent`.
#[must_use]
pub const fn intent_tasks() -> &'static [BenchTask] {
    &[
        BenchTask {
            id: "intent-git-compile-context",
            prompt: concat!("Who changed `compile_", "context` last?"),
            symbol: Some(concat!("compile_", "context")),
            naive_globs: &[concat!("crates/cortex-", "context/src/", "lib.rs")],
            anchors: &[
                Anchor {
                    id: "seed-symbol",
                    any_of: &[concat!("compile_", "context")],
                },
                Anchor {
                    id: "owning-file",
                    any_of: &[concat!("context/src/", "lib.rs")],
                },
                Anchor {
                    // Weavatrix git_history returns id/time/summary, not author.
                    id: "history-summary",
                    any_of: &[concat!("Stop minting ", "Verified")],
                },
                Anchor {
                    id: "owning-crate",
                    any_of: &[concat!("cortex-", "context")],
                },
            ],
        },
        BenchTask {
            id: "intent-stack-retry",
            prompt: concat!(
                "thread 'main' panicked at crates/cortex-run/src/",
                "re",
                "try.rs:24:1:\n",
                "stack backtrace:"
            ),
            symbol: None,
            naive_globs: &[concat!("crates/cortex-run/src/re", "try.rs")],
            anchors: &[
                Anchor {
                    id: "panic-file",
                    any_of: &[concat!("retry", ".rs")],
                },
                Anchor {
                    id: "limit-error",
                    any_of: &[concat!("Retry", "LimitTooLarge")],
                },
                Anchor {
                    id: "limit-constant",
                    any_of: &[concat!("MAX_", "RETRY_ATTEMPTS")],
                },
                Anchor {
                    id: "validator",
                    any_of: &[concat!("validate_", "retry_nodes")],
                },
            ],
        },
        BenchTask {
            id: "intent-tests-compile-context",
            prompt: concat!(
                "Which tests should I run after changing compile_",
                "context?"
            ),
            symbol: Some(concat!("compile_", "context")),
            naive_globs: &[concat!("crates/cortex-", "context/src/tests.rs")],
            anchors: &[
                Anchor {
                    id: "seed-symbol",
                    any_of: &[concat!("compile_", "context")],
                },
                Anchor {
                    id: "fail-closed-test",
                    any_of: &[concat!("critical_evidence_never_", "disappears_silently")],
                },
                Anchor {
                    id: "priority-test",
                    any_of: &[concat!(
                        "selects_priority_order_",
                        "and_reports_token_savings"
                    )],
                },
                Anchor {
                    id: "owning-crate",
                    any_of: &[concat!("cortex-", "context")],
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_source_cannot_satisfy_its_own_anchors() {
        let source = include_str!("intent_tasks.rs").to_ascii_lowercase();
        for task in intent_tasks() {
            for anchor in task.anchors {
                for candidate in anchor.any_of {
                    assert!(
                        !source.contains(&candidate.to_ascii_lowercase()),
                        "{} / {} is present in the fixture source: {candidate}",
                        task.id,
                        anchor.id
                    );
                }
            }
        }
    }
}
