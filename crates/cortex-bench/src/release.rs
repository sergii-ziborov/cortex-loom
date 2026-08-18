//! Release comparison contract.
//!
//! A published number must compare the same models, prompts, commits,
//! oracles, and retry budgets across: native agent, native + files,
//! raw Weavatrix, Cortex, and Serena/LSP.

/// Arms a release report is required to name.
pub const RELEASE_ARMS: &[&str] = &[
    "native",
    "native-files",
    "weavatrix-raw",
    "cortex-targeted",
    "serena",
];

/// Metrics a release report is required to name.
pub const RELEASE_METRICS: &[&str] = &[
    "critical_evidence_recall",
    "false_sufficiency",
    "task_success",
    "repair_loops",
    "paid_tokens_per_success",
    "stale_citations",
    "unsupported_claims",
    "deterministic_latency_ms",
    "upstream_turns",
];

/// How the current deterministic bench maps onto release arms.
#[must_use]
pub fn mapped_arm(kind: crate::ArmKind) -> Option<&'static str> {
    match kind {
        crate::ArmKind::Naive => Some("native-files"),
        crate::ArmKind::WeavatrixRaw => Some("weavatrix-raw"),
        crate::ArmKind::CortexLoomTargeted | crate::ArmKind::CortexLoomSource => {
            Some("cortex-targeted")
        }
        _ => None,
    }
}

/// Serena/LSP is off unless the operator points at a checkout.
#[must_use]
pub fn serena_configured() -> bool {
    std::env::var_os("CORTEX_SERENA_ROOT").is_some()
}

/// Why a Stage 4 arm is missing from this process.
#[must_use]
pub fn arm_availability(arm: &str) -> ArmAvailability {
    match arm {
        "serena" if !serena_configured() => ArmAvailability {
            available: false,
            reason: "CORTEX_SERENA_ROOT unset; Serena is not invoked without an explicit checkout",
        },
        "serena" => ArmAvailability {
            available: false,
            reason: "CORTEX_SERENA_ROOT is set, but this harness has no live Serena MCP client yet",
        },
        "native" => ArmAvailability {
            available: false,
            reason: "native agent task-success is not scored by the deterministic packet bench",
        },
        "native-files" | "weavatrix-raw" | "cortex-targeted" => ArmAvailability {
            available: true,
            reason: "mapped from cortex-bench naive / weavatrix-raw / cortex-source",
        },
        _ => ArmAvailability {
            available: false,
            reason: "unknown release arm",
        },
    }
}

/// Availability of one required release arm in this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmAvailability {
    pub available: bool,
    pub reason: &'static str,
}

/// Stage 4 envelope: required arms and metrics, plus who can run here.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseStatus {
    pub schema_version: &'static str,
    pub live_comparison: bool,
    pub serena_configured: bool,
    pub arms: Vec<ReleaseArmStatus>,
    pub metrics: &'static [&'static str],
}

/// One row of the Stage 4 table.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseArmStatus {
    pub arm: &'static str,
    pub available: bool,
    pub reason: &'static str,
}

/// Build the fail-closed Stage 4 status for this process.
#[must_use]
pub fn status() -> ReleaseStatus {
    let arms: Vec<ReleaseArmStatus> = RELEASE_ARMS
        .iter()
        .map(|arm| {
            let availability = arm_availability(arm);
            ReleaseArmStatus {
                arm,
                available: availability.available,
                reason: availability.reason,
            }
        })
        .collect();
    ReleaseStatus {
        schema_version: "cortex-release.v1",
        live_comparison: false,
        serena_configured: serena_configured(),
        arms,
        metrics: RELEASE_METRICS,
    }
}

/// Parse `cortex-bench release` and write the Stage 4 status.
///
/// # Errors
///
/// Returns when the output path cannot be written.
pub fn run_cli(arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let mut output = std::path::PathBuf::from(".cortex-loom/bench/release-status.json");
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--output" | "--out" => {
                output = std::path::PathBuf::from(next(&mut arguments, "--output")?);
            }
            other => return Err(format!("unknown release argument: {other}")),
        }
    }
    let report = status();
    let body = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("cannot encode release status: {error}"))?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    std::fs::write(&output, format!("{body}\n"))
        .map_err(|error| format!("cannot write {}: {error}", output.display()))?;
    let serena = report.arms.iter().find(|arm| arm.arm == "serena");
    match serena {
        Some(serena) => println!(
            "release: liveComparison={} serena={} ({})",
            report.live_comparison, serena.available, serena.reason
        ),
        None => println!(
            "release: liveComparison={} serena=missing-arm",
            report.live_comparison
        ),
    }
    println!("JSON report: {}", output.display());
    Ok(())
}

fn next(arguments: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

#[cfg(test)]
mod tests {
    use super::{RELEASE_ARMS, mapped_arm, serena_configured};
    use crate::ArmKind;

    #[test]
    fn release_arms_cover_the_required_comparison() {
        assert!(RELEASE_ARMS.contains(&"cortex-targeted"));
        assert!(RELEASE_ARMS.contains(&"serena"));
        assert_eq!(mapped_arm(ArmKind::Naive), Some("native-files"));
        assert!(!serena_configured() || std::env::var_os("CORTEX_SERENA_ROOT").is_some());
    }

    #[test]
    fn stage_four_status_fails_closed_without_serena() {
        let report = super::status();
        assert_eq!(report.schema_version, "cortex-release.v1");
        assert!(!report.live_comparison);
        let serena = report
            .arms
            .iter()
            .find(|arm| arm.arm == "serena")
            .expect("serena");
        assert!(!serena.available);
        assert!(serena.reason.contains("CORTEX_SERENA_ROOT"));
    }

    #[test]
    fn the_external_repo_catalog_has_at_least_twenty_entries() {
        let text = include_str!("../../../eval/public/external-repos.json");
        let value: serde_json::Value = serde_json::from_str(text).expect("catalog json");
        let repos = value["repositories"].as_array().expect("repositories");
        assert!(repos.len() >= 20, "{}", repos.len());
    }
}
