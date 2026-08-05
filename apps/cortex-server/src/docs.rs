//! In-app documentation.
//!
//! The design documents are baked into the binary next to the UI assets, so a
//! running instance explains itself with no network, no repository checkout,
//! and no second tool. The Markdown is served verbatim; rendering is the
//! client's job, which keeps this endpoint incapable of executing anything.

use axum::extract::Path as AxumPath;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::{ApiError, AppState};

/// One bundled document.
struct BundledDoc {
    /// Stable url segment, matching the file stem.
    id: &'static str,
    title: &'static str,
    /// One line describing when to read it.
    summary: &'static str,
    markdown: &'static str,
}

/// Reading order, not alphabetical order: a newcomer should be able to read
/// the list top to bottom.
static DOCS: &[BundledDoc] = &[
    BundledDoc {
        id: "architecture",
        title: "Architecture",
        summary: "Runtime flow, graph layers, modules, and every safety boundary.",
        markdown: include_str!("../../../docs/architecture.md"),
    },
    BundledDoc {
        id: "research",
        title: "Research",
        summary: "The claims this project is built on and how far each is proven.",
        markdown: include_str!("../../../docs/research.md"),
    },
    BundledDoc {
        id: "evaluation",
        title: "Evaluation gates",
        summary: "What a local model profile must pass before it is allowed to decide anything.",
        markdown: include_str!("../../../docs/evaluation.md"),
    },
    BundledDoc {
        id: "benchmark",
        title: "Context benchmark",
        summary: "Measured tokens and required-fact recall across three evidence-assembly arms.",
        markdown: include_str!("../../../docs/benchmark.md"),
    },
    BundledDoc {
        id: "competitors",
        title: "Competitive landscape",
        summary: "Where this project is differentiated, and where it is behind.",
        markdown: include_str!("../../../docs/competitors.md"),
    },
    BundledDoc {
        id: "local-models",
        title: "Local models",
        summary: "Which device runs which job, and what the code refuses to claim about it.",
        markdown: include_str!("../../../docs/local-models.md"),
    },
    BundledDoc {
        id: "shadow-mode",
        title: "Shadow mode",
        summary: "Observing local profiles on real traffic without letting them influence it.",
        markdown: include_str!("../../../docs/shadow-mode.md"),
    },
    BundledDoc {
        id: "rust-ui",
        title: "Rust UI assessment",
        summary: "Whether the editor should be rewritten in Rust, and what it would cost.",
        markdown: include_str!("../../../docs/rust-ui.md"),
    },
    BundledDoc {
        id: "roadmap",
        title: "Roadmap",
        summary: "What is done, what is next, and which gates remain open.",
        markdown: include_str!("../../../docs/roadmap.md"),
    },
    BundledDoc {
        id: "publishing",
        title: "Publishing",
        summary: "Which crates are public, under which licence, and in what order.",
        markdown: include_str!("../../../docs/publishing.md"),
    },
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocSummary {
    id: &'static str,
    title: &'static str,
    summary: &'static str,
    /// Characters, so a client can show a length without fetching the body.
    length: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocBody {
    id: &'static str,
    title: &'static str,
    markdown: &'static str,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/docs", get(list_docs))
        .route("/api/docs/{id}", get(read_doc))
}

async fn list_docs() -> Json<Vec<DocSummary>> {
    Json(
        DOCS.iter()
            .map(|doc| DocSummary {
                id: doc.id,
                title: doc.title,
                summary: doc.summary,
                length: doc.markdown.chars().count(),
            })
            .collect(),
    )
}

/// Lookup is an exact match against the baked list, so no path can escape it.
async fn read_doc(AxumPath(id): AxumPath<String>) -> Result<Json<DocBody>, ApiError> {
    DOCS.iter()
        .find(|doc| doc.id == id)
        .map(|doc| {
            Json(DocBody {
                id: doc.id,
                title: doc.title,
                markdown: doc.markdown,
            })
        })
        .ok_or_else(|| ApiError::NotFound(format!("document not found: {id}")))
}

#[cfg(test)]
mod tests {
    use super::DOCS;

    #[test]
    fn every_bundled_document_is_addressable_and_non_empty() {
        let mut ids = std::collections::HashSet::new();
        for doc in DOCS {
            assert!(ids.insert(doc.id), "duplicate document id {}", doc.id);
            assert!(
                doc.id
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character == '-'),
                "{} is not url-safe",
                doc.id
            );
            assert!(!doc.markdown.trim().is_empty(), "{} is empty", doc.id);
            assert!(!doc.summary.trim().is_empty(), "{} has no summary", doc.id);
        }
    }
}
