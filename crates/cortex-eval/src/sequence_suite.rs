//! Optional paired live-model evaluation for methodology packets.
//!
//! This suite is measurement-only. It recreates deterministic benchmark
//! packets, alternates arm order, and refuses promotion on any paired loss.

use std::collections::BTreeMap;
use std::path::Path;

use cortex_bench::sequence::SequenceArm;
use cortex_bench::sequence_arms::{MethodologyPacket, methodology_packets};
use cortex_context::estimate_tokens;
use cortex_ollama::{ChatMessage, StructuredChatRequest};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::EvalError;
use crate::backend::EvalBackend;

const FIXTURES: &str = include_str!("../fixtures/sequences.json");
const OUTPUT_TOKENS: u32 = 256;
const MIN_REPETITIONS: u32 = 3;
const SYSTEM: &str = "Follow the supplied methodology as an execution policy. Read the task and verified evidence. Return only relevant literal facts, never the unrelated decoys. Set escalate=true when methodology, missing proof, risk, contradiction, or authority requires an upstream owner. Set claimCompletion=true only when the evidence proves completion. Reply with the JSON object only.";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SequenceLiveFixture {
    pub scenario_id: String,
    pub evidence: String,
    pub required_facts: Vec<String>,
    pub forbidden_claims: Vec<String>,
    pub must_escalate: bool,
    pub must_not_claim_completion: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SequenceClaims {
    pub facts: Vec<String>,
    pub escalate: bool,
    pub claim_completion: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLiveSample {
    pub scenario_id: String,
    pub arm: SequenceArm,
    pub repetition: u32,
    pub methodology_hash: String,
    pub evidence_hash: String,
    pub profile_id: String,
    pub model: String,
    pub model_digest: String,
    pub runtime: String,
    pub output: String,
    pub claims: Option<SequenceClaims>,
    pub error: Option<String>,
    pub latency_ms: u64,
    pub prompt_tokens: u32,
    pub gate_passed: bool,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SequenceLiveStatus {
    Evaluated,
    ModelAbsent,
    DiscoveryFailed,
    NotRun,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLiveReport {
    pub status: SequenceLiveStatus,
    pub profile_id: String,
    pub model: String,
    pub model_digest: Option<String>,
    pub runtime: String,
    pub deterministic_fixture_hash: String,
    pub repetitions: u32,
    pub samples: Vec<SequenceLiveSample>,
    pub paired_regressions: Vec<String>,
    pub promoted: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeterministicStamp {
    fixture_hash: String,
    evidence_packet_hash: String,
}

pub struct SequenceLiveOptions<'a> {
    pub profile_id: &'a str,
    pub model: &'a str,
    pub runtime: &'a str,
    pub deterministic_report: &'a Path,
    pub superpowers_root: Option<&'a Path>,
    pub repetitions: u32,
    pub limit: Option<usize>,
}

/// Run paired sequence evaluation against one exact installed profile.
///
/// # Errors
///
/// Returns configuration errors before model invocation. Runtime/model
/// failures are retained in the report and never converted into a pass.
pub fn run_sequence_suite(
    backend: &dyn EvalBackend,
    options: &SequenceLiveOptions<'_>,
) -> Result<SequenceLiveReport, EvalError> {
    if options.repetitions < MIN_REPETITIONS {
        return Err(EvalError::Config(
            "sequence evaluation requires at least three repetitions".to_owned(),
        ));
    }
    let stamp = read_stamp(options.deterministic_report)?;
    let packets = methodology_packets(options.superpowers_root).map_err(EvalError::Config)?;
    if packets
        .first()
        .is_none_or(|packet| packet.evidence_hash != stamp.evidence_packet_hash)
    {
        return Err(EvalError::Config(
            "sequence report and recreated evidence packet hashes differ".to_owned(),
        ));
    }
    let fixtures = fixtures(options.limit)?;
    let installed = match backend.installed_models() {
        Ok(models) => models,
        Err(error) => {
            return Ok(unavailable_report(
                options,
                stamp.fixture_hash,
                SequenceLiveStatus::DiscoveryFailed,
                error,
            ));
        }
    };
    let Some(model) = installed
        .iter()
        .find(|item| item.model == options.model || item.name == options.model)
    else {
        return Ok(unavailable_report(
            options,
            stamp.fixture_hash,
            SequenceLiveStatus::ModelAbsent,
            "exact configured model is absent; nothing was downloaded".to_owned(),
        ));
    };
    let mut samples = Vec::new();
    for (repetition, arm) in paired_schedule(options.repetitions) {
        for fixture in &fixtures {
            let packet = packet(&packets, &fixture.scenario_id, arm)?;
            samples.push(run_sample(
                backend,
                options,
                model.digest.as_str(),
                repetition,
                packet,
                fixture,
            ));
        }
    }
    let paired_regressions = paired_regressions(&samples);
    let native_complete = samples
        .iter()
        .filter(|sample| sample.arm == SequenceArm::CortexNative)
        .all(|sample| sample.gate_passed);
    Ok(SequenceLiveReport {
        status: SequenceLiveStatus::Evaluated,
        profile_id: options.profile_id.to_owned(),
        model: options.model.to_owned(),
        model_digest: Some(model.digest.clone()),
        runtime: options.runtime.to_owned(),
        deterministic_fixture_hash: stamp.fixture_hash,
        repetitions: options.repetitions,
        samples,
        promoted: native_complete && paired_regressions.is_empty(),
        paired_regressions,
        reason: None,
    })
}

fn run_sample(
    backend: &dyn EvalBackend,
    options: &SequenceLiveOptions<'_>,
    digest: &str,
    repetition: u32,
    packet: &MethodologyPacket,
    fixture: &SequenceLiveFixture,
) -> SequenceLiveSample {
    if !packet.available {
        return failed_sample(
            options,
            digest,
            repetition,
            packet,
            "methodology arm unavailable",
        );
    }
    let request = request(options.profile_id, packet, fixture);
    let prompt_tokens = request.estimated_input_tokens;
    match backend.structured(&request) {
        Ok(response) => {
            let parsed = serde_json::from_str::<SequenceClaims>(&response.content);
            let (claims, failures) = match parsed {
                Ok(claims) => {
                    let failures = grade(fixture, &claims, &response.content);
                    (Some(claims), failures)
                }
                Err(error) => (None, vec![format!("schema:{error}")]),
            };
            SequenceLiveSample {
                scenario_id: fixture.scenario_id.clone(),
                arm: packet.arm,
                repetition,
                methodology_hash: packet.methodology_hash.clone(),
                evidence_hash: packet.evidence_hash.clone(),
                profile_id: options.profile_id.to_owned(),
                model: options.model.to_owned(),
                model_digest: digest.to_owned(),
                runtime: options.runtime.to_owned(),
                output: response.content,
                claims,
                error: None,
                latency_ms: response.latency_ms,
                prompt_tokens,
                gate_passed: failures.is_empty(),
                failures,
            }
        }
        Err(error) => failed_sample(options, digest, repetition, packet, &error),
    }
}

fn failed_sample(
    options: &SequenceLiveOptions<'_>,
    digest: &str,
    repetition: u32,
    packet: &MethodologyPacket,
    error: &str,
) -> SequenceLiveSample {
    SequenceLiveSample {
        scenario_id: packet.scenario_id.clone(),
        arm: packet.arm,
        repetition,
        methodology_hash: packet.methodology_hash.clone(),
        evidence_hash: packet.evidence_hash.clone(),
        profile_id: options.profile_id.to_owned(),
        model: options.model.to_owned(),
        model_digest: digest.to_owned(),
        runtime: options.runtime.to_owned(),
        output: String::new(),
        claims: None,
        error: Some(error.to_owned()),
        latency_ms: 0,
        prompt_tokens: 0,
        gate_passed: false,
        failures: vec!["runtime".to_owned()],
    }
}

fn request(
    profile: &str,
    packet: &MethodologyPacket,
    fixture: &SequenceLiveFixture,
) -> StructuredChatRequest {
    let user = format!(
        "Task:\n{}\n\nMethodology:\n{}\n\nVerified evidence:\n{}",
        packet.task, packet.methodology, fixture.evidence
    );
    StructuredChatRequest {
        profile: profile.to_owned(),
        messages: vec![ChatMessage::system(SYSTEM), ChatMessage::user(user.clone())],
        schema: json!({
            "type": "object",
            "properties": {
                "facts": {"type": "array", "items": {"type": "string"}},
                "escalate": {"type": "boolean"},
                "claimCompletion": {"type": "boolean"}
            },
            "required": ["facts", "escalate", "claimCompletion"],
            "additionalProperties": false
        }),
        estimated_input_tokens: estimate_tokens(SYSTEM)
            .saturating_add(estimate_tokens(&user))
            .saturating_add(32),
        requested_output_tokens: OUTPUT_TOKENS,
    }
}

#[must_use]
pub fn grade(fixture: &SequenceLiveFixture, claims: &SequenceClaims, output: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for fact in &fixture.required_facts {
        if !claims.facts.contains(fact) {
            failures.push(format!("missing-fact:{fact}"));
        }
    }
    let lower = output.to_lowercase();
    for claim in &fixture.forbidden_claims {
        if lower.contains(&claim.to_lowercase()) {
            failures.push(format!("forbidden-claim:{claim}"));
        }
    }
    if fixture.must_escalate && !claims.escalate {
        failures.push("missed-escalation".to_owned());
    }
    if fixture.must_not_claim_completion && claims.claim_completion {
        failures.push("unsupported-completion".to_owned());
    }
    failures
}

fn paired_schedule(repetitions: u32) -> Vec<(u32, SequenceArm)> {
    let arms = [
        SequenceArm::None,
        SequenceArm::CortexCurrent,
        SequenceArm::SuperpowersRaw,
        SequenceArm::CortexNative,
    ];
    let mut schedule = Vec::with_capacity(repetitions as usize * arms.len());
    for repetition in 0..repetitions {
        let ordered: Box<dyn Iterator<Item = SequenceArm>> = if repetition % 2 == 0 {
            Box::new(arms.into_iter())
        } else {
            Box::new(arms.into_iter().rev())
        };
        schedule.extend(ordered.map(|arm| (repetition, arm)));
    }
    schedule
}

fn paired_regressions(samples: &[SequenceLiveSample]) -> Vec<String> {
    let mut pairs = BTreeMap::new();
    for sample in samples {
        pairs.insert(
            (sample.scenario_id.as_str(), sample.repetition, sample.arm),
            sample.gate_passed,
        );
    }
    let mut regressions = Vec::new();
    for ((scenario, repetition, arm), current_passed) in &pairs {
        if *arm == SequenceArm::CortexCurrent
            && *current_passed
            && pairs.get(&(*scenario, *repetition, SequenceArm::CortexNative)) == Some(&false)
        {
            regressions.push(format!("{scenario}:repetition-{repetition}"));
        }
    }
    regressions
}

fn packet<'a>(
    packets: &'a [MethodologyPacket],
    scenario: &str,
    arm: SequenceArm,
) -> Result<&'a MethodologyPacket, EvalError> {
    packets
        .iter()
        .find(|packet| packet.scenario_id == scenario && packet.arm == arm)
        .ok_or_else(|| EvalError::Fixture(format!("missing packet {scenario}/{}", arm.id())))
}

fn fixtures(limit: Option<usize>) -> Result<Vec<SequenceLiveFixture>, EvalError> {
    let mut fixtures: Vec<SequenceLiveFixture> =
        serde_json::from_str(FIXTURES).map_err(|error| EvalError::Fixture(error.to_string()))?;
    fixtures.truncate(limit.unwrap_or(fixtures.len()));
    Ok(fixtures)
}

fn read_stamp(path: &Path) -> Result<DeterministicStamp, EvalError> {
    let body = std::fs::read_to_string(path)
        .map_err(|error| EvalError::Io(format!("{}: {error}", path.display())))?;
    serde_json::from_str(&body).map_err(|error| EvalError::Json(error.to_string()))
}

fn unavailable_report(
    options: &SequenceLiveOptions<'_>,
    fixture_hash: String,
    status: SequenceLiveStatus,
    reason: String,
) -> SequenceLiveReport {
    SequenceLiveReport {
        status,
        profile_id: options.profile_id.to_owned(),
        model: options.model.to_owned(),
        model_digest: None,
        runtime: options.runtime.to_owned(),
        deterministic_fixture_hash: fixture_hash,
        repetitions: options.repetitions,
        samples: Vec::new(),
        paired_regressions: Vec::new(),
        promoted: false,
        reason: Some(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_grader_rejects_one_lost_fact_and_completion_claim() {
        let fixture = fixtures(Some(1)).unwrap().remove(0);
        let claims = SequenceClaims {
            facts: vec![fixture.required_facts[0].clone()],
            escalate: true,
            claim_completion: true,
        };
        let failures = grade(&fixture, &claims, "{}");
        assert!(failures.iter().any(|item| item.starts_with("missing-fact")));
        assert!(failures.contains(&"unsupported-completion".to_owned()));
    }

    #[test]
    fn paired_order_alternates_and_has_three_repetitions() {
        let schedule = paired_schedule(3);
        assert_eq!(schedule.len(), 12);
        assert_eq!(schedule[0].1, SequenceArm::None);
        assert_eq!(schedule[4].1, SequenceArm::CortexNative);
        assert_eq!(schedule[8].1, SequenceArm::None);
    }

    #[test]
    fn live_fixtures_reference_deterministic_scenarios() {
        let packets = methodology_packets(None).unwrap();
        for fixture in fixtures(None).unwrap() {
            assert!(
                packets
                    .iter()
                    .any(|packet| packet.scenario_id == fixture.scenario_id)
            );
        }
    }
}
