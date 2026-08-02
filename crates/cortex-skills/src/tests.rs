use cortex_domain::EdgeKind;

use super::*;

const SKILL: &str = r#"---
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
"#;

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
fn imports_without_frontmatter() {
    let graph = import_skill_markdown("safe-review/SKILL.md", "# Safe Review\n\n- Inspect only.\n")
        .unwrap();
    assert_eq!(graph.name, "Safe Review");
    assert_eq!(graph.nodes.len(), 2);
}

#[test]
fn rejects_unclosed_frontmatter() {
    let error = import_skill_markdown("SKILL.md", "---\nname: Broken\n# Body\n").unwrap_err();
    assert!(matches!(error, SkillError::InvalidFrontmatter(_)));
}
