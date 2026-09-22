use std::path::Path;

use crate::plan::PlanPolicy;
use crate::{PlanHints, WeavatrixAdapter, WeavatrixConfig};

#[test]
fn semantic_retry_source_windows_cover_the_missing_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let adapter = WeavatrixAdapter::new(WeavatrixConfig::discover().expect("config"));
    let cases: &[(&str, Option<&str>, &[&str])] = &[
        (
            "How does `ProfileRegistry` refuse an uncalibrated classification profile?",
            Some("ProfileRegistry"),
            &["gate_passed", "NotCalibrated", "fn select"],
        ),
        (
            "How does `CORTEX_LLM` wire the gated classifier into `route_work`?",
            None,
            &["LlmRouter", "merge_tiers"],
        ),
        (
            "Which module owns `GraphStore` and the run persistence surface?",
            Some("GraphStore"),
            &["run_store", "fn open"],
        ),
        (
            "Who depends on `route` if its signature changes?",
            Some("route"),
            &["route_metric_tools::register", ", route)"],
        ),
    ];
    for (task, symbol, required) in cases {
        let (bundle, report) = adapter
            .prepare_verified_targeted_context(
                &root,
                task,
                *symbol,
                4_000,
                PlanPolicy::default(),
                PlanHints::default(),
            )
            .expect("verified context");
        let source = bundle
            .evidence
            .iter()
            .filter(|item| item.kind == crate::EvidenceKind::SourceReads)
            .map(|item| item.content.as_str())
            .collect::<String>();
        assert!(
            required.iter().all(|term| source.contains(term)),
            "missing contract terms for {task}: required={required:?}; report={report:?}; warnings={:?}; source={source}",
            bundle.warnings,
        );
        let compiled = crate::compile_evidence_bundle(bundle.clone(), task, 5_000, None)
            .expect("contract source compiles");
        let selected_evidence = compiled
            .context
            .content
            .split("<evidence ")
            .filter(|section| !section.contains("id=\"TASK\""))
            .collect::<Vec<_>>()
            .join("<evidence ");
        assert!(
            required.iter().all(|term| selected_evidence.contains(term)),
            "compiler dropped contract terms for {task}: required={required:?}; included={:?}; omitted={:?}",
            compiled.context.included_ids,
            compiled.context.omitted_ids,
        );
    }
}

#[test]
fn relative_repository_keeps_shadow_env_evidence_after_compilation() {
    let root = Path::new("../..");
    if !root.join("crates/cortex-shadow/src/lib.rs").exists() {
        return;
    }
    let task = "How is `ShadowHandle` spawned, and which env flag turns shadow mode on?";
    let adapter = WeavatrixAdapter::new(WeavatrixConfig::discover().expect("config"));
    let (bundle, report) = adapter
        .prepare_verified_targeted_context(
            root,
            task,
            Some("ShadowHandle"),
            4_000,
            PlanPolicy::default(),
            PlanHints::default(),
        )
        .expect("verified context");
    let compiled =
        crate::compile_evidence_bundle(bundle, task, 4_000, None).expect("shadow context compiles");
    let evidence_only = compiled
        .context
        .content
        .split("\n## ")
        .filter(|section| {
            !section
                .trim_start()
                .trim_start_matches("## ")
                .starts_with("[TASK]")
        })
        .collect::<Vec<_>>()
        .join("\n## ");
    let env_flag = ["CORTEX_", "SHADOW"].concat();
    assert!(
        evidence_only.contains(&env_flag),
        "relative root dropped env evidence: report={report:?}; included={:?}; omitted={:?}; evidence={evidence_only}",
        compiled.context.included_ids,
        compiled.context.omitted_ids,
    );
}
