use cortex_domain::EdgeKind;

use super::*;

/// The shipped library doubles as the round-trip fixture set: whatever we
/// hand a consumer must survive its own compiler.
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
    (
        "fixtures/evidence-first-change.md",
        include_str!("../fixtures/evidence-first-change.md"),
    ),
    (
        "fixtures/verification-before-completion.md",
        include_str!("../fixtures/verification-before-completion.md"),
    ),
    (
        "fixtures/blast-radius-analysis.md",
        include_str!("../fixtures/blast-radius-analysis.md"),
    ),
    (
        "fixtures/interface-contract-change.md",
        include_str!("../fixtures/interface-contract-change.md"),
    ),
    (
        "fixtures/dependency-upgrade.md",
        include_str!("../fixtures/dependency-upgrade.md"),
    ),
    (
        "fixtures/performance-investigation.md",
        include_str!("../fixtures/performance-investigation.md"),
    ),
    (
        "fixtures/incident-response.md",
        include_str!("../fixtures/incident-response.md"),
    ),
    (
        "fixtures/migration-cutover.md",
        include_str!("../fixtures/migration-cutover.md"),
    ),
    (
        "fixtures/api-versioning.md",
        include_str!("../fixtures/api-versioning.md"),
    ),
    (
        "fixtures/flaky-test-quarantine.md",
        include_str!("../fixtures/flaky-test-quarantine.md"),
    ),
    (
        "fixtures/security-threat-model.md",
        include_str!("../fixtures/security-threat-model.md"),
    ),
    (
        "fixtures/observability-first.md",
        include_str!("../fixtures/observability-first.md"),
    ),
    (
        "fixtures/data-migration.md",
        include_str!("../fixtures/data-migration.md"),
    ),
    (
        "fixtures/feature-flag-rollout.md",
        include_str!("../fixtures/feature-flag-rollout.md"),
    ),
    (
        "fixtures/documentation-sync.md",
        include_str!("../fixtures/documentation-sync.md"),
    ),
    (
        "fixtures/release-checklist.md",
        include_str!("../fixtures/release-checklist.md"),
    ),
    (
        "fixtures/backlog-triage.md",
        include_str!("../fixtures/backlog-triage.md"),
    ),
    (
        "fixtures/accessibility-audit.md",
        include_str!("../fixtures/accessibility-audit.md"),
    ),
    (
        "fixtures/configuration-drift.md",
        include_str!("../fixtures/configuration-drift.md"),
    ),
    (
        "fixtures/cache-invalidation.md",
        include_str!("../fixtures/cache-invalidation.md"),
    ),
    (
        "fixtures/concurrency-bug-hunt.md",
        include_str!("../fixtures/concurrency-bug-hunt.md"),
    ),
    (
        "fixtures/schema-evolution.md",
        include_str!("../fixtures/schema-evolution.md"),
    ),
    (
        "fixtures/dependency-audit.md",
        include_str!("../fixtures/dependency-audit.md"),
    ),
    (
        "fixtures/error-budget-review.md",
        include_str!("../fixtures/error-budget-review.md"),
    ),
    (
        "fixtures/capacity-planning.md",
        include_str!("../fixtures/capacity-planning.md"),
    ),
    (
        "fixtures/rollback-drill.md",
        include_str!("../fixtures/rollback-drill.md"),
    ),
    (
        "fixtures/contract-testing.md",
        include_str!("../fixtures/contract-testing.md"),
    ),
    (
        "fixtures/postmortem-writeup.md",
        include_str!("../fixtures/postmortem-writeup.md"),
    ),
];

#[test]
fn every_bundled_skill_compiles_and_has_a_unique_graph_id() {
    let mut ids = std::collections::HashSet::new();
    let mut graph_ids = std::collections::HashSet::new();
    for skill in bundled_skills() {
        assert!(ids.insert(skill.id), "duplicate bundled id {}", skill.id);
        let graph = import_skill_markdown(skill.source, skill.markdown)
            .unwrap_or_else(|error| panic!("{}: {error}", skill.id));
        graph.validate().unwrap();
        assert!(
            graph_ids.insert(graph.id.clone()),
            "bundled skills collide on graph id {}",
            graph.id
        );
        assert!(
            graph.nodes.len() > 3,
            "{} should carry real workflow",
            skill.id
        );
    }
    assert_eq!(
        ids.len(),
        FIXTURES.len(),
        "every bundled skill is also a round-trip fixture"
    );
}

/// The library has to be process graphs, not tables of contents.
///
/// Before typed steps existed, every bundled skill compiled to `skill` and
/// `deterministic` nodes only: readable, but with nothing to gate, nothing to
/// escalate, and no end. A run over such a graph has no decision to make.
#[test]
fn every_bundled_skill_is_a_process_graph_not_a_flat_list() {
    use cortex_domain::NodeKind;
    const GATES: [NodeKind; 5] = [
        NodeKind::QualityGate,
        NodeKind::HumanGate,
        NodeKind::TestGate,
        NodeKind::ReviewGate,
        NodeKind::EvidenceGate,
    ];
    for skill in bundled_skills() {
        let graph = import_skill_markdown(skill.source, skill.markdown).unwrap();
        let kinds: std::collections::HashSet<_> =
            graph.nodes.iter().map(|node| node.kind).collect();
        assert!(
            kinds.iter().any(|kind| GATES.contains(kind)),
            "{} has no gate: nothing in it can refuse",
            skill.id
        );
        assert!(
            kinds.contains(&NodeKind::Terminal),
            "{} never ends",
            skill.id
        );
        assert!(
            kinds.contains(&NodeKind::UpstreamAgent) || kinds.contains(&NodeKind::Handoff),
            "{} has no escalation path",
            skill.id
        );
        assert!(
            kinds.len() >= 5,
            "{} uses only {} node kinds",
            skill.id,
            kinds.len()
        );
    }
}

/// A guidance node must be titled by something a reader can act on.
#[test]
fn no_bundled_node_is_labelled_by_markup_or_an_annotation() {
    for skill in bundled_skills() {
        let graph = import_skill_markdown(skill.source, skill.markdown).unwrap();
        for node in &graph.nodes {
            assert!(
                !node.label.starts_with("```"),
                "{}: node {} is labelled by a fence",
                skill.id,
                node.id
            );
            for marker in ["[kind:", "[depends:"] {
                assert!(
                    !node.label.to_ascii_lowercase().contains(marker),
                    "{}: node {} leaks {marker} into its label",
                    skill.id,
                    node.id
                );
            }
        }
    }
}

pub(crate) fn semantic_view(graph: &cortex_domain::GraphDocument) -> Vec<(String, String)> {
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
fn a_multi_line_name_does_not_grow_the_document() {
    // The `# ` title must stay on one line. Writing a multi-line name raw
    // would spill the remainder into the body, the next import would read it
    // as extra nodes, and the document would grow on every cycle.
    let source = "---\nname: \"Two\\nLines\"\ndescription: d\n---\n# X\n\n- Do it.\n";
    let first = import_skill_markdown("S.md", source).unwrap();
    assert_eq!(first.name, "Two\nLines", "the exact name is preserved");

    let exported = export_skill_markdown(&first).unwrap();
    assert!(
        exported.contains("\n# Two Lines\n"),
        "the title is collapsed to one line: {exported:?}"
    );
    let second = import_skill_markdown("S.md", &exported).unwrap();
    let exported_again = export_skill_markdown(&second).unwrap();
    assert_eq!(exported, exported_again, "export is a fixpoint");
    assert_eq!(
        first.nodes.len(),
        second.nodes.len(),
        "the node count does not grow"
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
