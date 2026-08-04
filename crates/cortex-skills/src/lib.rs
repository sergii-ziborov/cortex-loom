//! Compiler between human-readable `SKILL.md` workflows and Cortex graphs.

mod export;
mod import;

use std::fmt::{Display, Formatter};

use cortex_domain::{GraphDocument, GraphError};

pub use export::export_skill_markdown;
pub use import::import_skill_markdown;

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
