use std::collections::HashSet;

use super::{instantiate_template, templates};

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
fn versions_have_a_total_order() {
    let version = templates()[0].version;
    assert!(version < super::TemplateVersion::new(1, 1, 0));
    assert_eq!(version.to_string(), "1.0.0");
}
