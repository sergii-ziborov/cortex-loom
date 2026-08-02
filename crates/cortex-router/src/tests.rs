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
    let decision = route(&RoutingRequest::new("Summarize the supplied evidence IDs"));
    assert_eq!(decision.target, ExecutionTarget::Ollama);
    assert!(decision.approves_local_model());
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
fn words_containing_auth_are_not_authentication() {
    assert_eq!(
        classify("Summarize the author notes").class,
        TaskClass::AdvisoryDraft
    );
}
