//! Importing a methodology library from a local checkout.
//!
//! The compiler in `cortex-skills` is pure; reading a directory is this
//! module's job. Nothing is copied into this repository — the operator points
//! at a checkout they already have, sees what would be imported together with
//! whatever licence sits beside it, and decides.
//!
//! Preview never writes. Import stores with `seed_if_missing`, so running it
//! twice cannot overwrite a graph the operator has since edited.

use std::fs;
use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use cortex_skills::{LibraryEntry, LibraryNotice, import_library};
use serde::{Deserialize, Serialize};

use crate::{ApiError, AppState};

/// Directories never worth walking for skills.
const SKIPPED_DIRECTORIES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "__pycache__",
];

/// Filesystem entries visited before the walk gives up. A bound, not a
/// quality judgement: a symlink cycle must not hang the server.
const MAX_VISITED: usize = 40_000;
const MAX_DEPTH: usize = 12;
/// Matches the single-document limit on `/api/skills/compile`.
const MAX_SKILL_BYTES: u64 = 2 * 1024 * 1024;
const MAX_NOTICE_BYTES: usize = 16 * 1024;

/// Root-level file name prefixes that carry attribution.
const NOTICE_PREFIXES: &[&str] = &["license", "licence", "notice", "copying", "authors"];

/// Compared against an already-lowercased file name, so the extension check
/// is case-insensitive by construction.
const MARKDOWN_SUFFIX: &str = ".md";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryRequest {
    path: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillPreview {
    source: String,
    id: String,
    name: String,
    node_count: usize,
    edge_count: usize,
    /// Set when a title collision forced a new id.
    #[serde(skip_serializing_if = "Option::is_none")]
    renamed_from: Option<String>,
    /// Only present on import: false when a graph with this id already
    /// existed and was left alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    stored: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkippedPreview {
    source: String,
    reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoticePreview {
    source: String,
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryResponse {
    library: String,
    imported: bool,
    skills: Vec<SkillPreview>,
    skipped: Vec<SkippedPreview>,
    /// Licence and notice files found beside the skills. Read them before
    /// importing: this endpoint reports attribution, it does not clear it.
    notices: Vec<NoticePreview>,
    /// Files visited while walking, so a surprising number is visible.
    visited: usize,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/skills/library/preview", post(preview_library))
        .route("/api/skills/library/import", post(import_library_route))
}

async fn preview_library(
    Json(request): Json<LibraryRequest>,
) -> Result<Json<LibraryResponse>, ApiError> {
    Ok(Json(compile(&request.path, None)?))
}

async fn import_library_route(
    State(state): State<AppState>,
    Json(request): Json<LibraryRequest>,
) -> Result<Json<LibraryResponse>, ApiError> {
    Ok(Json(compile(&request.path, Some(&state))?))
}

/// Read, compile, and — when `state` is given — store the library.
fn compile(path: &Path, state: Option<&AppState>) -> Result<LibraryResponse, ApiError> {
    let root = path.canonicalize().map_err(|error| {
        ApiError::BadRequest(format!("cannot open {}: {error}", path.display()))
    })?;
    if !root.is_dir() {
        return Err(ApiError::BadRequest(format!(
            "{} is not a directory",
            root.display()
        )));
    }
    let library = root.display().to_string();
    let found = discover(&root)?;
    let mut import = import_library(found.entries, found.notices, &library);
    import.skipped.extend(found.unreadable);

    let mut skills = Vec::with_capacity(import.skills.len());
    for skill in import.skills {
        // `seed_if_missing` returns whatever is stored, so "was it new?" has
        // to be asked before writing. An existing graph is left untouched:
        // importing twice must never overwrite an edited workflow.
        let stored = match state {
            None => None,
            Some(state) => {
                let existed = state.store.get(&skill.graph.id)?.is_some();
                state.store.seed_if_missing(&skill.graph)?;
                Some(!existed)
            }
        };
        skills.push(SkillPreview {
            source: skill.source,
            id: skill.graph.id,
            name: skill.graph.name,
            node_count: skill.graph.nodes.len(),
            edge_count: skill.graph.edges.len(),
            renamed_from: skill.renamed_from,
            stored,
        });
    }
    Ok(LibraryResponse {
        library,
        imported: state.is_some(),
        skills,
        skipped: import
            .skipped
            .into_iter()
            .map(|skipped| SkippedPreview {
                source: skipped.source,
                reason: skipped.reason,
            })
            .collect(),
        notices: import
            .notices
            .into_iter()
            .map(|notice| NoticePreview {
                source: notice.source,
                text: notice.text,
            })
            .collect(),
        visited: found.visited,
    })
}

struct Discovered {
    entries: Vec<LibraryEntry>,
    notices: Vec<LibraryNotice>,
    unreadable: Vec<cortex_skills::library::SkippedDocument>,
    visited: usize,
}

/// Collect `SKILL.md` documents at any depth.
///
/// A library that uses a flat layout — Markdown files directly in the root —
/// is accepted too, but only when no `SKILL.md` was found anywhere. Mixing
/// the two rules would pull in every README that happened to sit beside a
/// skill.
fn discover(root: &Path) -> Result<Discovered, ApiError> {
    let mut found = Discovered {
        entries: Vec::new(),
        notices: Vec::new(),
        unreadable: Vec::new(),
        visited: 0,
    };
    let mut flat: Vec<PathBuf> = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0_usize)];
    while let Some((directory, depth)) = stack.pop() {
        let listing = fs::read_dir(&directory)
            .map_err(|error| ApiError::BadRequest(format!("cannot read a directory: {error}")))?;
        for entry in listing {
            let entry =
                entry.map_err(|error| ApiError::BadRequest(format!("cannot read: {error}")))?;
            found.visited += 1;
            if found.visited > MAX_VISITED {
                return Err(ApiError::BadRequest(format!(
                    "the tree under {} exceeds {MAX_VISITED} entries",
                    root.display()
                )));
            }
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                if depth < MAX_DEPTH && !is_skipped(&path) {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            classify(root, &path, &metadata, &mut found, &mut flat);
        }
    }
    if found.entries.is_empty() {
        for path in flat {
            read_skill(root, &path, &mut found);
        }
    }
    found
        .entries
        .sort_by(|left, right| left.source.cmp(&right.source));
    found
        .notices
        .sort_by(|left, right| left.source.cmp(&right.source));
    found
        .unreadable
        .sort_by(|left, right| left.source.cmp(&right.source));
    Ok(found)
}

fn classify(
    root: &Path,
    path: &Path,
    metadata: &fs::Metadata,
    found: &mut Discovered,
    flat: &mut Vec<PathBuf>,
) {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let lowercase = name.to_ascii_lowercase();
    if lowercase == "skill.md" {
        if metadata.len() > MAX_SKILL_BYTES {
            found
                .unreadable
                .push(cortex_skills::library::SkippedDocument {
                    source: relative(root, path),
                    reason: format!("larger than {MAX_SKILL_BYTES} bytes"),
                });
            return;
        }
        read_skill(root, path, found);
        return;
    }
    let in_root = path.parent() == Some(root);
    if in_root
        && NOTICE_PREFIXES
            .iter()
            .any(|prefix| lowercase.starts_with(prefix))
    {
        if let Ok(text) = fs::read_to_string(path) {
            let mut text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned();
            text.truncate(
                text.char_indices()
                    .nth(MAX_NOTICE_BYTES)
                    .map_or(text.len(), |(at, _)| at),
            );
            found.notices.push(LibraryNotice {
                source: relative(root, path),
                text,
            });
        }
        return;
    }
    if in_root && lowercase.ends_with(MARKDOWN_SUFFIX) && metadata.len() <= MAX_SKILL_BYTES {
        flat.push(path.to_path_buf());
    }
}

fn read_skill(root: &Path, path: &Path, found: &mut Discovered) {
    let source = relative(root, path);
    match fs::read_to_string(path) {
        Ok(markdown) => found.entries.push(LibraryEntry { source, markdown }),
        Err(error) => found
            .unreadable
            .push(cortex_skills::library::SkippedDocument {
                source,
                reason: error.to_string(),
            }),
    }
}

fn is_skipped(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with('.') && name != "." || SKIPPED_DIRECTORIES.contains(&name)
        })
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{MAX_DEPTH, MAX_SKILL_BYTES, NOTICE_PREFIXES, is_skipped};
    use std::path::Path;

    #[test]
    fn hidden_and_build_directories_are_never_walked() {
        for name in [".git", "node_modules", "target", ".venv", ".idea"] {
            assert!(
                is_skipped(Path::new("/root").join(name).as_path()),
                "{name}"
            );
        }
        for name in ["skills", "docs", "plugin-skills"] {
            assert!(
                !is_skipped(Path::new("/root").join(name).as_path()),
                "{name} should be walked"
            );
        }
    }

    #[test]
    fn the_walk_is_bounded_in_every_dimension() {
        // Regression guard: these bounds are the only thing standing between
        // a user-supplied path and an unbounded read.
        const { assert!(MAX_DEPTH <= 32) };
        const { assert!(MAX_SKILL_BYTES <= 4 * 1024 * 1024) };
        assert!(NOTICE_PREFIXES.contains(&"license"));
        assert!(NOTICE_PREFIXES.contains(&"notice"));
    }
}
