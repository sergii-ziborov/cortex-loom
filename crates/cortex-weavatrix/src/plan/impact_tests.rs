use super::{PlanPolicy, coding_declaration_query, coding_focus_query, plan_with_hints};

#[test]
fn caller_and_transport_impact_plans_both_surfaces() {
    let task = "How do changes to `compile_evidence_bundle` affect its callers and the MCP/HTTP entry points used by `cortex_prepare`?";
    for hints in [
        crate::PlanHints::default(),
        crate::PlanHints {
            intent: Some(crate::IntentHint::RuntimeConfig),
            ..crate::PlanHints::default()
        },
    ] {
        let operations = plan_with_hints(
            task,
            Some("compile_evidence_bundle"),
            8_000,
            PlanPolicy::default(),
            hints,
        );
        let tools: Vec<_> = operations.iter().map(|operation| operation.tool).collect();
        assert!(tools.contains(&"get_dependents"), "{tools:?}");
        assert!(tools.contains(&"find_references"), "{tools:?}");
        assert!(tools.contains(&"list_endpoints"), "{tools:?}");
    }
}

#[test]
fn new_feature_searches_its_domain_instead_of_dirty_tree_tests() {
    let task = "Add an Elevated priority to the evidence compiler; update the schema and run relevant tests.";
    assert_eq!(coding_focus_query(task), r"(?i)\b(?:elevated|priority)\b");
    assert!(coding_declaration_query(task).contains("(?:elevated|priority)"));
    let operations = plan_with_hints(
        task,
        None,
        4_000,
        PlanPolicy::default(),
        crate::PlanHints::default(),
    );
    let search = operations
        .iter()
        .find(|operation| operation.id == "WX-FOCUS")
        .unwrap();
    assert_eq!(search.tool, "search_code");
    assert_eq!(search.arguments["query"], r"(?i)\b(?:elevated|priority)\b");
    assert!(
        operations
            .iter()
            .any(|operation| operation.id == "WX-FOCUS-DECL")
    );
    assert!(
        !operations
            .iter()
            .any(|operation| operation.tool == "select_tests")
    );
}

#[test]
fn naming_style_in_prose_is_not_a_missing_symbol() {
    let task = "Add a priority with snake_case serialization and MCP context_compile schema";
    assert_eq!(super::extract_identifiers(task), ["context_compile"]);
    assert_eq!(
        super::extract_identifiers("Change `snake_case` itself"),
        ["snake_case"]
    );
}

#[test]
fn creation_with_an_mcp_tool_label_still_searches_the_feature_domain() {
    let task = "Add Elevated priority with snake_case serialization, MCP context_compile schema, and UI help";
    let operations = plan_with_hints(
        task,
        None,
        4_000,
        PlanPolicy::default(),
        crate::PlanHints::default(),
    );
    assert!(
        operations
            .iter()
            .any(|operation| operation.id == "WX-FOCUS")
    );
    assert!(
        operations
            .iter()
            .any(|operation| operation.id == "WX-FOCUS-DECL")
    );
    assert!(
        operations
            .iter()
            .any(|operation| operation.id == "WX-FOCUS-UI")
    );
    assert!(
        operations
            .iter()
            .any(|operation| operation.id == "WX-FOCUS-MCP")
    );
    for operation in operations
        .iter()
        .filter(|operation| operation.tool == "search_code")
    {
        assert_eq!(
            operation.expected_tokens,
            u32::try_from(operation.arguments["token_budget"].as_u64().unwrap()).unwrap()
        );
    }
    assert_eq!(
        crate::source_followup::SourceWindow::for_task(task).max_files,
        4
    );
}
