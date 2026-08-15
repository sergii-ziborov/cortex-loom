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
    fn the_external_repo_catalog_has_at_least_twenty_entries() {
        let text = include_str!("../../../eval/public/external-repos.json");
        let value: serde_json::Value = serde_json::from_str(text).expect("catalog json");
        let repos = value["repositories"].as_array().expect("repositories");
        assert!(repos.len() >= 20, "{}", repos.len());
    }
}
