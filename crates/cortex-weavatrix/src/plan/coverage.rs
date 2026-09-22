use serde_json::json;

use super::{EvidenceKind, PlanPolicy, PlannedOperation};

/// Whether the task asks for measured line coverage, not which tests to run.
#[must_use]
pub fn asks_for_coverage(task: &str) -> bool {
    const CUES: &[&str] = &[
        "coverage_map",
        "line coverage",
        "test coverage",
        "coverage report",
        "measured coverage",
        "unmeasured",
        "untested line",
        "untested lines",
        "uncovered line",
        "lcov",
        "llvm-cov",
        "tarpaulin",
        "покрыти",
        "покритт",
    ];
    let lower = crate::fold::fold_text(task);
    CUES.iter().any(|cue| lower.contains(cue))
}

/// Source-path substring for `coverage_map`. Never a report filename:
/// Weavatrix treats `path` as a filter on files inside the report.
pub(super) fn coverage_path_filter(identifiers: &[String]) -> Option<String> {
    crate::fold::named_crate_scope(identifiers).filter(|path| !is_coverage_report(path))
}

fn is_coverage_report(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.ends_with("lcov.info")
        || lower.ends_with("tarpaulin-report.json")
        || lower.ends_with("coverage.json")
        || lower.ends_with("coverage-final.json")
}

pub(super) fn coverage_map_op(filter: Option<&str>, policy: PlanPolicy) -> PlannedOperation {
    let mut arguments = json!({ "top_n": 24 });
    if let Some(path) = filter.filter(|value| !value.is_empty() && !is_coverage_report(value)) {
        arguments["path"] = json!(path);
    }
    PlannedOperation {
        id: "WX-COV",
        tool: "coverage_map",
        kind: EvidenceKind::CoverageMap,
        arguments,
        expected_tokens: policy.test_selection_tokens,
        bounded: false,
    }
}

#[cfg(test)]
mod tests {
    use super::{asks_for_coverage, coverage_path_filter};

    #[test]
    fn coverage_cues_are_narrow() {
        assert!(asks_for_coverage(
            "What is the measured line coverage of crates/cortex-weavatrix?"
        ));
        assert!(asks_for_coverage("Покажи покрытие тестами для api.rs"));
        assert!(!asks_for_coverage(
            "Which tests should I run after changing compile_context?"
        ));
        assert!(!asks_for_coverage(
            "Find and fix one real bug or dead production path."
        ));
    }

    #[test]
    fn coverage_path_filters_source_not_the_report_file() {
        assert_eq!(
            coverage_path_filter(&[
                "crates/cortex-weavatrix".to_owned(),
                ".weavatrix/coverage/lcov.info".to_owned()
            ])
            .as_deref(),
            Some("crates/cortex-weavatrix")
        );
        assert_eq!(
            coverage_path_filter(&[".weavatrix/coverage/lcov.info".to_owned()]),
            None
        );
    }
}
