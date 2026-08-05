#![doc = include_str!("../README.md")]

mod export;
mod import;
pub mod library;

use std::fmt::{Display, Formatter};

use cortex_domain::{GraphDocument, GraphError};

pub use export::export_skill_markdown;
pub use import::import_skill_markdown;
pub use library::{
    ImportedSkill, LibraryEntry, LibraryImport, LibraryNotice, SkillIndexEntry, import_library,
    index_entry, render_index,
};

/// A failure to parse, validate, or export a skill workflow.
///
/// Marked `#[non_exhaustive]` so new failure modes can be reported without a
/// breaking release; match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkillError {
    /// The `---` frontmatter block is malformed.
    InvalidFrontmatter(String),
    /// The compiled graph failed [`cortex_domain`] validation.
    InvalidGraph(String),
    /// The graph was not produced by this compiler and cannot be exported.
    UnsupportedGraph(String),
}

impl Display for SkillError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFrontmatter(message) => {
                write!(formatter, "invalid frontmatter: {message}")
            }
            Self::InvalidGraph(message) => write!(formatter, "invalid skill graph: {message}"),
            Self::UnsupportedGraph(message) => {
                write!(formatter, "unsupported skill graph: {message}")
            }
        }
    }
}

impl std::error::Error for SkillError {}

impl From<GraphError> for SkillError {
    fn from(error: GraphError) -> Self {
        Self::InvalidGraph(error.to_string())
    }
}

/// One methodology skill shipped with this crate.
///
/// The library exists so a consumer starts with working methodology instead
/// of an empty editor: compile a bundled skill with
/// [`import_skill_markdown`], edit the graph, and export it back to Markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundledSkill {
    /// Stable identifier, matching the file stem.
    pub id: &'static str,
    /// Provenance string to pass to [`import_skill_markdown`].
    pub source: &'static str,
    /// The `SKILL.md` document.
    pub markdown: &'static str,
}

/// The methodology skills shipped with this crate.
///
/// ```
/// for skill in cortex_skills::bundled_skills() {
///     let graph = cortex_skills::import_skill_markdown(skill.source, skill.markdown)?;
///     assert!(!graph.nodes.is_empty());
/// }
/// # Ok::<(), cortex_skills::SkillError>(())
/// ```
#[must_use]
pub const fn bundled_skills() -> &'static [BundledSkill] {
    &[
        BundledSkill {
            id: "test-driven-development",
            source: "cortex-skills/fixtures/test-driven-development.md",
            markdown: include_str!("../fixtures/test-driven-development.md"),
        },
        BundledSkill {
            id: "systematic-debugging",
            source: "cortex-skills/fixtures/systematic-debugging.md",
            markdown: include_str!("../fixtures/systematic-debugging.md"),
        },
        BundledSkill {
            id: "grounded-review",
            source: "cortex-skills/fixtures/grounded-review.md",
            markdown: include_str!("../fixtures/grounded-review.md"),
        },
        BundledSkill {
            id: "evidence-first-change",
            source: "cortex-skills/fixtures/evidence-first-change.md",
            markdown: include_str!("../fixtures/evidence-first-change.md"),
        },
        BundledSkill {
            id: "blast-radius-analysis",
            source: "cortex-skills/fixtures/blast-radius-analysis.md",
            markdown: include_str!("../fixtures/blast-radius-analysis.md"),
        },
        BundledSkill {
            id: "interface-contract-change",
            source: "cortex-skills/fixtures/interface-contract-change.md",
            markdown: include_str!("../fixtures/interface-contract-change.md"),
        },
        BundledSkill {
            id: "dependency-upgrade",
            source: "cortex-skills/fixtures/dependency-upgrade.md",
            markdown: include_str!("../fixtures/dependency-upgrade.md"),
        },
        BundledSkill {
            id: "performance-investigation",
            source: "cortex-skills/fixtures/performance-investigation.md",
            markdown: include_str!("../fixtures/performance-investigation.md"),
        },
        BundledSkill {
            id: "incident-response",
            source: "cortex-skills/fixtures/incident-response.md",
            markdown: include_str!("../fixtures/incident-response.md"),
        },
    ]
}

/// A single-line rendering of a skill name for the Markdown `# ` title.
///
/// Frontmatter carries the exact name; the title must stay on one line.
/// Writing a multi-line name straight into the heading would spill the
/// remainder into the body, where the next import reads it as extra nodes —
/// so the document would grow on every round trip instead of reaching a
/// fixpoint. Import and export both compare through this function.
pub(crate) fn heading_text(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Short alias for [`import_skill_markdown`].
pub fn import_skill(source: &str, markdown: &str) -> Result<GraphDocument, SkillError> {
    import_skill_markdown(source, markdown)
}

/// Short alias for [`export_skill_markdown`].
pub fn export_skill(graph: &GraphDocument) -> Result<String, SkillError> {
    export_skill_markdown(graph)
}

#[cfg(test)]
mod tests;
