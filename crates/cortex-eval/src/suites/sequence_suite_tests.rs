use super::*;

fn sample(arm: SequenceArm, passed: bool) -> SequenceLiveSample {
    SequenceLiveSample {
        scenario_id: "scenario".to_owned(),
        arm,
        repetition: 0,
        methodology_hash: "method".to_owned(),
        evidence_hash: "evidence".to_owned(),
        profile_id: "profile".to_owned(),
        model: "model".to_owned(),
        model_digest: "digest".to_owned(),
        runtime: "runtime".to_owned(),
        output: String::new(),
        claims: None,
        error: None,
        latency_ms: 1,
        prompt_tokens: 1,
        gate_passed: passed,
        failures: Vec::new(),
    }
}

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

#[test]
fn paired_regressions_cover_raw_and_current_baselines() {
    let samples = vec![
        sample(SequenceArm::CortexCurrent, true),
        sample(SequenceArm::SuperpowersRaw, true),
        sample(SequenceArm::CortexNative, false),
    ];
    assert_eq!(
        paired_regressions(&samples),
        [
            "scenario:repetition-0:cortex-current",
            "scenario:repetition-0:superpowers-raw",
        ]
    );
}

#[test]
fn deterministic_report_without_raw_baseline_cannot_promote_live_suite() {
    let stamp = DeterministicStamp {
        fixture_hash: "fixture".to_owned(),
        evidence_packet_hash: "evidence".to_owned(),
        gate: DeterministicGateStamp {
            promoted: true,
            baselines_available: false,
        },
    };
    assert!(!deterministic_baselines_ready(&stamp));
}

#[test]
fn missing_superpowers_root_makes_live_methodology_fail_closed() {
    let packets = methodology_packets(None).unwrap();
    assert!(!methodology_baselines_ready(&packets));
}
