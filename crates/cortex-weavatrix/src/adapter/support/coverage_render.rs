use std::fmt::Write as _;

use serde_json::Value;

/// `coverage_map` as file hit/found lines. A missing report is unmeasured,
/// never 0%. Matches `weavatrix-rust` 2.16.3: `measured_coverage.present`,
/// `files[].lines_hit` / `lines_found`.
pub(crate) fn coverage_map(value: &Value) -> Option<String> {
    if !looks_like_coverage(value) {
        return None;
    }
    let measured = measured(value);
    let mut out = if measured {
        String::from("coverage: measured\n")
    } else {
        String::from("coverage: unmeasured\n")
    };
    if let Some(report) = report_name(value) {
        let _ = writeln!(out, "report: {report}");
    }
    if let Some(reason) = unmeasured_reason(value) {
        let _ = writeln!(out, "reason: {reason}");
    }
    let files = files(value);
    if !files.is_empty() {
        let _ = writeln!(out, "files: {}", files.len());
        for file in files.iter().take(24) {
            out.push_str(file);
            out.push('\n');
        }
    }
    Some(out)
}

fn looks_like_coverage(value: &Value) -> bool {
    value.get("measured_coverage").is_some()
        || value.get("actualCoverage").is_some()
        || value.get("actual_coverage").is_some()
        || (value.get("files").and_then(Value::as_array).is_some()
            && (value.get("static_reachability").is_some()
                || value.get("measured").is_some()
                || value.get("present").is_some()
                || value.get("warning").is_some()))
}

fn measured(value: &Value) -> bool {
    if value
        .pointer("/measured_coverage/present")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return true;
    }
    value.get("measured").and_then(Value::as_bool) == Some(true)
        || value.get("present").and_then(Value::as_bool) == Some(true)
}

fn report_name(value: &Value) -> Option<&str> {
    value
        .pointer("/measured_coverage/report")
        .or_else(|| value.get("report"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty() && *name != "COMPLETE")
}

fn unmeasured_reason(value: &Value) -> Option<String> {
    if let Some(reason) = value
        .pointer("/measured_coverage/reason")
        .and_then(Value::as_str)
    {
        return Some(reason.to_owned());
    }
    if let Some(warning) = value.get("warning").and_then(Value::as_str) {
        return Some(warning.to_owned());
    }
    None
}

fn files(value: &Value) -> Vec<String> {
    let Some(items) = value.get("files").and_then(Value::as_array) else {
        return Vec::new();
    };
    items.iter().filter_map(file_line).collect()
}

fn file_line(file: &Value) -> Option<String> {
    let path = file.get("path").and_then(Value::as_str)?;
    if path.is_empty() {
        return None;
    }
    let hit = file
        .get("lines_hit")
        .or_else(|| file.get("hit"))
        .or_else(|| file.get("covered"))
        .and_then(Value::as_u64);
    let found = file
        .get("lines_found")
        .or_else(|| file.get("found"))
        .or_else(|| file.get("coverable"))
        .and_then(Value::as_u64);
    Some(match (hit, found) {
        (Some(hit), Some(found)) => format!("- {path} {hit}/{found}"),
        _ => format!("- {path}"),
    })
}

#[cfg(test)]
mod tests {
    use super::coverage_map;
    use serde_json::json;

    #[test]
    fn a_missing_report_is_unmeasured_not_zero() {
        let text = coverage_map(&json!({
            "status": "COMPLETE",
            "measured_coverage": {
                "present": false,
                "reason": "no supported measured coverage report exists in the analyzed repository"
            },
            "files": [],
            "static_reachability": {"test_files": 12},
            "warning": "static test reachability is not measured coverage"
        }))
        .expect("coverage shape");
        assert!(text.starts_with("coverage: unmeasured\n"));
        assert!(text.contains("no supported measured coverage report"));
        assert!(!text.contains("0%"));
    }

    #[test]
    fn measured_files_render_as_hit_found() {
        let text = coverage_map(&json!({
            "status": "COMPLETE",
            "measured_coverage": {
                "present": true,
                "report": ".weavatrix/coverage/lcov.info"
            },
            "files": [{
                "path": "crates/cortex-weavatrix/src/lib.rs",
                "lines_hit": 40,
                "lines_found": 48
            }]
        }))
        .expect("coverage shape");
        assert!(text.starts_with("coverage: measured\n"));
        assert!(text.contains("report: .weavatrix/coverage/lcov.info"));
        assert!(text.contains("files: 1"));
        assert!(text.contains("- crates/cortex-weavatrix/src/lib.rs 40/48"));
    }
}
