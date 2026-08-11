use std::path::Path;

use mcport::ConcurrentToolServer;

use super::*;

#[test]
fn graph_summary_is_bounded_metadata() {
    let summary = graph_summary(&default_control_plane());
    assert_eq!(summary.get("nodes").and_then(Value::as_u64), Some(8));
    assert!(summary.get("nodes").is_some());
}

#[test]
fn registry_exposes_only_the_cortex_sequence_contract() {
    let state = CortexMcpState {
        store: GraphStore::open_in_memory().unwrap(),
        weavatrix: WeavatrixAdapter::new(WeavatrixConfig::discover().unwrap()),
        shadow: None,
        semantic: None,
        llm_router: None,
    };
    let catalog = build_server(state).catalog();
    let names: Vec<_> = catalog
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for expected in [
        "sequence_list",
        "sequence_recommend",
        "sequence_copy",
        "sequence_lint",
        "sequence_step_read",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
    assert!(!names.iter().any(|name| name.contains("superpowers")));
}

fn tool_names(profile: ServerProfile) -> Vec<String> {
    let state = CortexMcpState {
        store: GraphStore::open_in_memory().unwrap(),
        weavatrix: WeavatrixAdapter::new(WeavatrixConfig::discover().unwrap()),
        shadow: None,
        semantic: None,
        llm_router: None,
    };
    build_server_with(state, profile)
        .catalog()
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(ToOwned::to_owned))
        .collect()
}

#[test]
fn the_context_profile_exposes_evidence_compilation_and_nothing_else() {
    let names = tool_names(ServerProfile::Context);

    assert_eq!(names, ["context_compile", "weavatrix_context_compile"]);
}

#[test]
fn the_context_profile_is_a_strict_subset_of_the_full_surface() {
    let full = tool_names(ServerProfile::Full);
    let context = tool_names(ServerProfile::Context);

    // The point of the profile is a smaller standing schema cost, not a
    // different contract: a caller must be able to move between them
    // without a tool changing meaning.
    assert!(context.len() < full.len());
    for name in &context {
        assert!(full.contains(name), "context-only tool missing: {name}");
    }
}

#[test]
fn an_unknown_profile_name_fails_instead_of_serving_everything() {
    assert!(ServerProfile::parse("ctx").is_err());
    assert_eq!(
        ServerProfile::parse(" Context ").unwrap(),
        ServerProfile::Context
    );
    assert_eq!(ServerProfile::parse("full").unwrap(), ServerProfile::Full);
}

#[test]
fn weavatrix_refactor_preview_schema_accepts_only_a_native_plan() {
    let schema = refactor_preview_schema();
    assert_eq!(schema["properties"]["plan"]["type"], "object");
    assert!(schema["properties"].get("operation").is_none());
    assert!(schema["properties"].get("arguments").is_none());
    assert_eq!(
        schema["required"],
        serde_json::json!(["repository", "plan"])
    );
}

#[test]
fn weavatrix_refactor_preview_response_has_no_apply_authority() {
    let root =
        std::env::temp_dir().join(format!("cortex-mcp-native-preview-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    std::fs::create_dir_all(root.join("src")).unwrap();
    let adapter = WeavatrixAdapter::new(WeavatrixConfig::discover().unwrap());
    let plan = serde_json::json!({
        "schemaVersion": "weavatrix.refactor-plan.v1",
        "operation": "create",
        "operations": [{
            "kind": "create",
            "value": {"path": "src/new.rs", "contents": "pub fn new() {}\n"}
        }]
    });

    let response = preview_refactor_response(&adapter, Path::new(&root), &plan).unwrap();

    assert_eq!(response["mode"], "preview");
    assert!(response.get("preview").is_some());
    let rendered = response.to_string().to_ascii_lowercase();
    for forbidden in ["confirmationtoken", "applyavailable", "rollback"] {
        assert!(
            !rendered.contains(forbidden),
            "forbidden field: {forbidden}"
        );
    }
    assert!(!root.join("src/new.rs").exists());
    std::fs::remove_dir_all(&root).unwrap();
}
