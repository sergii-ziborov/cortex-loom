//! Four-arm methodology benchmark runner.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use cortex_domain::{EdgeKind, GraphDocument, NodeKind};
use cortex_sequences::{active_step_packet, candidate_templates, instantiate_template};
use sha2::{Digest, Sha256};

use crate::external_skills::ExternalSkillLibrary;
use crate::manifest::{BenchmarkManifest, McpManifest};
use crate::scoreboard::{FailureClass, ScoreboardRow};
use crate::sequence::{
    ArmTotals, SequenceArm, SequenceBenchReport, SequenceObservation, SequenceProbe,
    SequenceScenarioResult, gate, score,
};

const FIXTURES: &str = include_str!("../fixtures/sequence-probes.json");
const EVIDENCE_IDS: [&str; 3] = [
    "revision:sequence-bench-v1",
    "evidence:declared-contract",
    "evidence:focused-test",
];

#[derive(Debug, Clone)]
pub struct MethodologyPacket {
    pub scenario_id: String,
    pub task: String,
    pub arm: SequenceArm,
    pub available: bool,
    pub methodology_hash: String,
    pub evidence_hash: String,
    pub methodology: String,
}

/// Recreate the exact methodology inputs used by the deterministic report.
/// Bodies stay out of the JSON report so raw third-party skill text is never
/// accidentally vendored as a benchmark artifact.
pub fn methodology_packets(
    superpowers_root: Option<&Path>,
) -> Result<Vec<MethodologyPacket>, String> {
    let probes: Vec<SequenceProbe> = serde_json::from_str(FIXTURES)
        .map_err(|error| format!("invalid sequence fixtures: {error}"))?;
    let upstream = ExternalSkillLibrary::load(superpowers_root);
    let evidence_ids: Vec<String> = EVIDENCE_IDS.iter().map(ToString::to_string).collect();
    let evidence_hash = digest(&evidence_ids.join("\n"));
    let mut latencies = Vec::new();
    let mut packets = Vec::with_capacity(probes.len() * 4);
    for probe in &probes {
        for observation in observations(probe, &upstream, &evidence_ids, &mut latencies)? {
            packets.push(MethodologyPacket {
                scenario_id: probe.id.clone(),
                task: probe.task.clone(),
                arm: observation.arm,
                available: observation.available,
                methodology_hash: observation.source_hash,
                evidence_hash: evidence_hash.clone(),
                methodology: observation.context,
            });
        }
    }
    Ok(packets)
}

/// Run the static four-arm comparison.
///
/// # Errors
///
/// Returns an error when bundled fixtures or Cortex-native templates are
/// invalid. An unavailable external Superpowers root is represented in the
/// report instead of failing or silently removing that arm.
pub fn run(superpowers_root: Option<&Path>) -> Result<SequenceBenchReport, String> {
    let probes: Vec<SequenceProbe> = serde_json::from_str(FIXTURES)
        .map_err(|error| format!("invalid sequence fixtures: {error}"))?;
    if probes.len() < 28 {
        return Err("sequence benchmark requires at least 28 scenarios".to_owned());
    }
    let upstream = ExternalSkillLibrary::load(superpowers_root);
    let evidence_ids: Vec<String> = EVIDENCE_IDS.iter().map(ToString::to_string).collect();
    let mut native_latencies = Vec::with_capacity(probes.len());
    let mut scenarios = Vec::with_capacity(probes.len());
    for probe in &probes {
        let observations = observations(probe, &upstream, &evidence_ids, &mut native_latencies)?;
        let arms = observations
            .into_iter()
            .map(|observation| score(probe, observation))
            .collect();
        scenarios.push(SequenceScenarioResult {
            id: probe.id.clone(),
            task: probe.task.clone(),
            expected_sequence: probe.expected.sequence_id.clone(),
            arms,
        });
    }
    let p95_sla_passed = p95_micros(&mut native_latencies) <= 50_000;
    let totals = totals(&scenarios);
    let gate = gate(&scenarios, p95_sla_passed);
    let scoreboard = sequence_scoreboard(&scenarios);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let command: Vec<String> = std::env::args().collect();
    let manifest =
        BenchmarkManifest::detect("sequence-v2", &root, &command, McpManifest::in_process());
    Ok(SequenceBenchReport {
        schema_version: manifest.report_schema.clone(),
        historical: false,
        manifest,
        fixture_hash: digest(FIXTURES),
        upstream_version: upstream
            .stamp
            .as_ref()
            .and_then(|stamp| stamp.version.clone()),
        external_library: upstream.stamp,
        evidence_packet_hash: digest(&evidence_ids.join("\n")),
        scenarios,
        totals,
        gate,
        scoreboard,
    })
}

fn sequence_scoreboard(scenarios: &[SequenceScenarioResult]) -> Vec<ScoreboardRow> {
    let mut rows = Vec::new();
    for scenario in scenarios {
        for arm in &scenario.arms {
            let possible = 6;
            let earned = possible - arm.failures.len().min(possible);
            let mut row =
                ScoreboardRow::new("sequence", &scenario.id, arm.arm.id(), 0, earned, possible);
            row.tokens.selected = Some(arm.context_tokens);
            if !row.task_success {
                row.failure_class = Some(match arm.arm {
                    crate::sequence::SequenceArm::CortexCurrent
                    | crate::sequence::SequenceArm::CortexNative => FailureClass::CortexBug,
                    crate::sequence::SequenceArm::None
                    | crate::sequence::SequenceArm::SuperpowersRaw => FailureClass::HarnessBug,
                });
            }
            row.refresh_verdict();
            rows.push(row);
        }
    }
    rows
}

/// Parse the `cortex-bench sequence` command and write its stable report.
pub fn run_cli(arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let mut root = None;
    let mut output = PathBuf::from(".cortex-loom/bench/sequence-report.json");
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--superpowers-root" => {
                root = Some(PathBuf::from(next(&mut arguments, "--superpowers-root")?));
            }
            "--output" | "--out" => {
                output = PathBuf::from(next(&mut arguments, "--output")?);
            }
            other => return Err(format!("unknown sequence argument: {other}")),
        }
    }
    let report = run(root.as_deref())?;
    let body = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("could not serialize sequence report: {error}"))?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    std::fs::write(&output, format!("{body}\n"))
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    println!(
        "sequence: native {}/{}; tokens {} -> {}; promoted: {}",
        report.gate.native_passed,
        report.gate.scenarios,
        report.gate.current_tokens,
        report.gate.native_tokens,
        report.gate.promoted
    );
    println!("JSON report: {}", output.display());
    Ok(())
}

fn observations(
    probe: &SequenceProbe,
    upstream: &ExternalSkillLibrary,
    evidence_ids: &[String],
    native_latencies: &mut Vec<u64>,
) -> Result<Vec<SequenceObservation>, String> {
    let none = from_text(SequenceArm::None, "none", "", true, None);
    let current = match cortex_skills::bundled_skills()
        .iter()
        .find(|skill| skill.id == probe.current_skill)
    {
        Some(skill) => from_text(
            SequenceArm::CortexCurrent,
            skill.id,
            skill.markdown,
            true,
            None,
        ),
        None => from_text(
            SequenceArm::CortexCurrent,
            &probe.current_skill,
            "",
            false,
            Some("bundled Cortex skill is absent".to_owned()),
        ),
    };
    let raw = raw_observation(upstream, &probe.raw_skill);
    let started = Instant::now();
    let native = native_observation(probe, evidence_ids)?;
    native_latencies.push(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
    Ok(vec![none, current, raw, native])
}

fn native_observation(
    probe: &SequenceProbe,
    evidence_ids: &[String],
) -> Result<SequenceObservation, String> {
    let graph = instantiate_template(
        &probe.expected.sequence_id,
        &format!("bench-{}", probe.id),
        &probe.id,
    )
    .map_err(|error| error.to_string())?;
    let node = graph
        .nodes
        .iter()
        .find(|node| node.id == format!("step-{}", probe.expected.active_step))
        .ok_or_else(|| format!("{}: active step is absent", probe.id))?;
    let packet = active_step_packet(&graph, &node.id, evidence_ids)
        .map_err(|error| format!("{}: {error}", probe.id))?;
    let context = serde_json::to_string(&packet).map_err(|error| error.to_string())?;
    let selected_sequence = candidate_templates(&probe.task)
        .into_iter()
        .find(|candidate| candidate.template_id == probe.expected.sequence_id)
        .map(|candidate| candidate.template_id);
    Ok(SequenceObservation {
        arm: SequenceArm::CortexNative,
        available: true,
        unavailable_reason: None,
        source_id: format!("sequence:{}@1.0.0", probe.expected.sequence_id),
        source_hash: digest(&canonical_graph_source(&graph)),
        context,
        selected_sequence,
        node_kinds: graph.nodes.iter().map(|node| node.kind).collect(),
        evidence_classes: graph_evidence(&graph),
        escalates: graph
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::Escalates)
            && graph
                .nodes
                .iter()
                .any(|node| matches!(node.kind, NodeKind::Handoff | NodeKind::UpstreamAgent)),
        guards_completion: graph
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::EvidenceGate)
            && graph
                .nodes
                .iter()
                .any(|node| node.kind == NodeKind::Handoff),
    })
}

fn canonical_graph_source(graph: &GraphDocument) -> String {
    let mut kinds: Vec<_> = graph
        .nodes
        .iter()
        .map(|node| format!("{}:{}", node.id, node.kind.as_str()))
        .collect();
    kinds.sort();
    kinds.join("\n")
}

fn graph_evidence(graph: &GraphDocument) -> BTreeSet<String> {
    graph
        .nodes
        .iter()
        .filter_map(|node| node.config.get("requiredEvidence")?.as_array())
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn from_text(
    arm: SequenceArm,
    source_id: &str,
    context: &str,
    available: bool,
    unavailable_reason: Option<String>,
) -> SequenceObservation {
    let lowercase = context.to_lowercase();
    SequenceObservation {
        arm,
        available,
        unavailable_reason,
        source_id: source_id.to_owned(),
        source_hash: digest(context),
        context: context.to_owned(),
        selected_sequence: None,
        node_kinds: infer_node_kinds(&lowercase),
        evidence_classes: infer_evidence(&lowercase),
        escalates: contains_any(&lowercase, &["escalat", "hand off", "handoff", "stop"]),
        guards_completion: contains_any(
            &lowercase,
            &[
                "before completion",
                "before claim",
                "evidence before",
                "verify",
                "verification",
            ],
        ),
    }
}

fn raw_observation(library: &ExternalSkillLibrary, skill: &str) -> SequenceObservation {
    match library.body(skill) {
        Ok(body) => from_text(SequenceArm::SuperpowersRaw, skill, body, true, None),
        Err(reason) => from_text(SequenceArm::SuperpowersRaw, skill, "", false, Some(reason)),
    }
}

fn infer_node_kinds(text: &str) -> HashSet<NodeKind> {
    let mut kinds = HashSet::new();
    for kind in NodeKind::ALL {
        if text.contains(kind.as_str()) || text.contains(&kind.as_str().replace('_', " ")) {
            kinds.insert(kind);
        }
    }
    insert_when(&mut kinds, NodeKind::TestGate, text, &["test", "reproduc"]);
    insert_when(
        &mut kinds,
        NodeKind::EvidenceGate,
        text,
        &["evidence", "verify", "citation"],
    );
    insert_when(
        &mut kinds,
        NodeKind::ReviewGate,
        text,
        &["review", "feedback", "diff"],
    );
    insert_when(
        &mut kinds,
        NodeKind::QualityGate,
        text,
        &["quality", "policy", "independent", "parallel"],
    );
    insert_when(
        &mut kinds,
        NodeKind::AgentTask,
        text,
        &["plan", "approach", "agent"],
    );
    insert_when(
        &mut kinds,
        NodeKind::UpstreamAgent,
        text,
        &["escalat", "agent", "hand off"],
    );
    kinds
}

fn infer_evidence(text: &str) -> BTreeSet<String> {
    let mut evidence = BTreeSet::new();
    insert_evidence(&mut evidence, "test output", text, &["test", "reproduc"]);
    insert_evidence(
        &mut evidence,
        "review findings",
        text,
        &["review", "feedback"],
    );
    insert_evidence(&mut evidence, "diff", text, &["diff", "change"]);
    insert_evidence(
        &mut evidence,
        "policy result",
        text,
        &["policy", "quality", "independent", "authority"],
    );
    insert_evidence(
        &mut evidence,
        "current-attempt evidence",
        text,
        &["evidence", "citation", "source"],
    );
    insert_evidence(
        &mut evidence,
        "cited repository evidence",
        text,
        &["repository", "source", "citation"],
    );
    evidence
}

fn insert_when(set: &mut HashSet<NodeKind>, kind: NodeKind, text: &str, cues: &[&str]) {
    if contains_any(text, cues) {
        set.insert(kind);
    }
}

fn insert_evidence(set: &mut BTreeSet<String>, value: &str, text: &str, cues: &[&str]) {
    if contains_any(text, cues) {
        set.insert(value.to_owned());
    }
}

fn contains_any(text: &str, cues: &[&str]) -> bool {
    cues.iter().any(|cue| text.contains(cue))
}

fn totals(scenarios: &[SequenceScenarioResult]) -> BTreeMap<String, ArmTotals> {
    let mut totals = BTreeMap::<String, ArmTotals>::new();
    for scenario in scenarios {
        for result in &scenario.arms {
            let total = totals.entry(result.arm.id().to_owned()).or_default();
            total.available_scenarios += usize::from(result.available);
            total.passed_scenarios += usize::from(result.passed);
            total.context_tokens += u64::from(result.context_tokens);
        }
    }
    totals
}

fn p95_micros(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    let index = values
        .len()
        .saturating_mul(95)
        .div_ceil(100)
        .saturating_sub(1);
    values.get(index).copied().unwrap_or(u64::MAX)
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn next(arguments: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_superpowers_is_explicit_and_report_stays_complete() {
        let report = run(None).unwrap();
        assert_eq!(report.scenarios.len(), 28);
        assert!(
            report
                .scenarios
                .iter()
                .all(|scenario| scenario.arms.len() == 4)
        );
        assert!(
            report
                .scenarios
                .iter()
                .all(|scenario| !scenario.arms[2].available)
        );
    }

    #[test]
    fn bundled_run_is_byte_deterministic() {
        let first = serde_json::to_vec_pretty(&run(None).unwrap()).unwrap();
        let second = serde_json::to_vec_pretty(&run(None).unwrap()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn native_arm_has_no_regression_and_uses_no_more_context() {
        let report = run(None).unwrap();
        assert!(report.gate.regressions_vs_current.is_empty());
        assert!(report.gate.native_tokens <= report.gate.current_tokens);
        assert_eq!(report.gate.native_passed, 28);
    }
}
