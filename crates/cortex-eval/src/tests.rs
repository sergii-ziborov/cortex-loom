use cortex_ollama::ModelInfo;
use cortex_router::{ModelTier, classify};

use crate::backend::ScriptedBackend;
use crate::comparators::policy_tier;
use crate::fixtures::{
    ClassificationFixture, CompressionFixture, EvidenceFixture, FixtureSet, RetrievalFixtures,
    default_fixtures,
};
use crate::metrics::{
    ClassificationAggregate, CompressionAggregate, ExtractionAggregate, latency_stats, percentile,
};
use crate::report::{EvalReport, render_markdown};
use crate::runner::{EvalProfile, ProfileStatus, SuiteSelection, run_profile};
use crate::verdict::{VerdictReason, judge};
use crate::{PROMPT_VERSION, SCHEMA_VERSION};

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

fn classification_only() -> SuiteSelection {
    SuiteSelection {
        classification: true,
        extraction: false,
        compression: false,
        retrieval: false,
    }
}

fn empty_fixture_set() -> FixtureSet {
    FixtureSet {
        classification: Vec::new(),
        extraction: Vec::new(),
        compression: Vec::new(),
        retrieval: RetrievalFixtures {
            corpus: Vec::new(),
            queries: Vec::new(),
        },
    }
}

fn classification_fixture(id: &str, task: &str, gold_tier: ModelTier) -> ClassificationFixture {
    ClassificationFixture {
        id: id.to_owned(),
        task: task.to_owned(),
        gold_class: classify(task).class,
        gold_tier,
    }
}

#[test]
fn default_fixtures_match_the_deterministic_policy() {
    let fixtures = default_fixtures().expect("embedded fixtures must be valid");
    assert!(fixtures.classification.len() >= 28);
    assert!(fixtures.extraction.len() >= 10);
    assert!(fixtures.compression.len() >= 6);
    for fixture in &fixtures.classification {
        let classification = classify(&fixture.task);
        assert_eq!(classification.class, fixture.gold_class, "{}", fixture.id);
        assert_eq!(
            policy_tier(classification.class),
            fixture.gold_tier,
            "{}",
            fixture.id
        );
    }
}

#[test]
fn absent_models_are_skipped_without_a_single_model_call() {
    let backend = ScriptedBackend::new(Vec::new(), vec![Ok("{\"tier\":\"none\"}".to_owned())]);
    let fixtures = default_fixtures().unwrap();
    let report = run_profile(&backend, &profile(), &fixtures, SuiteSelection::all(), None);
    assert_eq!(report.status, ProfileStatus::ModelAbsent);
    assert_eq!(backend.remaining(), 1, "no scripted response was consumed");
    assert!(!report.verdict.pass);
    assert_eq!(
        report.verdict.reasons.len(),
        2,
        "both local_small role suites reported unrun"
    );
}

#[test]
fn classification_counts_missed_escalations_and_schema_failures() {
    let mut fixtures = empty_fixture_set();
    fixtures.classification = vec![
        classification_fixture(
            "gold-upstream",
            "Deploy the server to the staging cluster",
            ModelTier::UpstreamStrong,
        ),
        classification_fixture(
            "gold-medium",
            "Summarize the verified evidence for the review gate",
            ModelTier::LocalMedium,
        ),
        classification_fixture(
            "gold-none",
            "Sort the dependency list and count duplicate entries",
            ModelTier::None,
        ),
    ];
    let backend = ScriptedBackend::new(
        test_model(),
        vec![
            Ok("{\"tier\":\"local_small\"}".to_owned()),
            Ok("{\"tier\":\"local_medium\"}".to_owned()),
            Ok("not json".to_owned()),
        ],
    );
    let report = run_profile(&backend, &profile(), &fixtures, classification_only(), None);
    let aggregate = report.classification.expect("classification suite ran");
    assert_eq!(aggregate.samples, 3);
    assert_eq!(aggregate.schema_valid, 2);
    assert_eq!(aggregate.agreements, 1);
    assert_eq!(aggregate.missed_escalations, 1);
    assert!(!report.verdict.pass);
    assert!(
        report
            .verdict
            .reasons
            .iter()
            .any(|reason| matches!(reason, VerdictReason::MissedEscalations { count: 1 }))
    );
}

#[test]
fn compression_flags_hallucinated_citations() {
    let mut fixtures = empty_fixture_set();
    fixtures.compression = vec![CompressionFixture {
        id: "cp-test".to_owned(),
        task: "Summarize the evidence".to_owned(),
        evidence: vec![
            EvidenceFixture {
                id: "WX-A".to_owned(),
                source: "weavatrix:a".to_owned(),
                content: "alpha evidence ".repeat(40),
            },
            EvidenceFixture {
                id: "WX-B".to_owned(),
                source: "weavatrix:b".to_owned(),
                content: "beta evidence ".repeat(40),
            },
        ],
        must_cite: vec!["WX-A".to_owned(), "WX-B".to_owned()],
    }];
    let backend = ScriptedBackend::new(
        test_model(),
        vec![Ok(
            "{\"summary\":\"Grounded in [WX-A] and [WX-FAKE].\",\"evidenceIds\":[\"WX-B\"]}"
                .to_owned(),
        )],
    );
    let selection = SuiteSelection {
        classification: false,
        extraction: false,
        compression: true,
        retrieval: false,
    };
    let medium_profile = EvalProfile {
        id: "candidate-medium".to_owned(),
        tier: ModelTier::LocalMedium,
        model: "qwen-test:4b".to_owned(),
    };
    let report = run_profile(&backend, &medium_profile, &fixtures, selection, None);
    let aggregate = report.compression.expect("compression suite ran");
    assert_eq!(aggregate.hallucinated_total, 1);
    assert_eq!(aggregate.missing_total, 0);
    assert!((aggregate.min_preserved_ratio - 1.0).abs() < 1e-9);
    assert!(aggregate.mean_token_delta < 0, "the draft compresses");
    assert!(
        report
            .verdict
            .reasons
            .iter()
            .any(|reason| matches!(reason, VerdictReason::HallucinatedCitations { count: 1 }))
    );
}

#[test]
fn limits_bound_the_sample_count() {
    let mut fixtures = empty_fixture_set();
    fixtures.classification = vec![
        classification_fixture("one", "Sort the values", ModelTier::None),
        classification_fixture("two", "Sort the keys", ModelTier::None),
        classification_fixture("three", "Sort the names", ModelTier::None),
    ];
    let backend = ScriptedBackend::new(test_model(), vec![Ok("{\"tier\":\"none\"}".to_owned())]);
    let report = run_profile(
        &backend,
        &profile(),
        &fixtures,
        classification_only(),
        Some(1),
    );
    assert_eq!(report.classification_samples.len(), 1);
    assert_eq!(backend.remaining(), 0);
}

#[test]
fn percentiles_use_nearest_rank() {
    assert_eq!(percentile(&[], 50), 0);
    assert_eq!(percentile(&[4, 1, 3, 2], 50), 2);
    assert_eq!(percentile(&[4, 1, 3, 2], 95), 4);
    let stats = latency_stats(&[10, 20, 30]);
    assert_eq!(stats.samples, 3);
    assert_eq!(stats.p50_ms, 20);
    assert_eq!(stats.max_ms, 30);
}

fn passing_aggregates() -> (
    ClassificationAggregate,
    ExtractionAggregate,
    CompressionAggregate,
) {
    (
        ClassificationAggregate {
            samples: 28,
            schema_valid: 28,
            schema_valid_rate: 1.0,
            agreements: 25,
            accuracy: 0.89,
            under_called: 1,
            missed_escalations: 0,
        },
        ExtractionAggregate {
            samples: 10,
            schema_valid: 10,
            schema_valid_rate: 1.0,
            action_matches: 9,
            action_accuracy: 0.9,
            exact_matches: 7,
            exact_match_rate: 0.7,
        },
        CompressionAggregate {
            samples: 6,
            schema_valid: 6,
            schema_valid_rate: 1.0,
            mean_preserved_ratio: 0.97,
            min_preserved_ratio: 0.92,
            hallucinated_total: 0,
            missing_total: 1,
            compressed_count: 6,
            mean_token_delta: -180,
        },
    )
}

#[test]
fn verdicts_gate_only_the_suites_the_role_grants() {
    let (classification, extraction, compression) = passing_aggregates();

    // A small profile passes on its role suites even with terrible
    // compression, because routing never assigns compression to it.
    let mut bad_compression = compression.clone();
    bad_compression.hallucinated_total = 18;
    bad_compression.min_preserved_ratio = 0.0;
    assert!(
        judge(
            ModelTier::LocalSmall,
            Some(&classification),
            Some(&extraction),
            Some(&bad_compression),
        )
        .pass
    );

    // A medium profile passes on perfect compression despite weak
    // classification, and fails once its compression hallucinates.
    let mut weak_classification = classification.clone();
    weak_classification.accuracy = 0.7;
    weak_classification.missed_escalations = 1;
    assert!(
        judge(
            ModelTier::LocalMedium,
            Some(&weak_classification),
            Some(&extraction),
            Some(&compression),
        )
        .pass
    );
    assert!(!judge(ModelTier::LocalMedium, None, None, Some(&bad_compression)).pass);

    // Missed escalations always fail the small role.
    let mut missed = classification.clone();
    missed.missed_escalations = 1;
    let verdict = judge(
        ModelTier::LocalSmall,
        Some(&missed),
        Some(&extraction),
        Some(&compression),
    );
    assert!(!verdict.pass);
    assert!(
        verdict
            .reasons
            .iter()
            .any(|reason| matches!(reason, VerdictReason::MissedEscalations { count: 1 }))
    );

    // A non-local tier is gated on the full matrix.
    let mut inflating = compression.clone();
    inflating.mean_token_delta = 40;
    let full = judge(
        ModelTier::UpstreamStrong,
        Some(&classification),
        Some(&extraction),
        Some(&inflating),
    );
    assert!(full.reasons.iter().any(|reason| matches!(
        reason,
        VerdictReason::DraftDoesNotCompress {
            mean_token_delta: 40
        }
    )));

    // A gated suite that did not run fails explicitly.
    assert!(
        !judge(
            ModelTier::LocalSmall,
            None,
            Some(&extraction),
            Some(&compression)
        )
        .pass
    );
}

#[test]
fn markdown_report_states_the_verdict() {
    let backend = ScriptedBackend::new(Vec::new(), Vec::new());
    let fixtures = default_fixtures().unwrap();
    let skipped = run_profile(&backend, &profile(), &fixtures, SuiteSelection::all(), None);
    let report = EvalReport {
        generated_at_unix: 1,
        ollama_version: Some("test".to_owned()),
        prompt_version: PROMPT_VERSION.to_owned(),
        schema_version: SCHEMA_VERSION.to_owned(),
        profiles: vec![skipped],
        embeddings: Vec::new(),
    };
    let markdown = render_markdown(&report);
    assert!(markdown.contains("model_absent"));
    assert!(markdown.contains(PROMPT_VERSION));
}

#[test]
fn retrieval_metrics_and_gate_work_on_scripted_embeddings() {
    use crate::fixtures::{CorpusDoc, RetrievalQuery};
    use crate::metrics::{cosine_similarity, ndcg_at_k, rank_by_similarity, recall_at_k};
    use crate::runner::{EmbeddingProfile, run_embedding_profile};
    use crate::verdict::judge_retrieval;

    assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-9);
    assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-9);
    let ranking = rank_by_similarity(&[1.0, 0.0], &[vec![0.0, 1.0], vec![1.0, 0.1]]);
    assert_eq!(ranking, [1, 0]);
    let ranked = ["a", "b", "c"];
    let relevant = vec!["b".to_owned()];
    assert!((recall_at_k(&ranked, &relevant, 1) - 0.0).abs() < 1e-9);
    assert!((recall_at_k(&ranked, &relevant, 2) - 1.0).abs() < 1e-9);
    assert!(
        ndcg_at_k(&ranked, &relevant, 5) < 1.0,
        "rank 2 is discounted"
    );

    let fixtures = RetrievalFixtures {
        corpus: vec![
            CorpusDoc {
                id: "doc-a".to_owned(),
                text: "alpha".to_owned(),
            },
            CorpusDoc {
                id: "doc-b".to_owned(),
                text: "beta".to_owned(),
            },
        ],
        queries: vec![RetrievalQuery {
            id: "q-1".to_owned(),
            text: "find alpha".to_owned(),
            relevant: vec!["doc-a".to_owned()],
        }],
    };
    let backend = ScriptedBackend::new(
        vec![ModelInfo {
            name: "embed-test".to_owned(),
            model: "embed-test:latest".to_owned(),
            size: 1,
            digest: "sha256:embed".to_owned(),
        }],
        Vec::new(),
    );
    // Corpus batch, then query batch: the query vector matches doc-a.
    backend.queue_embeddings(Ok(vec![vec![1.0, 0.0], vec![0.0, 1.0]]));
    backend.queue_embeddings(Ok(vec![vec![0.9, 0.1]]));
    let profile = EmbeddingProfile {
        id: "embed-test".to_owned(),
        model: "embed-test:latest".to_owned(),
    };
    let report = run_embedding_profile(&backend, &profile, &fixtures, None);
    assert_eq!(report.status, ProfileStatus::Evaluated);
    assert_eq!(report.dimensions, Some(2));
    let aggregate = report.retrieval.expect("retrieval ran");
    assert!((aggregate.mean_recall_at_5 - 1.0).abs() < 1e-9);
    assert!((aggregate.mean_reciprocal_rank - 1.0).abs() < 1e-9);
    assert!(report.verdict.pass, "perfect retrieval passes the gate");

    // An absent model is skipped without any embed call.
    let absent_backend = ScriptedBackend::new(Vec::new(), Vec::new());
    let absent = run_embedding_profile(&absent_backend, &profile, &fixtures, None);
    assert_eq!(absent.status, ProfileStatus::ModelAbsent);

    assert!(!judge_retrieval(None).pass, "unrun suite fails explicitly");
}
