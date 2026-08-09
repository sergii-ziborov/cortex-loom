use cortex_domain::EdgeKind;

use super::*;
use crate::tests_roundtrip::semantic_view;

const SKILL: &str = r"---
name: Evidence First
description: Gather facts before proposing a change.
license: MIT
---
# Evidence First

Use observed repository evidence.

## Workflow

1. Inspect the relevant files.
2. Record evidence IDs.
- [ ] Draft the bounded answer after step 2.
- Ask for review [depends: 1, 2]
";

#[test]
fn imports_frontmatter_provenance_and_typed_steps() {
    let graph = import_skill_markdown("skills/evidence/SKILL.md", SKILL).unwrap();
    graph.validate().unwrap();
    assert_eq!(graph.name, "Evidence First");
    assert_eq!(
        graph
            .metadata
            .get("frontmatter.license")
            .map(String::as_str),
        Some("MIT")
    );
    let steps: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| {
            node.config.get("role").and_then(serde_json::Value::as_str) == Some("workflow_step")
        })
        .collect();
    assert_eq!(steps.len(), 4);
    assert!(steps.iter().all(|node| !node.provenance.is_empty()));
    assert_eq!(steps[2].config["marker"], "checklist");
}

#[test]
fn creates_sequence_and_explicit_dependency_edges() {
    let graph = import_skill_markdown("SKILL.md", SKILL).unwrap();
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Sequence)
            .count(),
        3
    );
    let dependencies: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.label == "explicit dependency")
        .collect();
    assert_eq!(dependencies.len(), 3);
    assert!(
        dependencies
            .iter()
            .any(|edge| edge.from == "step-2" && edge.to == "step-3")
    );
    assert!(
        dependencies
            .iter()
            .any(|edge| edge.from == "step-1" && edge.to == "step-4")
    );
}

#[test]
fn canonical_round_trip_keeps_workflow_semantics() {
    let first = import_skill_markdown("SKILL.md", SKILL).unwrap();
    let markdown = export_skill_markdown(&first).unwrap();
    let second = import_skill_markdown("SKILL.md", &markdown).unwrap();

    let semantic_nodes = |graph: &cortex_domain::GraphDocument| {
        graph
            .nodes
            .iter()
            .map(|node| {
                (
                    node.config
                        .get("role")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    node.label.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(semantic_nodes(&first), semantic_nodes(&second));
    assert_eq!(
        first
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Sequence)
            .count(),
        second
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Sequence)
            .count()
    );
}

#[test]
fn graph_dependency_edits_are_reflected_in_exported_markdown() {
    let mut graph = import_skill_markdown("SKILL.md", SKILL).unwrap();
    graph
        .edges
        .retain(|edge| edge.label != "explicit dependency" || edge.to != "step-4");
    let markdown = export_skill_markdown(&graph).unwrap();
    assert!(!markdown.contains("[depends: 1, 2]"));

    let second = import_skill_markdown("SKILL.md", &markdown).unwrap();
    assert!(
        second
            .edges
            .iter()
            .all(|edge| edge.label != "explicit dependency" || edge.to != "step-4")
    );
}

#[test]
fn imports_without_frontmatter() {
    let graph = import_skill_markdown("safe-review/SKILL.md", "# Safe Review\n\n- Inspect only.\n")
        .unwrap();
    assert_eq!(graph.name, "Safe Review");
    assert_eq!(graph.nodes.len(), 2);
}

#[test]
fn compiled_graph_starts_unsaved_at_revision_zero() {
    assert_eq!(
        import_skill_markdown("safe-review/SKILL.md", "# Safe Review\n")
            .unwrap()
            .revision,
        0
    );
}

#[test]
fn a_byte_order_mark_does_not_hide_the_frontmatter() {
    // Windows editors write one routinely. Before this was handled, such a
    // document imported with the H1 as its name and no metadata at all, and
    // an unclosed frontmatter block stopped being an error because there was
    // no frontmatter left to be unclosed.
    let document = "---\nname: Marked\ndescription: kept\nlicense: MIT\n---\n# Marked\n\n- Step.\n";
    let plain = import_skill_markdown("SKILL.md", document).unwrap();
    let marked = import_skill_markdown("SKILL.md", &format!("\u{feff}{document}")).unwrap();
    assert_eq!(marked.name, "Marked");
    assert_eq!(
        marked
            .metadata
            .get("frontmatter.license")
            .map(String::as_str),
        Some("MIT")
    );
    assert_eq!(semantic_view(&plain), semantic_view(&marked));

    assert!(matches!(
        import_skill_markdown("SKILL.md", "\u{feff}---\nname: Broken\n# unclosed\n"),
        Err(SkillError::InvalidFrontmatter(_))
    ));
}

#[test]
fn a_declared_kind_becomes_a_typed_node_and_survives_the_round_trip() {
    let markdown = "---\nname: Gated\ndescription: With gates.\n---\n# Gated\n\n\
                    ## Work\n\n\
                    1. Collect the evidence.\n\
                    2. Run the suite. [kind: test_gate] [depends: 1]\n\
                    3. Approve or reject with a reason. [kind: review-gate] [depends: 2]\n\
                    4. Hand the rest to the upstream agent. [kind: upstream_agent]\n";
    let graph = import_skill_markdown("SKILL.md", markdown).unwrap();
    let kinds: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| {
            node.config.get("role").and_then(serde_json::Value::as_str) == Some("workflow_step")
        })
        .map(|node| (node.kind, node.label.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            (
                cortex_domain::NodeKind::Deterministic,
                "Collect the evidence."
            ),
            (cortex_domain::NodeKind::TestGate, "Run the suite."),
            (
                cortex_domain::NodeKind::ReviewGate,
                "Approve or reject with a reason."
            ),
            (
                cortex_domain::NodeKind::UpstreamAgent,
                "Hand the rest to the upstream agent."
            ),
        ],
        "a hyphenated spelling is accepted and labels carry no annotation text"
    );

    let exported = export_skill_markdown(&graph).unwrap();
    assert!(exported.contains("Run the suite. [kind: test_gate] [depends: 1]"));
    let second = import_skill_markdown("SKILL.md", &exported).unwrap();
    assert_eq!(semantic_view(&graph), semantic_view(&second));
    assert_eq!(
        exported,
        export_skill_markdown(&second).unwrap(),
        "export stays a fixpoint with annotations present"
    );
}

#[test]
fn changing_a_node_kind_in_the_graph_rewrites_the_markdown() {
    // The graph is canonical; the Markdown is its view. Editing the kind in
    // the editor has to show up in the export, not be overwritten by whatever
    // the original text said.
    let mut graph =
        import_skill_markdown("SKILL.md", "# Flow\n\n1. Check it. [kind: test_gate]\n").unwrap();
    for node in &mut graph.nodes {
        if node.id == "step-1" {
            node.kind = cortex_domain::NodeKind::HumanGate;
        }
    }
    let exported = export_skill_markdown(&graph).unwrap();
    assert!(exported.contains("[kind: human_gate]"), "{exported}");
    assert!(!exported.contains("test_gate"));
}

#[test]
fn a_fenced_block_is_titled_by_its_content_not_its_fence() {
    let graph = import_skill_markdown(
        "SKILL.md",
        "# Flow\n\n```text\nA test that fails for the wrong reason proves nothing.\n```\n",
    )
    .unwrap();
    let guidance = graph
        .nodes
        .iter()
        .find(|node| {
            node.config.get("role").and_then(serde_json::Value::as_str) == Some("guidance")
        })
        .expect("the block imported");
    assert_eq!(
        guidance.label,
        "A test that fails for the wrong reason proves nothing."
    );
    assert!(guidance.description.starts_with("```text"), "source kept");
}

#[test]
fn rejects_unclosed_frontmatter() {
    let error = import_skill_markdown("SKILL.md", "---\nname: Broken\n# Body\n").unwrap_err();
    assert!(matches!(error, SkillError::InvalidFrontmatter(_)));
}
