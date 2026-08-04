use cortex_domain::EdgeKind;

use super::*;

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
fn rejects_unclosed_frontmatter() {
    let error = import_skill_markdown("SKILL.md", "---\nname: Broken\n# Body\n").unwrap_err();
    assert!(matches!(error, SkillError::InvalidFrontmatter(_)));
}

const FIXTURES: &[(&str, &str)] = &[
    (
        "fixtures/test-driven-development.md",
        include_str!("../fixtures/test-driven-development.md"),
    ),
    (
        "fixtures/systematic-debugging.md",
        include_str!("../fixtures/systematic-debugging.md"),
    ),
    (
        "fixtures/grounded-review.md",
        include_str!("../fixtures/grounded-review.md"),
    ),
];

fn semantic_view(graph: &cortex_domain::GraphDocument) -> Vec<(String, String)> {
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
        .collect()
}

fn edge_counts(graph: &cortex_domain::GraphDocument) -> (usize, usize) {
    (
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Sequence)
            .count(),
        graph
            .edges
            .iter()
            .filter(|edge| edge.label == "explicit dependency")
            .count(),
    )
}

#[test]
fn superpowers_format_fixtures_round_trip_semantically() {
    for (source, markdown) in FIXTURES {
        let first = import_skill_markdown(source, markdown)
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        first.validate().unwrap();
        assert!(
            first.metadata.contains_key("frontmatter.license"),
            "{source}: frontmatter extras survive import"
        );
        let (sequences, dependencies) = edge_counts(&first);
        assert!(sequences >= 4, "{source}: step chain imported");
        assert!(
            dependencies >= 1,
            "{source}: explicit dependencies imported"
        );

        let exported = export_skill_markdown(&first).unwrap();
        let second = import_skill_markdown(source, &exported)
            .unwrap_or_else(|error| panic!("{source} re-import: {error}"));
        assert_eq!(
            semantic_view(&first),
            semantic_view(&second),
            "{source}: node semantics are stable"
        );
        assert_eq!(
            edge_counts(&first),
            edge_counts(&second),
            "{source}: workflow edges are stable"
        );

        // A second export is byte-identical: the canonical view is a fixpoint.
        let exported_again = export_skill_markdown(&second).unwrap();
        assert_eq!(exported, exported_again, "{source}: export is stable");
    }
}

#[test]
fn crlf_input_matches_lf_input_semantically() {
    let (source, markdown) = FIXTURES[0];
    let lf = import_skill_markdown(source, markdown).unwrap();
    let crlf = import_skill_markdown(source, &markdown.replace('\n', "\r\n")).unwrap();
    assert_eq!(semantic_view(&lf), semantic_view(&crlf));
    assert_eq!(edge_counts(&lf), edge_counts(&crlf));
}

#[test]
fn unicode_labels_survive_the_round_trip() {
    let graph = import_skill_markdown("fixtures/grounded-review.md", FIXTURES[2].1).unwrap();
    assert!(
        graph
            .nodes
            .iter()
            .any(|node| node.label.contains('✓') || node.description.contains('✓')),
        "unicode guidance imported"
    );
    let exported = export_skill_markdown(&graph).unwrap();
    let second = import_skill_markdown("fixtures/grounded-review.md", &exported).unwrap();
    assert_eq!(semantic_view(&graph), semantic_view(&second));
}

#[test]
fn long_dependency_chains_scale_and_round_trip() {
    use std::fmt::Write as _;
    let mut markdown = String::from(
        "---\nname: Long Chain\ndescription: pressure\n---\n# Long Chain\n\n## Steps\n\n",
    );
    for step in 1..=60 {
        if step == 1 {
            markdown.push_str("1. Start the chain.\n");
        } else {
            let _ = writeln!(markdown, "{step}. Continue after step {}.", step - 1);
        }
    }
    let graph = import_skill_markdown("chain/SKILL.md", &markdown).unwrap();
    let (sequences, dependencies) = edge_counts(&graph);
    assert_eq!(sequences, 59);
    assert_eq!(dependencies, 59, "every 'after step N' hint became an edge");

    let exported = export_skill_markdown(&graph).unwrap();
    let second = import_skill_markdown("chain/SKILL.md", &exported).unwrap();
    assert_eq!(edge_counts(&second), (59, 59));
}

#[test]
fn frontmatter_escapes_do_not_accumulate_across_round_trips() {
    // A double-quoted frontmatter scalar is a JSON string literal on export,
    // so import must unescape it. Otherwise every round trip adds a layer:
    // `say "hi"` -> `say \"hi\"` -> `say \\\"hi\\\"`.
    let quoted =
        "---\nname: Fast\ndescription: \"Run the \\\"fast\\\" path\"\n---\n# Fast\n\n- Do it.\n";
    let graph = import_skill_markdown("SKILL.md", quoted).unwrap();
    assert_eq!(
        graph.metadata.get("description").map(String::as_str),
        Some("Run the \"fast\" path"),
        "double-quoted scalars are unescaped on import"
    );

    // Every hostile scalar shape must be an export fixpoint.
    for (label, source) in [
        (
            "quote in description",
            "---\nname: Q\ndescription: say \"hi\"\n---\n# Q\n\n- Do it.\n",
        ),
        (
            "quote in name",
            "---\nname: Say \"hi\"\ndescription: p\n---\n# Say \"hi\"\n\n- Do it.\n",
        ),
        (
            "windows path backslashes",
            "---\nname: B\ndescription: C:\\path\\to\n---\n# B\n\n- Do it.\n",
        ),
        (
            "control character",
            "---\nname: C\ndescription: A\u{7}B\n---\n# C\n\n- Do it.\n",
        ),
        (
            "quote in an extra key",
            "---\nname: E\ndescription: p\nowner: a \"b\" c\n---\n# E\n\n- Do it.\n",
        ),
    ] {
        let first = import_skill_markdown("SKILL.md", source).unwrap();
        let exported = export_skill_markdown(&first).unwrap();
        let second = import_skill_markdown("SKILL.md", &exported).unwrap();
        let exported_again = export_skill_markdown(&second).unwrap();
        assert_eq!(
            exported, exported_again,
            "{label} is not an export fixpoint"
        );
        assert_eq!(semantic_view(&first), semantic_view(&second), "{label}");
    }

    // Single-quoted and plain scalars keep their exact value.
    let mixed = "---\nname: S\ndescription: p\nreviewers: \"one required\"\nteam: 'core'\nplain: no quotes\n---\n# S\n\n- Do it.\n";
    let graph = import_skill_markdown("SKILL.md", mixed).unwrap();
    assert_eq!(
        graph
            .metadata
            .get("frontmatter.reviewers")
            .map(String::as_str),
        Some("one required")
    );
    assert_eq!(
        graph.metadata.get("frontmatter.team").map(String::as_str),
        Some("core")
    );
    assert_eq!(
        graph.metadata.get("frontmatter.plain").map(String::as_str),
        Some("no quotes")
    );
}

#[test]
fn empty_structural_items_are_skipped_not_fatal() {
    // A stray `- `, `1. `, or `## ` is valid Markdown. It carries no workflow
    // meaning, so it is skipped rather than failing the whole document on the
    // empty-label invariant.
    for (label, source) in [
        ("empty bullet", "# T\n\n- \n- Real step.\n"),
        ("empty numbered", "# T\n\n1. \n2. Real step.\n"),
        ("empty heading", "# T\n\n## \n\n- Real step.\n"),
    ] {
        let graph = import_skill_markdown("SKILL.md", source)
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        graph
            .validate()
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        let steps: Vec<_> = graph
            .nodes
            .iter()
            .filter(|node| {
                node.config.get("role").and_then(serde_json::Value::as_str) == Some("workflow_step")
            })
            .collect();
        assert_eq!(steps.len(), 1, "{label}: only the real step survives");
        assert_eq!(steps[0].label, "Real step.", "{label}");

        let exported = export_skill_markdown(&graph).unwrap();
        let second = import_skill_markdown("SKILL.md", &exported)
            .unwrap_or_else(|error| panic!("{label} re-import: {error}"));
        assert_eq!(semantic_view(&graph), semantic_view(&second), "{label}");
    }
}

#[test]
fn hostile_frontmatter_is_rejected_or_contained() {
    // A value-less line is a hard error, not a silent skip.
    assert!(matches!(
        import_skill_markdown("SKILL.md", "---\nno delimiter here\n---\n# X\n"),
        Err(SkillError::InvalidFrontmatter(_))
    ));
    // An empty key is rejected.
    assert!(matches!(
        import_skill_markdown("SKILL.md", "---\n: value\n---\n# X\n"),
        Err(SkillError::InvalidFrontmatter(_))
    ));
    // An empty document with empty frontmatter still compiles to a bare
    // skill node named from the source path.
    let graph = import_skill_markdown("bare-skill/SKILL.md", "---\n---\n").unwrap();
    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.name, "SKILL");
}
