use std::collections::HashSet;

use cortex_domain::{EdgeKind, ExecutionPolicy, ExecutionTarget, GraphEdge, NodeKind, RiskLevel};

use super::{DiagnosticCode, active_step_packet, instantiate_template, lint_sequence, templates};

#[test]
fn a_copy_is_editable_and_detached_from_its_template() {
    let graph = instantiate_template("discover-and-plan", "my-plan", "My plan").unwrap();
    assert_eq!(graph.id, "my-plan");
    assert_eq!(graph.name, "My plan");
    assert_eq!(graph.metadata["sequence.templateId"], "discover-and-plan");
    assert_eq!(graph.metadata["sequence.templateVersion"], "1.0.0");
    assert_eq!(graph.metadata["sequence.editable"], "true");
    assert_eq!(graph.revision, 0);
}

#[test]
fn catalog_ids_and_fingerprints_are_unique_and_stable() {
    let catalog = templates();
    assert_eq!(catalog.len(), 7);
    let ids: HashSet<_> = catalog.iter().map(|template| template.id).collect();
    assert_eq!(ids.len(), catalog.len());

    let first = instantiate_template("discover-and-plan", "one", "One").unwrap();
    let second = instantiate_template("discover-and-plan", "two", "Two").unwrap();
    assert_eq!(
        first.metadata["sequence.templateFingerprint"],
        second.metadata["sequence.templateFingerprint"]
    );
    assert_eq!(first.metadata["sequence.templateFingerprint"].len(), 64);
}

#[test]
fn catalog_templates_are_safe_complete_and_round_trip_stably() {
    use cortex_domain::NodeKind;

    for template in templates() {
        assert!(
            template.markdown.lines().count() < 140,
            "{} is too long",
            template.id
        );
        assert!(
            !template
                .markdown
                .to_ascii_lowercase()
                .contains("superpowers")
        );
        let graph = instantiate_template(template.id, "copy", template.title).unwrap();
        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.kind == NodeKind::Terminal),
            "{} has no terminal",
            template.id
        );
        assert!(
            graph
                .nodes
                .iter()
                .any(|node| matches!(node.kind, NodeKind::UpstreamAgent | NodeKind::Handoff)),
            "{} has no upstream/handoff path",
            template.id
        );
        assert!(
            graph.nodes.iter().any(|node| matches!(
                node.kind,
                NodeKind::EvidenceGate
                    | NodeKind::TestGate
                    | NodeKind::ReviewGate
                    | NodeKind::QualityGate
            )),
            "{} has no proof gate",
            template.id
        );
        let exported = cortex_skills::export_skill_markdown(&graph).unwrap();
        let reimported = cortex_skills::import_skill_markdown("roundtrip.md", &exported).unwrap();
        let second = cortex_skills::export_skill_markdown(&reimported).unwrap();
        assert_eq!(exported, second, "{} is not a fixpoint", template.id);
    }
}

#[test]
fn selected_upstream_mechanics_are_fully_covered_without_bootstrap_hook() {
    let expected: HashSet<_> = [
        "brainstorming",
        "dispatching-parallel-agents",
        "executing-plans",
        "finishing-a-development-branch",
        "receiving-code-review",
        "requesting-code-review",
        "subagent-driven-development",
        "systematic-debugging",
        "test-driven-development",
        "using-git-worktrees",
        "verification-before-completion",
        "writing-plans",
        "writing-skills",
    ]
    .into_iter()
    .collect();
    let covered: HashSet<_> = templates()
        .iter()
        .flat_map(|template| template.markdown.lines())
        .filter_map(|line| line.strip_prefix("mechanics: "))
        .flat_map(|value| value.split(',').map(str::trim))
        .collect();

    assert_eq!(covered, expected);
    assert!(!covered.contains("using-superpowers"));
}

#[test]
fn active_step_packet_discloses_only_the_selected_step() {
    let graph = instantiate_template("discover-and-plan", "packet", "Packet").unwrap();
    let step = graph
        .nodes
        .iter()
        .find(|node| node.label.starts_with("Ask Weavatrix"))
        .unwrap();
    let packet = active_step_packet(&graph, &step.id, &["WX-1".to_owned()]).unwrap();

    assert!(packet.instruction.contains("Ask Weavatrix"));
    assert!(!packet.instruction.contains("Finish with"));
    assert_eq!(packet.evidence_ids, ["WX-1"]);
    assert_eq!(packet.graph_id, "packet");
}

#[test]
fn lint_reports_each_safety_and_structure_invariant() {
    let baseline = instantiate_template("bounded-implementation", "lint", "Lint").unwrap();
    assert!(lint_sequence(&baseline).is_empty());

    let mut unreachable = baseline.clone();
    unreachable.edges.retain(|edge| edge.to != "step-5");
    assert_code(&unreachable, DiagnosticCode::UnreachableNode);

    let mut missing_terminal = baseline.clone();
    for node in &mut missing_terminal.nodes {
        if node.kind == NodeKind::Terminal {
            node.kind = NodeKind::Deterministic;
        }
    }
    assert_code(&missing_terminal, DiagnosticCode::MissingTerminal);

    let mut cycle = baseline.clone();
    cycle.edges.push(GraphEdge {
        id: "bad-cycle".to_owned(),
        from: "step-6".to_owned(),
        to: "step-4".to_owned(),
        kind: EdgeKind::Sequence,
        label: "cycle".to_owned(),
        condition: None,
    });
    assert_code(&cycle, DiagnosticCode::ExecutableCycle);

    let mut retry = baseline.clone();
    retry
        .nodes
        .iter_mut()
        .find(|node| node.kind == NodeKind::Retry)
        .unwrap()
        .config
        .remove("maxAttempts");
    assert_code(&retry, DiagnosticCode::UnboundedRetry);

    let mut gate = baseline.clone();
    let gate_id = gate
        .nodes
        .iter()
        .find(|node| node.kind == NodeKind::EvidenceGate)
        .unwrap()
        .id
        .clone();
    gate.edges.retain(|edge| {
        edge.from != gate_id
            || !matches!(
                edge.kind,
                EdgeKind::Failure | EdgeKind::Fallback | EdgeKind::Reject | EdgeKind::Escalates
            )
    });
    assert_code(&gate, DiagnosticCode::GateWithoutFailureRoute);

    let mut unsafe_local = baseline.clone();
    let local = unsafe_local
        .nodes
        .iter_mut()
        .find(|node| node.id == "step-5")
        .unwrap();
    local.kind = NodeKind::LocalModel;
    local.execution = Some(ExecutionPolicy {
        target: ExecutionTarget::Ollama,
        risk: RiskLevel::High,
        max_input_tokens: 1_000,
        max_output_tokens: 200,
        require_evidence: true,
        require_upstream_review: false,
        allow_mutation: true,
        model_profile: Some("unsafe".to_owned()),
    });
    assert_code(&unsafe_local, DiagnosticCode::UnsafeLocalAuthority);

    let mut branch = baseline.clone();
    let branch_node = branch
        .nodes
        .iter_mut()
        .find(|node| node.id == "step-2")
        .unwrap();
    branch_node.kind = NodeKind::Branch;
    branch
        .edges
        .retain(|edge| edge.from != "step-2" || edge.kind != EdgeKind::Conditional);
    assert_code(&branch, DiagnosticCode::BranchWithoutChoices);

    let mut completion = baseline.clone();
    completion
        .nodes
        .iter_mut()
        .find(|node| node.id == "step-5")
        .unwrap()
        .config
        .remove("completionCriteria");
    assert_code(&completion, DiagnosticCode::MissingCompletionCriteria);

    let mut external = baseline;
    external.edges.push(GraphEdge {
        id: "external".to_owned(),
        from: "step-1".to_owned(),
        to: "outside".to_owned(),
        kind: EdgeKind::Sequence,
        label: "external".to_owned(),
        condition: None,
    });
    assert_code(&external, DiagnosticCode::ExternalNodeReference);
}

fn assert_code(graph: &cortex_domain::GraphDocument, expected: DiagnosticCode) {
    let diagnostics = lint_sequence(graph);
    assert!(
        diagnostics.iter().any(|item| item.code == expected),
        "missing {expected:?}: {diagnostics:#?}"
    );
}

#[test]
fn versions_have_a_total_order() {
    let version = templates()[0].version;
    assert!(version < super::TemplateVersion::new(1, 1, 0));
    assert_eq!(version.to_string(), "1.0.0");
}
