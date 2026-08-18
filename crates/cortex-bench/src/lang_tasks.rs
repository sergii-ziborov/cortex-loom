//! Multi-language probe fixtures. Anchors live in `fixtures/langs/`.

use crate::Anchor;
use crate::tasks::BenchTask;

/// TS/JS, Python, Go, Java, and C# tasks over checked-in samples.
#[must_use]
pub const fn lang_tasks() -> &'static [BenchTask] {
    &[
        BenchTask {
            id: "lang-ts-retry",
            prompt: concat!(
                "How does `scheduleTsRetry` cap attempts with `TS_",
                "RETRY_CAP`?"
            ),
            symbol: Some("scheduleTsRetry"),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.ts"],
            anchors: &[
                Anchor {
                    id: "ts-cap",
                    any_of: &[concat!("TS_", "RETRY_CAP")],
                },
                Anchor {
                    id: "ts-fn",
                    any_of: &[concat!("function ", "scheduleTsRetry")],
                },
            ],
        },
        BenchTask {
            id: "lang-js-retry",
            prompt: concat!(
                "How does `scheduleJsRetry` cap attempts with `JS_",
                "RETRY_CAP`?"
            ),
            symbol: Some("scheduleJsRetry"),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.js"],
            anchors: &[
                Anchor {
                    id: "js-cap",
                    any_of: &[concat!("JS_", "RETRY_CAP")],
                },
                Anchor {
                    id: "js-fn",
                    any_of: &[concat!("function ", "scheduleJsRetry")],
                },
            ],
        },
        BenchTask {
            id: "lang-py-retry",
            prompt: concat!("How does `schedule_py_retry` use `PY_", "RETRY_CAP`?"),
            symbol: Some("schedule_py_retry"),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.py"],
            anchors: &[
                Anchor {
                    id: "py-cap",
                    any_of: &[concat!("PY_", "RETRY_CAP")],
                },
                Anchor {
                    id: "py-fn",
                    any_of: &[concat!("def ", "schedule_py_retry")],
                },
            ],
        },
        BenchTask {
            id: "lang-go-retry",
            prompt: concat!("How does `ScheduleGoRetry` honour `Go", "RetryCap`?"),
            symbol: Some("ScheduleGoRetry"),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.go"],
            anchors: &[
                Anchor {
                    id: "go-cap",
                    any_of: &[concat!("Go", "RetryCap")],
                },
                Anchor {
                    id: "go-fn",
                    any_of: &[concat!("func ", "ScheduleGoRetry")],
                },
            ],
        },
        BenchTask {
            id: "lang-java-retry",
            prompt: concat!(
                "How does `schedule",
                "JavaRetry` honour `JAVA_",
                "RETRY_CAP`?"
            ),
            symbol: Some(concat!("schedule", "JavaRetry")),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.java"],
            anchors: &[
                Anchor {
                    id: "java-cap",
                    any_of: &[concat!("JAVA_", "RETRY_CAP")],
                },
                Anchor {
                    id: "java-fn",
                    any_of: &[concat!("schedule", "JavaRetry")],
                },
            ],
        },
        BenchTask {
            id: "lang-cs-retry",
            prompt: concat!("How does `Schedule", "CsRetry` honour `Cs", "RetryCap`?"),
            symbol: Some(concat!("Schedule", "CsRetry")),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.cs"],
            anchors: &[
                Anchor {
                    id: "cs-cap",
                    any_of: &[concat!("Cs", "RetryCap")],
                },
                Anchor {
                    id: "cs-fn",
                    any_of: &[concat!("Schedule", "CsRetry")],
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
        let source = include_str!("lang_tasks.rs").to_ascii_lowercase();
        for task in lang_tasks() {
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
