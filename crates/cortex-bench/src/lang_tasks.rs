//! Multi-language probe fixtures. Anchors live in `fixtures/langs/`.

use crate::Anchor;
use crate::tasks::BenchTask;

/// TS/JS, Python, Go, Java, and C# tasks over checked-in samples.
#[must_use]
pub const fn lang_tasks() -> &'static [BenchTask] {
    &[
        BenchTask {
            id: "lang-ts-retry",
            prompt: "How does `scheduleTsRetry` cap attempts with `TS_RETRY_CAP`?",
            symbol: Some("scheduleTsRetry"),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.ts"],
            anchors: &[
                Anchor {
                    id: "ts-cap",
                    any_of: &["TS_RETRY_CAP"],
                },
                Anchor {
                    id: "ts-fn",
                    any_of: &["function scheduleTsRetry"],
                },
            ],
        },
        BenchTask {
            id: "lang-js-retry",
            prompt: "How does `scheduleJsRetry` cap attempts with `JS_RETRY_CAP`?",
            symbol: Some("scheduleJsRetry"),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.js"],
            anchors: &[
                Anchor {
                    id: "js-cap",
                    any_of: &["JS_RETRY_CAP"],
                },
                Anchor {
                    id: "js-fn",
                    any_of: &["function scheduleJsRetry"],
                },
            ],
        },
        BenchTask {
            id: "lang-py-retry",
            prompt: "How does `schedule_py_retry` use `PY_RETRY_CAP`?",
            symbol: Some("schedule_py_retry"),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.py"],
            anchors: &[
                Anchor {
                    id: "py-cap",
                    any_of: &["PY_RETRY_CAP"],
                },
                Anchor {
                    id: "py-fn",
                    any_of: &["def schedule_py_retry"],
                },
            ],
        },
        BenchTask {
            id: "lang-go-retry",
            prompt: "How does `ScheduleGoRetry` honour `GoRetryCap`?",
            symbol: Some("ScheduleGoRetry"),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.go"],
            anchors: &[
                Anchor {
                    id: "go-cap",
                    any_of: &["GoRetryCap"],
                },
                Anchor {
                    id: "go-fn",
                    any_of: &["func ScheduleGoRetry"],
                },
            ],
        },
        BenchTask {
            id: "lang-java-retry",
            prompt: "How does `scheduleJavaRetry` honour `JAVA_RETRY_CAP`?",
            symbol: Some("scheduleJavaRetry"),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.java"],
            anchors: &[
                Anchor {
                    id: "java-cap",
                    any_of: &["JAVA_RETRY_CAP"],
                },
                Anchor {
                    id: "java-fn",
                    any_of: &["scheduleJavaRetry"],
                },
            ],
        },
        BenchTask {
            id: "lang-cs-retry",
            prompt: "How does `ScheduleCsRetry` honour `CsRetryCap`?",
            symbol: Some("ScheduleCsRetry"),
            naive_globs: &["crates/cortex-bench/fixtures/langs/*.cs"],
            anchors: &[
                Anchor {
                    id: "cs-cap",
                    any_of: &["CsRetryCap"],
                },
                Anchor {
                    id: "cs-fn",
                    any_of: &["ScheduleCsRetry"],
                },
            ],
        },
    ]
}
