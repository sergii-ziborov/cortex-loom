use super::{
    BUDGET_HONOURING, EvidenceKind, PlanPolicy, extract_identifiers, plan, plan_with_hints,
    search_pattern,
};

#[test]
fn identifiers_are_recognised_by_shape_not_by_vocabulary() {
    let found = extract_identifiers(
        "Change bounded retry so MAX_RETRY_ATTEMPTS and maxAttempts agree, \
         see crates/cortex-run/src/retry.rs and RunError::RetryLimitTooLarge.",
    );
    assert!(found.contains(&"MAX_RETRY_ATTEMPTS".to_owned()));
    assert!(found.contains(&"maxAttempts".to_owned()));
    assert!(found.iter().any(|value| value.ends_with("retry.rs")));
    assert!(
        found
            .iter()
            .any(|value| value.contains("RetryLimitTooLarge"))
    );
    for word in ["Change", "bounded", "retry", "and", "agree"] {
        assert!(
            !found.contains(&word.to_owned()),
            "{word} was taken as code"
        );
    }
}

#[test]
fn an_explicit_lowercase_backtick_is_a_searchable_identifier() {
    assert_eq!(
        extract_identifiers("Who depends on `route` if its signature changes?"),
        vec!["route"]
    );
}

#[test]
fn url_paths_and_backticks_are_identifiers() {
    let found =
        extract_identifiers("What breaks if `POST` `/api/skills/compile` changes, see `/mcp`?");
    assert!(found.contains(&"/api/skills/compile".to_owned()));
    assert!(found.contains(&"/mcp".to_owned()));
    assert!(
        !found
            .iter()
            .any(|value| value == "HTTP" || value == "POST" || value == "API"),
        "prose acronyms must not enter the search alternation, got {found:?}"
    );
    let templated = extract_identifiers("Inspect `GET /api/adapters/{agent}`");
    assert!(templated.contains(&"/api/adapters/{agent}".to_owned()));
    assert!(!templated.contains(&"GET".to_owned()));
    let task = "`alpha_one` `beta_two` `gamma_three` `delta_four` \
                `epsilon_five` `zeta_six` `eta_seven` `theta_eight` `iota_nine`";
    let found = extract_identifiers(task);
    assert_eq!(found.len(), super::MAX_IDENTIFIERS);
    assert_eq!(found[0], "alpha_one");
    assert!(!found.contains(&"iota_nine".to_owned()));
}

#[test]
fn a_task_naming_code_asks_for_the_facts_a_summary_cannot_carry() {
    let operations = plan("rename `RetryLimitTooLarge`", Some("apply_command"), 16_000);
    let tools: Vec<&str> = operations.iter().map(|operation| operation.tool).collect();
    assert_eq!(
        tools,
        [
            "search_code",
            "context_bundle",
            "module_map",
            "get_dependents"
        ]
    );
    let recommended = plan("rename `RetryLimitTooLarge`", Some("apply_command"), 4_000);
    let tools: Vec<&str> = recommended.iter().map(|operation| operation.tool).collect();
    assert_eq!(
        tools,
        ["search_code", "context_bundle", "module_map"],
        "symbol evidence really costs about 4 800 even when a budget is \
         requested, so at 4 000 there is no room for dependents or a plan"
    );
    for operation in &operations {
        assert_eq!(
            operation.arguments.get("token_budget").is_some(),
            operation.bounded,
            "{} sends a budget it does not honour, or honours one it was not sent",
            operation.tool
        );
        assert_eq!(
            operation.bounded,
            BUDGET_HONOURING.contains(&operation.tool),
            "{} disagrees with the runtime's own list",
            operation.tool
        );
    }
    assert_eq!(operations[0].kind, EvidenceKind::SearchHits);
}

#[test]
fn blast_radius_intent_asks_for_dependents_first() {
    let operations = plan(
        "Who depends on `compile_context` and what breaks if its signature changes?",
        Some("compile_context"),
        4_000,
    );
    let tools: Vec<&str> = operations.iter().map(|operation| operation.tool).collect();
    assert_eq!(
        tools.first().copied(),
        Some("get_dependents"),
        "blast-radius questions must keep dependents under a 4k budget, got {tools:?}"
    );
    assert!(tools.contains(&"get_dependents"));
    assert!(
        !tools.contains(&"context_bundle"),
        "symbol source is secondary on a dependents question"
    );
    assert!(
        !tools.contains(&"list_endpoints"),
        "endpoints are for contract questions, not blast radius"
    );
}

#[test]
fn api_contract_intent_asks_for_endpoints() {
    let operations = plan(
        "What breaks if the `/api/skills/compile` HTTP contract changes?",
        None,
        4_000,
    );
    let tools: Vec<&str> = operations.iter().map(|operation| operation.tool).collect();
    assert_eq!(tools.first().copied(), Some("list_endpoints"));
    assert!(tools.contains(&"search_code"));
}

#[test]
fn module_topology_intent_asks_for_module_map_first() {
    let operations = plan(
        "Which module owns `compile_context`, and where does the crate layout put it?",
        Some("compile_context"),
        4_000,
    );
    let tools: Vec<&str> = operations.iter().map(|operation| operation.tool).collect();
    assert_eq!(
        tools.first().copied(),
        Some("module_map"),
        "topology questions must keep module_map under a 4k budget, got {tools:?}"
    );
    assert!(tools.contains(&"search_code"));
    assert_eq!(
        tools.iter().filter(|tool| **tool == "module_map").count(),
        1,
        "module_map must not be planned twice"
    );
}

#[test]
fn a_task_naming_no_code_falls_back_to_structure() {
    let operations = plan("make the thing faster please", None, 4_000);
    let tools: Vec<&str> = operations.iter().map(|operation| operation.tool).collect();
    assert_eq!(tools, ["module_map"]);
}

#[test]
fn runtime_config_intent_searches_code_and_config_without_a_change_plan() {
    let operations = plan(
        "How does `CORTEX_LLM` read config/llm-profiles.json and enforce its profile gate?",
        None,
        4_000,
    );
    let searches: Vec<_> = operations
        .iter()
        .filter(|operation| operation.tool == "search_code")
        .collect();
    assert_eq!(searches.len(), 2);
    assert_eq!(searches[0].id, "WX-SEARCH");
    assert_eq!(searches[1].id, "WX-CONFIG");
    assert_eq!(searches[1].arguments["glob"], "config/**");
    assert!(
        operations
            .iter()
            .all(|operation| operation.tool != "verified_change")
    );
}

#[test]
fn an_explicit_change_plan_can_still_request_verified_change() {
    let operations = plan(
        "Prepare an implementation plan for changing `compile_context`",
        Some("compile_context"),
        16_000,
    );
    assert!(
        operations
            .iter()
            .any(|operation| operation.tool == "verified_change")
    );
}

#[test]
fn active_skill_hints_override_intent_and_can_forbid_change_plans() {
    let operations = plan_with_hints(
        "Prepare an implementation plan for `CORTEX_SHADOW`.",
        None,
        4_000,
        PlanPolicy::default(),
        crate::PlanHints {
            intent: Some(crate::IntentHint::RuntimeConfig),
            source_followup: Some(true),
            skip_change_plan: true,
        },
    );
    assert!(
        operations
            .iter()
            .any(|operation| operation.id == "WX-CONFIG")
    );
    assert!(
        operations
            .iter()
            .all(|operation| operation.kind != EvidenceKind::ChangePlan)
    );
}

#[test]
fn regex_metacharacters_in_identifiers_are_escaped() {
    let pattern = search_pattern(&["a.b".to_owned(), "c::d".to_owned()]);
    assert_eq!(pattern, "a\\.b|c::d");
    // Slashes must stay literal: Rust's regex crate rejects `\/`.
    assert_eq!(
        search_pattern(&["/api/skills/compile".to_owned()]),
        "/api/skills/compile"
    );
}

#[test]
fn a_small_budget_stops_asking_for_evidence_it_cannot_carry() {
    let generous = plan("rename `RetryLimitTooLarge`", Some("apply_command"), 16_000);
    assert_eq!(generous.len(), 4);
    let tight = plan("rename `RetryLimitTooLarge`", Some("apply_command"), 600);
    assert!(tight.len() < generous.len());
    assert_eq!(
        tight.first().map(|operation| operation.tool),
        Some("search_code")
    );
    assert!(
        !tight
            .iter()
            .any(|operation| operation.tool == "verified_change")
    );
}

#[test]
fn a_tiny_budget_still_produces_usable_operation_budgets() {
    for operation in plan("touch `alpha_one`", None, 1) {
        let budget = operation.arguments["token_budget"].as_u64().unwrap();
        assert!(budget > 0, "{} received a zero budget", operation.tool);
    }
}
