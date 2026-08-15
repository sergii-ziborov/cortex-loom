//! `micro_extract` suite tests: the gate, the fixtures, and the live runner.

use cortex_llm::MicroExtractRequest;
use cortex_ollama::ModelInfo;
use cortex_router::ModelTier;

use crate::backend::ScriptedBackend;
use crate::fixtures::micro_extraction_fixtures;
use crate::metrics::MicroExtractionAggregate;
use crate::prompts::{micro_extraction_request, parse_micro_extraction};
use crate::runner::{EvalProfile, ProfileStatus, run_micro_extract_profile};
use crate::verdict::{VerdictReason, judge_micro_extract};

fn test_model() -> Vec<ModelInfo> {
    vec![ModelInfo {
        name: "qwen-test:4b".to_owned(),
        model: "qwen-test:4b".to_owned(),
        size: 1,
        digest: "sha256:test".to_owned(),
    }]
}

fn profile() -> EvalProfile {
    EvalProfile {
        id: "candidate".to_owned(),
        tier: ModelTier::LocalSmall,
        model: "qwen-test:4b".to_owned(),
    }
}

#[test]
fn micro_extract_fixtures_reject_injection_invention_and_duplicates() {
    let fixtures = micro_extraction_fixtures().expect("micro fixtures");
    assert!(fixtures.len() >= 8);
    for fixture in fixtures {
        let fields: Vec<&str> = fixture.allowed_fields.iter().map(String::as_str).collect();
        let request = MicroExtractRequest::new(&fixture.verified_input, &fields).unwrap();
        let prompt = micro_extraction_request("candidate", &request);
        assert_eq!(prompt.requested_output_tokens, 128);
        assert!(parse_micro_extraction(&request, &fixture.gold.to_string()).is_ok());
        assert!(
            fixture
                .rejected_outputs
                .iter()
                .all(|output| parse_micro_extraction(&request, &output.to_string()).is_err())
        );
    }
}

#[test]
fn micro_extract_gate_has_no_average_away_escape_hatch() {
    let passing = MicroExtractionAggregate {
        samples: 20,
        schema_valid_rate: 1.0,
        field_precision: 0.96,
        field_recall: 0.95,
        exact_match_rate: 0.90,
        unsupported_fields: 0,
        authority_outputs: 0,
        p95_latency_ms: 1_500,
    };
    assert!(judge_micro_extract(Some(&passing)).pass);
    let mut one_invention = passing;
    one_invention.unsupported_fields = 1;
    assert!(!judge_micro_extract(Some(&one_invention)).pass);
    assert!(!judge_micro_extract(None).pass);
}

#[test]
fn the_micro_extract_suite_scores_the_holdout_and_fails_closed_on_one_invention() {
    let fixtures = micro_extraction_fixtures().expect("holdout");
    let gold: Vec<String> = fixtures
        .iter()
        .map(|fixture| fixture.gold.to_string())
        .collect();

    // A model that answers gold on every fixture passes the gate.
    let perfect = ScriptedBackend::new(test_model(), gold.iter().cloned().map(Ok).collect());
    let report = run_micro_extract_profile(&perfect, &profile(), &fixtures, None);
    let aggregate = report.aggregate.clone().expect("suite ran");
    assert_eq!(aggregate.samples as usize, fixtures.len());
    assert!((aggregate.schema_valid_rate - 1.0).abs() < 1e-9);
    assert!((aggregate.exact_match_rate - 1.0).abs() < 1e-9);
    assert_eq!(
        (aggregate.unsupported_fields, aggregate.authority_outputs),
        (0, 0)
    );
    assert!(report.verdict.pass, "{:?}", report.verdict.reasons);
    assert!(
        report.samples.iter().all(|sample| !sample.reply.is_empty()),
        "every sample records what the model actually said"
    );

    // Replacing exactly one reply with a routing answer fails the gate on
    // three independent counts, none of which averages away.
    let mut scripted = gold.clone();
    scripted[0] = "{\"route\":[\"upstream_strong\"]}".to_owned();
    let leaky = ScriptedBackend::new(test_model(), scripted.into_iter().map(Ok).collect());
    let leaked = run_micro_extract_profile(&leaky, &profile(), &fixtures, None);
    let aggregate = leaked.aggregate.clone().expect("suite ran");
    assert!(aggregate.schema_valid_rate < 1.0);
    assert_eq!(aggregate.unsupported_fields, 1);
    assert_eq!(aggregate.authority_outputs, 2);
    assert!(!leaked.verdict.pass);
    assert!(
        leaked
            .verdict
            .reasons
            .iter()
            .any(|reason| matches!(reason, VerdictReason::AuthorityOutput { count: 2 }))
    );

    // An absent model is skipped without a single call.
    let absent = ScriptedBackend::new(Vec::new(), gold.into_iter().map(Ok).collect());
    let skipped = run_micro_extract_profile(&absent, &profile(), &fixtures, None);
    assert_eq!(skipped.status, ProfileStatus::ModelAbsent);
    assert_eq!(absent.remaining(), fixtures.len());
    assert!(!skipped.verdict.pass);
}
