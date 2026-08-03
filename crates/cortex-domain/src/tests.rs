use super::*;

#[test]
fn default_graph_is_valid_and_connected() {
    let graph = default_control_plane();
    graph.validate().expect("default graph must remain valid");
    assert_eq!(graph.reachable_from("request").len(), graph.nodes.len());
}

#[test]
fn rejects_an_edge_to_a_missing_node() {
    let mut graph = default_control_plane();
    graph.edges[0].to = "missing".to_owned();
    assert!(matches!(
        graph.validate(),
        Err(GraphError::MissingEndpoint { node, .. }) if node == "missing"
    ));
}

#[test]
fn local_execution_is_advisory_and_non_mutating() {
    let mut graph = default_control_plane();
    graph.nodes[0].execution = Some(ExecutionPolicy {
        target: ExecutionTarget::Ollama,
        risk: RiskLevel::Low,
        max_input_tokens: 2_048,
        max_output_tokens: 256,
        require_evidence: true,
        require_upstream_review: false,
        allow_mutation: false,
        model_profile: Some("local-small".to_owned()),
    });
    assert!(matches!(
        graph.validate(),
        Err(GraphError::InvalidExecution { .. })
    ));
}

#[test]
fn rejects_empty_node_labels() {
    let mut graph = default_control_plane();
    graph.nodes[0].label.clear();
    assert!(matches!(
        graph.validate(),
        Err(GraphError::EmptyNodeField { field: "label", .. })
    ));
}
