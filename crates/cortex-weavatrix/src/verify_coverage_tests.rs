use super::{
    TaskIntent, coverage_requirements, is_sibling_surface_label, sibling_surface_requirements,
};

fn labels(task: &str, symbol: Option<&str>, intent: TaskIntent) -> Vec<String> {
    coverage_requirements(task, symbol, intent)
        .into_iter()
        .map(|requirement| requirement.label)
        .collect()
}

fn sibling_labels(task: &str, intent: TaskIntent) -> Vec<String> {
    sibling_surface_requirements(&task.to_ascii_lowercase(), intent)
        .into_iter()
        .map(|requirement| requirement.label)
        .collect()
}

#[test]
fn bounded_retry_implies_the_limit_constant_and_overflow_error() {
    let found = sibling_labels(
        "Change bounded retry so a target that reached its `maxAttempts` \
resolves the run normally instead of leaving it retryable.",
        TaskIntent::IdentifierChange,
    );
    assert!(found.contains(&"retry_limit_constant".to_owned()));
    assert!(found.contains(&"retry_limit_error".to_owned()));
}

#[test]
fn fail_closed_priority_implies_rank_estimator_and_budget_error() {
    let found = sibling_labels(
        "Add a new band to `EvidencePriority` between High and Normal in the \
deterministic context compiler, keeping critical evidence fail-closed.",
        TaskIntent::IdentifierChange,
    );
    assert!(found.contains(&"fail_closed_error".to_owned()));
    assert!(found.contains(&"priority_rank".to_owned()));
    assert!(found.contains(&"token_estimator".to_owned()));
}

#[test]
fn frontmatter_implies_decoder_heading_and_depends() {
    let found = sibling_labels(
        "Support a list-valued frontmatter key in `import_skill_markdown` \
and export, without breaking the export fixpoint.",
        TaskIntent::IdentifierChange,
    );
    assert!(found.contains(&"scalar_decoder".to_owned()));
    assert!(found.contains(&"title_heading".to_owned()));
    assert!(found.contains(&"step_depends".to_owned()));
}

#[test]
fn mcp_usage_tool_implies_registry_and_compile_tool() {
    let found = sibling_labels(
        "Expose the token-accounting `quality_summary` as a bounded MCP \
tool alongside the existing `usage_read` and `usage_report` tools.",
        TaskIntent::IdentifierChange,
    );
    assert!(found.contains(&"tool_registry".to_owned()));
    assert!(found.contains(&"compile_tool".to_owned()));
}

#[test]
fn streamable_mcp_implies_the_session_header() {
    let found = sibling_labels(
        "Which services read the Streamable HTTP MCP transport at `/mcp`?",
        TaskIntent::ApiContract,
    );
    assert!(found.contains(&"session_header".to_owned()));
}

#[test]
fn compile_context_blast_implies_server_and_http_readers() {
    let found = sibling_labels(
        "Who depends on `compile_context` and what breaks if its signature changes?",
        TaskIntent::BlastRadius,
    );
    assert!(found.contains(&"mcp_server_builder".to_owned()));
    assert!(found.contains(&"http_transport".to_owned()));
}

#[test]
fn probe_prompts_do_not_pick_up_core_sibling_terms() {
    let probes = [
        (
            "Who depends on `route` if its signature changes?",
            TaskIntent::BlastRadius,
        ),
        (
            "Who calls `compile_evidence_bundle` versus `compile_probe_bundle`, \
and what breaks if the generic path starts refusing more packets?",
            TaskIntent::BlastRadius,
        ),
        (
            "What breaks if the `GET /api/usage/quality` HTTP contract changes?",
            TaskIntent::ApiContract,
        ),
        (
            "How does `CORTEX_LLM` wire the gated classifier into `route_work`?",
            TaskIntent::RuntimeConfig,
        ),
    ];
    for (task, intent) in probes {
        let found = sibling_labels(task, intent);
        assert!(
            found.is_empty(),
            "probe prompt picked up sibling terms {found:?}: {task}"
        );
        assert!(
            !labels(task, None, intent)
                .iter()
                .any(|label| is_sibling_surface_label(label)),
            "sibling label leaked into full coverage for {task}"
        );
    }
}
