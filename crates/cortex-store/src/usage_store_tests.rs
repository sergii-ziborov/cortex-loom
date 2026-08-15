use super::*;
use crate::GraphStore;

fn route_sample(target: &str) -> UsageSample {
    UsageSample {
        operation: UsageOperation::RouteWork,
        run_id: None,
        target: Some(target.to_owned()),
        model_tier: Some("upstream_strong".to_owned()),
        task_class: Some("implementation".to_owned()),
        budget_tokens: None,
        raw_tokens: None,
        selected_tokens: None,
        omitted_tokens: None,
        requires_upstream: None,
        latency_ms: None,
        token_accounting: None,
    }
}

fn compile_sample(raw: u32, selected: u32, latency: u64) -> UsageSample {
    UsageSample {
        operation: UsageOperation::ContextCompile,
        run_id: None,
        target: None,
        model_tier: None,
        task_class: None,
        budget_tokens: Some(4_000),
        raw_tokens: Some(raw),
        selected_tokens: Some(selected),
        omitted_tokens: Some(raw.saturating_sub(selected)),
        requires_upstream: Some(true),
        latency_ms: Some(latency),
        token_accounting: None,
    }
}

fn attributed(run_id: &str, raw: u32, selected: u32) -> UsageSample {
    UsageSample {
        run_id: Some(run_id.to_owned()),
        ..compile_sample(raw, selected, 5)
    }
}

#[test]
fn usage_ledger_is_append_only_and_summarizes_volume() {
    let store = GraphStore::open_in_memory().unwrap().usage();
    store.insert(&route_sample("upstream")).unwrap();
    store.insert(&route_sample("deterministic")).unwrap();
    store.insert(&compile_sample(7_500, 3_900, 2_000)).unwrap();
    store.insert(&compile_sample(7_500, 1_900, 1_500)).unwrap();

    let summary = store.summary().unwrap();
    assert_eq!(summary.route_calls, 2);
    assert_eq!(summary.routed_away_from_upstream, 1);
    assert_eq!(summary.compile_calls, 2);
    assert_eq!(summary.raw_tokens_total, 15_000);
    assert_eq!(summary.omitted_tokens_total, 3_600 + 5_600);
    assert_eq!(summary.requires_upstream_count, 2);
    assert_eq!(summary.compile_latency_p50_ms, 1_500);

    let rows = store
        .list(Some(UsageOperation::ContextCompile), 10)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].id > rows[1].id, "newest first");
    let rendered = serde_json::to_string(&rows[0]).expect("rows serialize");
    assert!(rendered.contains("\"omittedTokens\""));
}

#[test]
// One sequential scenario: build a clean run, an open run, and a ghost.
#[allow(clippy::too_many_lines)]
fn quality_summary_credits_only_clean_succeeded_runs() {
    use cortex_domain::default_control_plane;
    use cortex_run::{NodeOutcome, RunCommand};

    let graph_store = GraphStore::open_in_memory().unwrap();
    let seeded = graph_store
        .seed_if_missing(&default_control_plane())
        .unwrap();
    let runs = graph_store.runs();

    // A clean succeeded run: walk the default graph to completion.
    let mut clean = runs.create("clean", &seeded).unwrap();
    for node in [
        "request",
        "scan",
        "weavatrix",
        "skill",
        "local",
        "gate",
        "upstream",
        "result",
    ] {
        clean = runs
            .apply(
                "clean",
                &RunCommand::StartNode {
                    expected_revision: clean.revision,
                    node_id: node.to_owned(),
                    executor: None,
                },
            )
            .unwrap();
        // The example graph asks the upstream node to cite evidence, so
        // a clean walk has to supply some.
        let requires_evidence = seeded
            .nodes
            .iter()
            .find(|candidate| candidate.id == node)
            .and_then(|candidate| candidate.execution.as_ref())
            .is_some_and(|policy| policy.require_evidence);
        let evidence_ids = if requires_evidence {
            let id = format!("{node}-evidence");
            clean = runs
                .apply(
                    "clean",
                    &RunCommand::SubmitEvidence {
                        expected_revision: clean.revision,
                        node_id: node.to_owned(),
                        evidence_id: id.clone(),
                        submitted_by: "test".to_owned(),
                        source: "graph".to_owned(),
                        locator: format!("node:{node}"),
                        digest: None,
                        summary: "Bounded evidence for the walk".to_owned(),
                        executor: None,
                    },
                )
                .unwrap();
            vec![id]
        } else {
            Vec::new()
        };
        clean = runs
            .apply(
                "clean",
                &RunCommand::CompleteNode {
                    expected_revision: clean.revision,
                    node_id: node.to_owned(),
                    outcome: NodeOutcome::Succeeded,
                    selected_edge_ids: Vec::new(),
                    evidence_ids,
                    detail: None,
                    executor: None,
                },
            )
            .unwrap();
    }
    assert_eq!(clean.status, cortex_run::RunStatus::Succeeded);

    // An unfinished run stays unproven.
    runs.create("open", &seeded).unwrap();

    let usage = graph_store.usage();
    usage.insert(&attributed("clean", 7_500, 1_500)).unwrap();
    usage.insert(&attributed("clean", 7_500, 1_500)).unwrap();
    usage.insert(&attributed("open", 7_500, 1_500)).unwrap();
    usage.insert(&attributed("ghost", 100, 50)).unwrap();
    usage.insert(&compile_sample(100, 50, 1)).unwrap();

    usage
        .insert_report(&UsageReport {
            run_id: Some("clean".to_owned()),
            agent: "claude-code".to_owned(),
            input_tokens: 20_000,
            output_tokens: 4_000,
            note: Some("dogfood balance".to_owned()),
        })
        .unwrap();
    usage
        .insert_report(&UsageReport {
            run_id: None,
            agent: "claude-code".to_owned(),
            input_tokens: 500,
            output_tokens: 100,
            note: None,
        })
        .unwrap();

    let summary = usage.summary().unwrap();
    assert_eq!(summary.upstream_reports, 2);
    assert_eq!(summary.upstream_input_tokens_total, 20_500);
    assert_eq!(summary.upstream_output_tokens_total, 4_100);
    assert_eq!(usage.list_reports(1).unwrap().len(), 1);

    let quality = usage.quality_summary().unwrap();
    assert_eq!(quality.attributed_runs, 3);
    assert_eq!(quality.clean_runs, 1);
    assert_eq!(quality.clean_run_omitted_tokens, 12_000);
    assert_eq!(quality.quality_equivalent_runs, 0);
    assert_eq!(quality.quality_equivalent_omitted_tokens, 0);
    assert_eq!(quality.unproven_omitted_tokens, 12_000 + 6_000 + 50);
    assert_eq!(quality.unattributed_samples, 1);
    let clean_row = quality
        .runs
        .iter()
        .find(|row| row.run_id == "clean")
        .unwrap();
    assert!(clean_row.clean_run && !clean_row.quality_equivalent);
    assert!(!clean_row.retried && !clean_row.rejected);
    assert_eq!(clean_row.compile_calls, 2);
    assert_eq!(clean_row.upstream_reports, 1);
    assert_eq!(clean_row.upstream_input_tokens, 20_000);
    assert_eq!(clean_row.upstream_output_tokens, 4_000);
    let ghost_row = quality
        .runs
        .iter()
        .find(|row| row.run_id == "ghost")
        .unwrap();
    assert_eq!(ghost_row.status, None, "missing runs are never creditable");
    assert!(!ghost_row.quality_equivalent);
    assert!(!ghost_row.clean_run);
}

#[test]
fn quality_equivalent_requires_a_passing_oracle_and_artifact_hash() {
    use cortex_domain::default_control_plane;
    use cortex_run::RunCommand;

    let graph_store = GraphStore::open_in_memory().unwrap();
    let seeded = graph_store
        .seed_if_missing(&default_control_plane())
        .unwrap();
    let runs = graph_store.runs();
    let mut clean = runs.create("oracle", &seeded).unwrap();
    for node in [
        "request",
        "scan",
        "weavatrix",
        "skill",
        "local",
        "gate",
        "upstream",
        "result",
    ] {
        clean = runs
            .apply(
                "oracle",
                &RunCommand::StartNode {
                    expected_revision: clean.revision,
                    node_id: node.to_owned(),
                    executor: None,
                },
            )
            .unwrap();
        let requires_evidence = seeded
            .nodes
            .iter()
            .find(|candidate| candidate.id == node)
            .and_then(|candidate| candidate.execution.as_ref())
            .is_some_and(|policy| policy.require_evidence);
        let evidence_ids = if requires_evidence {
            let id = format!("{node}-evidence");
            clean = runs
                .apply(
                    "oracle",
                    &RunCommand::SubmitEvidence {
                        expected_revision: clean.revision,
                        node_id: node.to_owned(),
                        evidence_id: id.clone(),
                        submitted_by: "test".to_owned(),
                        source: "graph".to_owned(),
                        locator: format!("node:{node}"),
                        digest: None,
                        summary: "Bounded evidence for the walk".to_owned(),
                        executor: None,
                    },
                )
                .unwrap();
            vec![id]
        } else {
            Vec::new()
        };
        clean = runs
            .apply(
                "oracle",
                &RunCommand::CompleteNode {
                    expected_revision: clean.revision,
                    node_id: node.to_owned(),
                    outcome: cortex_run::NodeOutcome::Succeeded,
                    selected_edge_ids: Vec::new(),
                    evidence_ids,
                    detail: None,
                    executor: None,
                },
            )
            .unwrap();
    }
    clean = runs
        .apply(
            "oracle",
            &RunCommand::AttestOracle {
                expected_revision: clean.revision,
                kind: "hidden_tests".to_owned(),
                passed: true,
                artifact_hash: Some("sha256:abc".to_owned()),
                baseline_hash: None,
                attested_by: "bench".to_owned(),
                reason: "hidden suite passed".to_owned(),
            },
        )
        .unwrap();
    assert!(clean.oracle.is_some_and(|oracle| oracle.passed));

    let usage = graph_store.usage();
    usage.insert(&attributed("oracle", 4_000, 1_000)).unwrap();
    let quality = usage.quality_summary().unwrap();
    assert_eq!(quality.clean_runs, 1);
    assert_eq!(quality.quality_equivalent_runs, 1);
    assert_eq!(quality.quality_equivalent_omitted_tokens, 3_000);
    let row = quality
        .runs
        .iter()
        .find(|row| row.run_id == "oracle")
        .unwrap();
    assert!(row.clean_run && row.quality_equivalent);
    assert_eq!(row.oracle_kind.as_deref(), Some("hidden_tests"));
}

#[test]
fn token_accounting_round_trips() {
    let store = GraphStore::open_in_memory().unwrap().usage();
    let mut sample = compile_sample(100, 40, 3);
    sample.token_accounting = Some(
        r#"{"counterId":"conservative","tokenizerRevision":"v1","budgetOmittedTokens":60,"dedupSavedTokens":4}"#
            .to_owned(),
    );
    store.insert(&sample).unwrap();
    let rows = store.list(Some(UsageOperation::ContextCompile), 1).unwrap();
    let blob = rows[0].sample.token_accounting.as_deref().unwrap();
    assert!(blob.contains("conservative"));
    assert!(blob.contains("budgetOmittedTokens"));
    assert!(blob.contains("dedupSavedTokens"));
}

#[test]
fn listing_is_bounded() {
    let store = GraphStore::open_in_memory().unwrap().usage();
    for _ in 0..5 {
        store.insert(&route_sample("upstream")).unwrap();
    }
    assert_eq!(store.list(None, 3).unwrap().len(), 3);
    assert_eq!(store.list(None, 10_000).unwrap().len(), 5);
}
