use super::*;

#[test]
fn high_risk_phrases_never_route_locally() {
    let phrases = [
        "audit the authentication flow",
        "fix a security vulnerability",
        "resolve a concurrency race condition",
        "prepare a database migration",
        "create the release tag",
        "deploy to production",
        "publish this crate",
        "rotate an OAuth access token",
        "repair tenant isolation",
        "change the Kubernetes deployment",
    ];
    for task in phrases {
        let decision = route(&RoutingRequest::new(task));
        assert_eq!(decision.target, ExecutionTarget::Upstream, "{task}");
        assert!(!decision.advisory_only, "{task}");
    }
}

#[test]
fn advisory_summary_can_use_ollama() {
    let mut request = RoutingRequest::new("Summarize the supplied evidence IDs");
    request.evidence = EvidenceStatus::Verified;
    let decision = route(&request);
    assert_eq!(decision.target, ExecutionTarget::Ollama);
    assert!(decision.approves_local_model());
    assert_eq!(decision.model_tier, ModelTier::LocalMedium);
    assert_eq!(
        decision.context.strategy,
        ContextStrategy::CitationCompression
    );
}

#[test]
fn deterministic_and_graph_tasks_use_non_model_tools() {
    assert_eq!(
        route(&RoutingRequest::new("Validate JSON using the schema")).target,
        ExecutionTarget::Deterministic
    );
    assert_eq!(
        route(&RoutingRequest::new(
            "Build the repository dependency graph"
        ))
        .target,
        ExecutionTarget::Weavatrix
    );
}

#[test]
fn structured_extraction_uses_the_small_local_tier() {
    let decision = route(&RoutingRequest::new(
        "Extract fields from the supplied text",
    ));
    assert_eq!(decision.target, ExecutionTarget::Ollama);
    assert_eq!(decision.model_tier, ModelTier::LocalSmall);
}

#[test]
fn evidence_compression_requires_verified_inputs() {
    let decision = route(&RoutingRequest::new(
        "Compress context for the coding agent",
    ));
    assert_eq!(decision.target, ExecutionTarget::Upstream);
    assert!(decision.reasons.contains(&RoutingReason::MissingEvidence));
}

#[test]
fn evidence_schema_budget_and_mutation_guards_fail_closed() {
    let mut cases = Vec::new();
    let mut missing = RoutingRequest::new("Summarize the evidence");
    missing.evidence = EvidenceStatus::Missing;
    cases.push(missing);
    let mut contradictory = RoutingRequest::new("Summarize the evidence");
    contradictory.evidence = EvidenceStatus::Contradictory;
    cases.push(contradictory);
    let mut invalid = RoutingRequest::new("Summarize the evidence");
    invalid.schema_valid = false;
    cases.push(invalid);
    let mut over_budget = RoutingRequest::new("Summarize the evidence");
    over_budget.budget.estimated_output_tokens = 2_000;
    cases.push(over_budget);
    let mut mutation = RoutingRequest::new("Summarize the evidence");
    mutation.mutation = MutationStatus::ApprovalRequired;
    cases.push(mutation);

    for request in cases {
        assert_eq!(route(&request).target, ExecutionTarget::Upstream);
    }
}

#[test]
fn approved_mutations_still_bypass_local_models() {
    let mut request = RoutingRequest::new("Implement a small wording change");
    request.mutation = MutationStatus::Approved;
    let decision = route(&request);
    assert_eq!(decision.target, ExecutionTarget::Upstream);
}

#[test]
fn an_upstream_plan_never_exceeds_the_caller_budget() {
    // A caller with a small ceiling must not receive a larger plan.
    let mut tight = RoutingRequest::new("Deploy to production");
    tight.budget.max_input_tokens = 2_000;
    let decision = route(&tight);
    assert_eq!(decision.target, ExecutionTarget::Upstream);
    assert_eq!(decision.context.max_input_tokens, 2_000);

    // A generous caller is still capped at the upstream evidence ceiling.
    let mut generous = RoutingRequest::new("Deploy to production");
    generous.budget.max_input_tokens = 100_000;
    assert_eq!(
        route(&generous).context.max_input_tokens,
        UPSTREAM_EVIDENCE_TOKENS
    );

    // The same contract holds on the guard path (fail-closed escalation).
    let mut guarded = RoutingRequest::new("Summarize the evidence");
    guarded.evidence = EvidenceStatus::Missing;
    guarded.budget.max_input_tokens = 512;
    let decision = route(&guarded);
    assert_eq!(decision.target, ExecutionTarget::Upstream);
    assert_eq!(decision.context.max_input_tokens, 512);
}

#[test]
fn words_containing_auth_are_not_authentication() {
    assert_ne!(
        classify("Summarize the author notes").class,
        TaskClass::Authentication
    );
}
