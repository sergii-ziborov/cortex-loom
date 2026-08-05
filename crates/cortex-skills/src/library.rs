//! Importing a whole methodology library.
//!
//! The bundled skills exist so the editor is not empty. They are not a
//! substitute for the libraries people already write: the portable `SKILL.md`
//! format has a large public ecosystem, and a compiler that can only read its
//! own fixtures is a compiler nobody needs.
//!
//! This module is pure. It takes documents that someone else read and returns
//! validated graphs, collisions resolved, failures reported, and attribution
//! carried alongside. Walking a directory is the caller's job, which keeps
//! this crate free of filesystem and transport concerns.
//!
//! ## Attribution
//!
//! A library is someone else's work. [`LibraryImport::notices`] carries the
//! licence and notice files found beside the skills so a consumer can show
//! them before anything is stored, and every imported graph records where it
//! came from in `metadata["library"]`. Nothing here decides whether a licence
//! permits the use — that is a human decision, and the data exists so a human
//! can make it.

use std::collections::HashMap;

use cortex_domain::GraphDocument;

use crate::{SkillError, import_skill_markdown};

/// Largest library this function will compile in one call.
pub const MAX_LIBRARY_SKILLS: usize = 512;

/// One candidate document, already read by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    /// Provenance string, normally the path relative to the library root.
    pub source: String,
    pub markdown: String,
}

/// A licence or notice found beside the skills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryNotice {
    pub source: String,
    pub text: String,
}

/// One skill that compiled.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedSkill {
    pub source: String,
    /// Set when the graph id collided and was disambiguated.
    pub renamed_from: Option<String>,
    pub graph: GraphDocument,
}

/// One document that did not compile, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedDocument {
    pub source: String,
    pub reason: String,
}

/// The result of compiling a library.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibraryImport {
    pub skills: Vec<ImportedSkill>,
    pub skipped: Vec<SkippedDocument>,
    pub notices: Vec<LibraryNotice>,
}

impl LibraryImport {
    /// True when nothing compiled: a caller should say so rather than
    /// silently storing an empty library.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

/// Compile a library into validated graphs.
///
/// Entries are processed in the order given, so the result is deterministic
/// for a deterministic walk. A document that fails to compile is skipped with
/// its error rather than failing the whole library — one malformed file in
/// somebody's repository must not cost the other ninety-nine.
///
/// Two skills with the same title produce the same graph id. Rather than drop
/// the second, the id is suffixed (`-2`, `-3`, …) and
/// [`ImportedSkill::renamed_from`] records it: a library that names two
/// documents "Code Review" still imports both, and the report says what
/// happened.
#[must_use]
pub fn import_library(
    entries: impl IntoIterator<Item = LibraryEntry>,
    notices: Vec<LibraryNotice>,
    library: &str,
) -> LibraryImport {
    let mut result = LibraryImport {
        notices,
        ..LibraryImport::default()
    };
    let mut seen: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        if result.skills.len() >= MAX_LIBRARY_SKILLS {
            result.skipped.push(SkippedDocument {
                source: entry.source,
                reason: format!("library exceeds {MAX_LIBRARY_SKILLS} skills"),
            });
            continue;
        }
        match import_skill_markdown(&entry.source, &entry.markdown) {
            Ok(graph) => result
                .skills
                .push(disambiguate(graph, &entry.source, library, &mut seen)),
            Err(error) => result.skipped.push(SkippedDocument {
                source: entry.source,
                reason: describe(&error),
            }),
        }
    }
    result
}

fn disambiguate(
    mut graph: GraphDocument,
    source: &str,
    library: &str,
    seen: &mut HashMap<String, usize>,
) -> ImportedSkill {
    graph
        .metadata
        .insert("library".to_owned(), library.to_owned());
    let original = graph.id.clone();
    let count = seen.entry(original.clone()).or_insert(0);
    *count += 1;
    let renamed_from = if *count == 1 {
        None
    } else {
        graph.id = format!("{original}-{count}");
        Some(original)
    };
    ImportedSkill {
        source: source.to_owned(),
        renamed_from,
        graph,
    }
}

fn describe(error: &SkillError) -> String {
    error.to_string()
}

/// One line of the catalogue an agent keeps in context.
///
/// Everything here is cheap: what the workflow is called, when it applies,
/// and how big it is. The steps themselves are not — they are fetched by id
/// when a task actually matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillIndexEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The trigger line. Falls back to the description when the author did
    /// not write a `when-to-use` frontmatter key.
    pub when_to_use: String,
    pub steps: usize,
    pub gates: usize,
}

/// Summarise one compiled skill graph for the catalogue.
///
/// Returns `None` for a graph this compiler did not produce: a catalogue of
/// workflows must not advertise something `skill_read` cannot render.
#[must_use]
pub fn index_entry(graph: &GraphDocument) -> Option<SkillIndexEntry> {
    if graph.metadata.get("compiler").map(String::as_str) != Some("cortex-skills") {
        return None;
    }
    let description = graph
        .metadata
        .get("description")
        .cloned()
        .unwrap_or_default();
    let when_to_use = graph
        .metadata
        .get("frontmatter.when-to-use")
        .or_else(|| graph.metadata.get("frontmatter.when_to_use"))
        .cloned()
        .unwrap_or_else(|| description.clone());
    let steps = graph
        .nodes
        .iter()
        .filter(|node| {
            node.config.get("role").and_then(serde_json::Value::as_str) == Some("workflow_step")
        })
        .count();
    let gates = graph
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.kind,
                cortex_domain::NodeKind::QualityGate
                    | cortex_domain::NodeKind::HumanGate
                    | cortex_domain::NodeKind::TestGate
                    | cortex_domain::NodeKind::ReviewGate
                    | cortex_domain::NodeKind::EvidenceGate
            )
        })
        .count();
    Some(SkillIndexEntry {
        id: graph.id.clone(),
        name: graph.name.clone(),
        description,
        when_to_use,
        steps,
        gates,
    })
}

/// Render the catalogue an agent keeps loaded.
///
/// This is the whole point of the two-tier surface: an agent that carries
/// thirty workflows in its prompt pays for thirty workflows on every turn,
/// whether or not the task is about any of them. It carries this instead —
/// one line each — and calls `skill_read` for the one that matches.
#[must_use]
pub fn render_index(entries: &[SkillIndexEntry]) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    output.push_str(
        "# Cortex Loom workflow catalogue\n\n\
         Pick at most one workflow, then fetch it with `skill_read { id }`. \
         Do not load a workflow you are not about to follow: the catalogue is \
         cheap and the workflows are not.\n\n",
    );
    for entry in entries {
        let _ = writeln!(
            output,
            "- `{}` — **{}**. {} ({} steps, {} gate{})",
            entry.id,
            entry.name,
            entry.when_to_use,
            entry.steps,
            entry.gates,
            if entry.gates == 1 { "" } else { "s" }
        );
    }
    if entries.is_empty() {
        output.push_str("- (no workflows stored)\n");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{LibraryEntry, LibraryNotice, import_library};

    fn entry(source: &str, name: &str) -> LibraryEntry {
        LibraryEntry {
            source: source.to_owned(),
            markdown: format!(
                "---\nname: {name}\ndescription: imported\n---\n# {name}\n\n\
                 ## Steps\n\n1. Do the first thing.\n2. Do the second. [depends: 1]\n"
            ),
        }
    }

    #[test]
    fn a_malformed_document_costs_only_itself() {
        let import = import_library(
            vec![
                entry("skills/alpha/SKILL.md", "Alpha"),
                LibraryEntry {
                    source: "skills/broken/SKILL.md".to_owned(),
                    markdown: "---\nname: Broken\n# no closing delimiter\n".to_owned(),
                },
                entry("skills/beta/SKILL.md", "Beta"),
            ],
            Vec::new(),
            "/checkout/superpowers",
        );
        assert_eq!(import.skills.len(), 2, "the good documents still import");
        assert_eq!(import.skipped.len(), 1);
        assert_eq!(import.skipped[0].source, "skills/broken/SKILL.md");
        assert!(import.skipped[0].reason.contains("frontmatter"));
    }

    #[test]
    fn colliding_titles_are_disambiguated_rather_than_dropped() {
        let import = import_library(
            vec![
                entry("skills/one/SKILL.md", "Code Review"),
                entry("skills/two/SKILL.md", "Code Review"),
                entry("skills/three/SKILL.md", "Code Review"),
            ],
            Vec::new(),
            "/checkout/library",
        );
        let ids: Vec<&str> = import
            .skills
            .iter()
            .map(|skill| skill.graph.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "code-review-skill",
                "code-review-skill-2",
                "code-review-skill-3"
            ]
        );
        assert_eq!(import.skills[0].renamed_from, None);
        assert_eq!(
            import.skills[2].renamed_from.as_deref(),
            Some("code-review-skill")
        );
        assert!(import.skipped.is_empty());
    }

    #[test]
    fn provenance_and_attribution_travel_with_the_import() {
        let notice = LibraryNotice {
            source: "LICENSE".to_owned(),
            text: "MIT License\n\nCopyright (c) someone".to_owned(),
        };
        let import = import_library(
            vec![entry("skills/alpha/SKILL.md", "Alpha")],
            vec![notice.clone()],
            "/checkout/superpowers",
        );
        let graph = &import.skills[0].graph;
        assert_eq!(
            graph.metadata.get("library").map(String::as_str),
            Some("/checkout/superpowers")
        );
        assert_eq!(
            graph.metadata.get("source").map(String::as_str),
            Some("skills/alpha/SKILL.md"),
            "the per-document provenance the compiler wrote is untouched"
        );
        assert_eq!(import.notices, [notice], "attribution is carried, not lost");
        assert_eq!(graph.revision, 0, "an import is unsaved until stored");
    }

    #[test]
    fn an_empty_library_reports_itself_as_empty() {
        let import = import_library(Vec::new(), Vec::new(), "/checkout/empty");
        assert!(import.is_empty());
    }

    #[test]
    fn the_catalogue_costs_a_fraction_of_the_workflows_it_lists() {
        // The reason the two-tier surface exists, asserted rather than
        // assumed: an agent that keeps the catalogue loaded must not be
        // paying anything close to what keeping the workflows loaded costs.
        let skills: Vec<_> = crate::bundled_skills()
            .iter()
            .map(|skill| crate::import_skill_markdown(skill.source, skill.markdown).unwrap())
            .collect();
        let entries: Vec<_> = skills
            .iter()
            .map(|graph| super::index_entry(graph).expect("bundled graphs are skill graphs"))
            .collect();
        assert_eq!(entries.len(), skills.len());

        let catalogue = super::render_index(&entries).chars().count();
        let bodies: usize = crate::bundled_skills()
            .iter()
            .map(|skill| skill.markdown.chars().count())
            .sum();
        assert!(
            catalogue * 4 < bodies,
            "catalogue {catalogue} chars vs {bodies} chars of workflows: not worth the indirection"
        );
    }

    #[test]
    fn a_graph_this_compiler_did_not_produce_is_not_advertised() {
        let foreign = cortex_domain::default_control_plane();
        assert_eq!(super::index_entry(&foreign), None);
    }
}
