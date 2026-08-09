use std::fmt::{Display, Formatter};

use cortex_domain::GraphDocument;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::SequenceError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct TemplateVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl TemplateVersion {
    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl Display for TemplateVersion {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivationHints {
    pub task_classes: &'static [&'static str],
    pub intents: &'static [&'static str],
    pub risks: &'static [&'static str],
    pub mutation: bool,
    pub evidence_classes: &'static [&'static str],
    pub lexical_cues: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub struct SequenceTemplate {
    pub id: &'static str,
    pub version: TemplateVersion,
    pub title: &'static str,
    pub description: &'static str,
    pub markdown: &'static str,
    pub changelog: &'static str,
    pub activation: ActivationHints,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateRef {
    pub id: String,
    pub version: TemplateVersion,
    pub fingerprint: String,
}

#[must_use]
pub const fn templates() -> &'static [SequenceTemplate] {
    &crate::catalog::TEMPLATES
}

/// Create a detached, editable graph from an immutable built-in template.
///
/// # Errors
///
/// Returns [`SequenceError`] when the template is unknown, the requested
/// identity is empty, or the bundled Markdown cannot compile and validate.
pub fn instantiate_template(
    template_id: &str,
    graph_id: &str,
    name: &str,
) -> Result<GraphDocument, SequenceError> {
    if graph_id.trim().is_empty() || name.trim().is_empty() {
        return Err(SequenceError::InvalidCopy(
            "sequence graph id and name must not be empty".to_owned(),
        ));
    }
    let template = templates()
        .iter()
        .find(|candidate| candidate.id == template_id)
        .ok_or_else(|| SequenceError::UnknownTemplate(template_id.to_owned()))?;
    let source = format!("cortex-sequences/templates/{}.md", template.id);
    let mut graph = cortex_skills::import_skill_markdown(&source, template.markdown)?;
    let canonical = cortex_skills::export_skill_markdown(&graph)?;
    let fingerprint = format!("{:x}", Sha256::digest(canonical.as_bytes()));
    graph.id = graph_id.trim().to_owned();
    graph.name = name.trim().to_owned();
    graph.revision = 0;
    graph
        .metadata
        .insert("sequence.templateId".to_owned(), template.id.to_owned());
    graph.metadata.insert(
        "sequence.templateVersion".to_owned(),
        template.version.to_string(),
    );
    graph
        .metadata
        .insert("sequence.templateFingerprint".to_owned(), fingerprint);
    graph
        .metadata
        .insert("sequence.editable".to_owned(), "true".to_owned());
    graph
        .validate()
        .map_err(|error| SequenceError::InvalidCopy(error.to_string()))?;
    Ok(graph)
}
