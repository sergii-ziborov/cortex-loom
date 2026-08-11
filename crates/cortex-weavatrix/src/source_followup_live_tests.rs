use std::path::Path;

use crate::plan::PlanPolicy;
use crate::{PlanHints, WeavatrixAdapter, WeavatrixConfig};

/// Live probe: the skills-compile contract lives on the Rust server route.
#[test]
fn source_followup_opens_the_rust_server_for_skills_compile() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if !root.join("apps/cortex-server/src/main.rs").exists() {
        return;
    }
    let task = "What breaks if the `/api/skills/compile` HTTP contract changes?";
    let planned = crate::plan::plan(task, None, 4_000);
    let search = planned
        .iter()
        .find(|operation| operation.tool == "search_code")
        .expect("contract plan searches");
    let query = search.arguments["query"].as_str().unwrap_or("");
    assert!(
        query == "/api/skills/compile" || query.starts_with("/api/skills/compile|"),
        "search query was {query}"
    );
    assert!(
        !query.split('|').any(|part| part == "HTTP"),
        "HTTP acronym leaked into search: {query}"
    );
    let adapter = WeavatrixAdapter::new(WeavatrixConfig::discover().expect("config"));
    let bundle = adapter
        .prepare_targeted_context_with_source_reads(&root, task, None, 4_000, PlanPolicy::default())
        .expect("source follow-up bundle");
    assert!(
        bundle
            .evidence
            .iter()
            .all(|fragment| fragment.kind != crate::EvidenceKind::ChangePlan),
        "gathering evidence must not add an unverified change plan"
    );
    let haystack: String = bundle
        .evidence
        .iter()
        .map(|fragment| fragment.content.as_str())
        .collect();
    assert!(
        haystack.contains("compile_skill") || haystack.contains("/api/skills/compile"),
        "expected Rust server contract evidence; query={query}; warnings={:?}; search_head={}",
        bundle.warnings,
        bundle
            .evidence
            .iter()
            .find(|fragment| fragment.id.starts_with("WX-SEARCH"))
            .map(|fragment| fragment.content.chars().take(400).collect::<String>())
            .unwrap_or_default(),
    );
    let compiled =
        crate::compile_evidence_bundle(bundle, task, 4_000, None).expect("source bundle compiles");
    assert!(
        compiled
            .context
            .included_ids
            .iter()
            .any(|id| id.starts_with("WX-SOURCE")),
        "source evidence must survive the compiler: {:?}",
        compiled.context.omitted_ids
    );
    assert!(!compiled.context.requires_upstream);
}

#[test]
fn thin_rust_only_search_gets_one_wide_retry_and_source_followup() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if !root.join("ui/src/api/client.ts").exists() {
        return;
    }
    let identifier = format!("compile{}", "Markdown");
    let task = format!("Where does the `{identifier}` client call live?");
    let adapter = WeavatrixAdapter::new(WeavatrixConfig::discover().expect("config"));
    let (bundle, report) = adapter
        .prepare_verified_targeted_context(
            &root,
            &task,
            None,
            4_000,
            PlanPolicy::default(),
            PlanHints::default(),
        )
        .expect("verified context");
    assert!(
        report.retry_performed,
        "the Rust-only search should be thin"
    );
    assert!(report.sufficient, "retry remained thin: {report:?}");
    assert!(
        bundle
            .evidence
            .iter()
            .any(|item| item.id.starts_with("WX-RETRY-SEARCH"))
    );
    assert!(
        bundle
            .evidence
            .iter()
            .any(|item| item.id.starts_with("WX-RETRY-SOURCE"))
    );
    let compiled = crate::compile_evidence_bundle(bundle.clone(), &task, 4_000, None)
        .expect("retry bundle compiles");
    let naive_tokens = u32::try_from(
        std::fs::read_to_string(root.join("ui/src/api/client.ts"))
            .expect("UI client")
            .chars()
            .count()
            .div_ceil(4),
    )
    .unwrap_or(u32::MAX);
    assert!(
        compiled.context.selected_estimated_tokens < naive_tokens,
        "retry context should stay below the one known whole file: {naive_tokens}"
    );
    let final_report = crate::assess_compiled(
        &bundle,
        &compiled.context.included_ids,
        &task,
        None,
        PlanHints::default(),
        true,
        report.retry_performed,
    );
    assert!(
        final_report.sufficient,
        "compiled retry remained thin: {final_report:?}; included={:?}",
        compiled.context.included_ids
    );
}
