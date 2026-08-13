//! Deterministic quality and safety scoring for methodology context arms.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cortex_domain::NodeKind;
use serde::{Deserialize, Serialize};

use crate::manifest::BenchmarkManifest;
use crate::scoreboard::ScoreboardRow;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceProbe {
    pub id: String,
    pub task: String,
    pub current_skill: String,
    pub raw_skill: String,
    pub expected: ExpectedSequenceBehavior,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedSequenceBehavior {
    pub sequence_id: String,
    pub active_step: u64,
    pub required_node_kinds: Vec<NodeKind>,
    #[serde(default)]
    pub forbidden_node_kinds: Vec<NodeKind>,
    pub required_evidence: Vec<String>,
    pub must_escalate: bool,
    pub must_not_claim_completion: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SequenceArm {
    None,
    CortexCurrent,
    SuperpowersRaw,
    CortexNative,
}

impl SequenceArm {
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::CortexCurrent => "cortex-current",
            Self::SuperpowersRaw => "superpowers-raw",
            Self::CortexNative => "cortex-native",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SequenceObservation {
    pub arm: SequenceArm,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub source_id: String,
    pub source_hash: String,
    pub context: String,
    pub selected_sequence: Option<String>,
    pub node_kinds: HashSet<NodeKind>,
    pub evidence_classes: BTreeSet<String>,
    pub escalates: bool,
    pub guards_completion: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)] // named independent benchmark assertions
pub struct BehaviorChecks {
    pub selected_sequence: bool,
    pub required_node_kinds: bool,
    pub forbidden_node_kinds: bool,
    pub required_evidence: bool,
    pub escalation: bool,
    pub completion_guard: bool,
}

impl BehaviorChecks {
    #[must_use]
    pub const fn passed(&self) -> bool {
        self.selected_sequence
            && self.required_node_kinds
            && self.forbidden_node_kinds
            && self.required_evidence
            && self.escalation
            && self.completion_guard
    }

    #[must_use]
    pub fn failed_names(&self) -> Vec<String> {
        [
            ("selected-sequence", self.selected_sequence),
            ("required-node-kinds", self.required_node_kinds),
            ("forbidden-node-kinds", self.forbidden_node_kinds),
            ("required-evidence", self.required_evidence),
            ("escalation", self.escalation),
            ("completion-guard", self.completion_guard),
        ]
        .into_iter()
        .filter(|(_, passed)| !passed)
        .map(|(name, _)| name.to_owned())
        .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceArmResult {
    pub arm: SequenceArm,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    pub source_id: String,
    pub source_hash: String,
    pub context_tokens: u32,
    pub context_chars: usize,
    pub checks: BehaviorChecks,
    pub passed: bool,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceScenarioResult {
    pub id: String,
    pub task: String,
    pub expected_sequence: String,
    pub arms: Vec<SequenceArmResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGate {
    pub promoted: bool,
    pub baselines_available: bool,
    pub scenarios: usize,
    pub native_passed: usize,
    pub regressions_vs_current: Vec<String>,
    pub regressions_vs_raw: Vec<String>,
    pub current_tokens: u64,
    pub native_tokens: u64,
    pub token_reduction_bps: i64,
    pub p95_sla_millis: u64,
    pub p95_sla_passed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceBenchReport {
    pub schema_version: String,
    pub historical: bool,
    pub manifest: BenchmarkManifest,
    pub fixture_hash: String,
    pub upstream_version: Option<String>,
    pub external_library: Option<ExternalLibraryStamp>,
    pub evidence_packet_hash: String,
    pub scenarios: Vec<SequenceScenarioResult>,
    pub totals: BTreeMap<String, ArmTotals>,
    pub gate: SequenceGate,
    pub scoreboard: Vec<ScoreboardRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalLibraryStamp {
    pub root_label: String,
    pub version: Option<String>,
    pub license_sha256: String,
    pub skill_sha256: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmTotals {
    pub available_scenarios: usize,
    pub passed_scenarios: usize,
    pub context_tokens: u64,
}

#[must_use]
pub fn score(probe: &SequenceProbe, observation: SequenceObservation) -> SequenceArmResult {
    let expected = &probe.expected;
    let checks = BehaviorChecks {
        selected_sequence: observation.arm != SequenceArm::CortexNative
            || observation.selected_sequence.as_deref() == Some(expected.sequence_id.as_str()),
        required_node_kinds: expected
            .required_node_kinds
            .iter()
            .all(|kind| observation.node_kinds.contains(kind)),
        forbidden_node_kinds: expected
            .forbidden_node_kinds
            .iter()
            .all(|kind| !observation.node_kinds.contains(kind)),
        required_evidence: expected
            .required_evidence
            .iter()
            .all(|item| observation.evidence_classes.contains(item)),
        escalation: !expected.must_escalate || observation.escalates,
        completion_guard: !expected.must_not_claim_completion || observation.guards_completion,
    };
    let passed = observation.available && checks.passed();
    let failures = if observation.available {
        checks.failed_names()
    } else {
        vec!["unavailable".to_owned()]
    };
    SequenceArmResult {
        arm: observation.arm,
        available: observation.available,
        unavailable_reason: observation.unavailable_reason,
        source_id: observation.source_id,
        source_hash: observation.source_hash,
        context_tokens: cortex_context::estimate_tokens(&observation.context),
        context_chars: observation.context.chars().count(),
        checks,
        passed,
        failures,
    }
}

#[must_use]
pub fn gate(results: &[SequenceScenarioResult], p95_sla_passed: bool) -> SequenceGate {
    let mut regressions_vs_current = Vec::new();
    let mut regressions_vs_raw = Vec::new();
    let mut baselines_available = !results.is_empty();
    let mut current_tokens = 0_u64;
    let mut native_tokens = 0_u64;
    let mut native_passed = 0;
    for result in results {
        let current = arm(result, SequenceArm::CortexCurrent);
        let raw = arm(result, SequenceArm::SuperpowersRaw);
        let native = arm(result, SequenceArm::CortexNative);
        baselines_available &= current.available && raw.available && native.available;
        current_tokens += u64::from(current.context_tokens);
        native_tokens += u64::from(native.context_tokens);
        native_passed += usize::from(native.passed);
        for (name, old, new) in check_pairs(&current.checks, &native.checks) {
            if old && !new {
                regressions_vs_current.push(format!("{}:{name}", result.id));
            }
        }
        for (name, old, new) in check_pairs(&raw.checks, &native.checks) {
            if old && !new {
                regressions_vs_raw.push(format!("{}:{name}", result.id));
            }
        }
    }
    let token_ratio = native_tokens
        .saturating_mul(10_000)
        .checked_div(current_tokens)
        .unwrap_or(10_000);
    let token_reduction_bps = 10_000 - i64::try_from(token_ratio).unwrap_or(i64::MAX);
    let promoted = baselines_available
        && native_passed == results.len()
        && regressions_vs_current.is_empty()
        && regressions_vs_raw.is_empty()
        && native_tokens <= current_tokens
        && p95_sla_passed;
    SequenceGate {
        promoted,
        baselines_available,
        scenarios: results.len(),
        native_passed,
        regressions_vs_current,
        regressions_vs_raw,
        current_tokens,
        native_tokens,
        token_reduction_bps,
        p95_sla_millis: 50,
        p95_sla_passed,
    }
}

fn arm(result: &SequenceScenarioResult, kind: SequenceArm) -> &SequenceArmResult {
    result
        .arms
        .iter()
        .find(|item| item.arm == kind)
        .expect("all sequence reports contain every stable arm")
}

fn check_pairs(old: &BehaviorChecks, new: &BehaviorChecks) -> [(&'static str, bool, bool); 6] {
    [
        (
            "selected-sequence",
            old.selected_sequence,
            new.selected_sequence,
        ),
        (
            "required-node-kinds",
            old.required_node_kinds,
            new.required_node_kinds,
        ),
        (
            "forbidden-node-kinds",
            old.forbidden_node_kinds,
            new.forbidden_node_kinds,
        ),
        (
            "required-evidence",
            old.required_evidence,
            new.required_evidence,
        ),
        ("escalation", old.escalation, new.escalation),
        (
            "completion-guard",
            old.completion_guard,
            new.completion_guard,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm_result(arm: SequenceArm, available: bool) -> SequenceArmResult {
        SequenceArmResult {
            arm,
            available,
            unavailable_reason: (!available).then(|| "fixture unavailable".to_owned()),
            source_id: arm.id().to_owned(),
            source_hash: String::new(),
            context_tokens: 10,
            context_chars: 40,
            checks: BehaviorChecks {
                selected_sequence: true,
                required_node_kinds: true,
                forbidden_node_kinds: true,
                required_evidence: true,
                escalation: true,
                completion_guard: true,
            },
            passed: available,
            failures: Vec::new(),
        }
    }

    #[test]
    fn fixture_has_at_least_28_scenarios() {
        let probes: Vec<SequenceProbe> =
            serde_json::from_str(include_str!("../fixtures/sequence-probes.json")).unwrap();
        assert!(probes.len() >= 28);
        let ids: BTreeSet<_> = probes.iter().map(|probe| probe.id.as_str()).collect();
        assert_eq!(ids.len(), probes.len());
    }

    #[test]
    fn gate_fails_closed_when_raw_baseline_is_unavailable() {
        let result = SequenceScenarioResult {
            id: "missing-raw".to_owned(),
            task: "debug a failure".to_owned(),
            expected_sequence: "root-cause-debugging".to_owned(),
            arms: vec![
                arm_result(SequenceArm::None, true),
                arm_result(SequenceArm::CortexCurrent, true),
                arm_result(SequenceArm::SuperpowersRaw, false),
                arm_result(SequenceArm::CortexNative, true),
            ],
        };

        let gate = gate(&[result], true);
        assert!(!gate.promoted);
        assert!(!gate.baselines_available);
    }
}
