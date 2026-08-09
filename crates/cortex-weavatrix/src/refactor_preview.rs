use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use weavatrix_refactor_plan::{
    RefactorOperation, RefactorPlanLimits, fingerprint_plan, parse_refactor_plan,
    validate_consumer_plan,
};

use crate::WeavatrixError;

const MAX_RETAINED_BODY_BYTES: usize = 64 * 1024;
const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefactorPreview {
    pub schema_version: String,
    pub fingerprint: String,
    pub operation: String,
    pub completeness: String,
    pub affected_paths: Vec<String>,
    pub changes: Vec<PreviewChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChange {
    pub kind: String,
    pub path: String,
    pub destination: Option<String>,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// Validate and render an exact upstream-authored plan without touching disk.
///
/// # Errors
///
/// Returns [`WeavatrixError`] for malformed plans, unsafe paths, stale hashes,
/// non-UTF-8 sources, or edit batches that exceed the native hard limits.
pub fn preview_refactor_plan(
    repository: &Path,
    raw_plan: &[u8],
) -> Result<RefactorPreview, WeavatrixError> {
    let limits = RefactorPlanLimits::default();
    let plan = parse_refactor_plan(raw_plan, limits).map_err(plan_error)?;
    let validated = validate_consumer_plan(&plan, limits).map_err(plan_error)?;
    let fingerprint = fingerprint_plan(validated.plan()).map_err(plan_error)?;
    let root = repository.canonicalize().map_err(|error| {
        WeavatrixError::Engine(format!(
            "cannot open repository {}: {error}",
            repository.display()
        ))
    })?;
    let mut affected_paths = Vec::new();
    let mut changes = Vec::with_capacity(plan.operations.len());
    let mut warnings = Vec::new();

    for operation in &plan.operations {
        match operation {
            RefactorOperation::Modify(file) => {
                let source = read_existing(&root, &file.path)?;
                verify_hash(&file.path, &source, &file.sha256)?;
                let prepared = weavatrix_edit::prepare_edits_with_limits(
                    &source,
                    &file.edits,
                    weavatrix_edit::ApplyLimits::default(),
                )
                .map_err(edit_error)?;
                let after = prepared.apply().text;
                push_unique(&mut affected_paths, &file.path);
                changes.push(change_with_bodies(
                    "modify",
                    &file.path,
                    None,
                    Some(&source),
                    Some(&after),
                    &mut warnings,
                ));
            }
            RefactorOperation::Create(file) => {
                resolve_missing(&root, &file.path)?;
                push_unique(&mut affected_paths, &file.path);
                changes.push(change_with_bodies(
                    "create",
                    &file.path,
                    None,
                    None,
                    Some(&file.contents),
                    &mut warnings,
                ));
            }
            RefactorOperation::Delete(file) => {
                let source = read_existing(&root, &file.path)?;
                verify_hash(&file.path, &source, &file.expected_sha256)?;
                push_unique(&mut affected_paths, &file.path);
                changes.push(change_with_bodies(
                    "delete",
                    &file.path,
                    None,
                    Some(&source),
                    None,
                    &mut warnings,
                ));
            }
            RefactorOperation::Rename(file) => {
                let source = read_existing(&root, &file.from)?;
                verify_hash(&file.from, &source, &file.expected_source_sha256)?;
                resolve_missing(&root, &file.to)?;
                let prepared = weavatrix_edit::prepare_edits_with_limits(
                    &source,
                    &file.edits,
                    weavatrix_edit::ApplyLimits::default(),
                )
                .map_err(edit_error)?;
                let after = prepared.apply().text;
                push_unique(&mut affected_paths, &file.from);
                push_unique(&mut affected_paths, &file.to);
                changes.push(change_with_bodies(
                    "rename",
                    &file.from,
                    Some(&file.to),
                    Some(&source),
                    Some(&after),
                    &mut warnings,
                ));
            }
        }
    }

    Ok(RefactorPreview {
        schema_version: "cortex.refactor-preview.v1".to_owned(),
        fingerprint: fingerprint.to_string(),
        operation: plan.operation,
        completeness: plan
            .completeness
            .as_ref()
            .map_or("UNSPECIFIED", |value| value.as_str())
            .to_owned(),
        affected_paths,
        changes,
        warnings,
    })
}

fn change_with_bodies(
    kind: &str,
    path: &str,
    destination: Option<&str>,
    before: Option<&str>,
    after: Option<&str>,
    warnings: &mut Vec<String>,
) -> PreviewChange {
    PreviewChange {
        kind: kind.to_owned(),
        path: path.to_owned(),
        destination: destination.map(str::to_owned),
        before_sha256: before.map(hash_text),
        after_sha256: after.map(hash_text),
        before: before.map(|body| retain_body(path, "before", body, warnings)),
        after: after.map(|body| retain_body(path, "after", body, warnings)),
    }
}

fn read_existing(root: &Path, portable: &str) -> Result<String, WeavatrixError> {
    let path = resolve_existing(root, portable)?;
    let metadata = std::fs::metadata(&path).map_err(|error| io_error(&path, &error))?;
    if !metadata.is_file() {
        return Err(WeavatrixError::InvalidArguments(format!(
            "refactor source is not a file: {portable}"
        )));
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(WeavatrixError::InvalidArguments(format!(
            "refactor source exceeds the {MAX_SOURCE_BYTES}-byte preview limit: {portable}"
        )));
    }
    std::fs::read_to_string(&path).map_err(|error| io_error(&path, &error))
}

fn resolve_existing(root: &Path, portable: &str) -> Result<PathBuf, WeavatrixError> {
    let candidate = root.join(portable);
    let resolved = candidate
        .canonicalize()
        .map_err(|error| io_error(&candidate, &error))?;
    ensure_contained(root, &resolved, portable)?;
    Ok(resolved)
}

fn resolve_missing(root: &Path, portable: &str) -> Result<PathBuf, WeavatrixError> {
    let candidate = root.join(portable);
    if std::fs::symlink_metadata(&candidate).is_ok() {
        return Err(WeavatrixError::InvalidArguments(format!(
            "refactor destination already exists: {portable}"
        )));
    }
    let parent = candidate.parent().ok_or_else(|| {
        WeavatrixError::InvalidArguments(format!("refactor destination has no parent: {portable}"))
    })?;
    let resolved_parent = parent
        .canonicalize()
        .map_err(|error| io_error(parent, &error))?;
    ensure_contained(root, &resolved_parent, portable)?;
    let name = candidate.file_name().ok_or_else(|| {
        WeavatrixError::InvalidArguments(format!(
            "refactor destination has no file name: {portable}"
        ))
    })?;
    Ok(resolved_parent.join(name))
}

fn ensure_contained(root: &Path, path: &Path, portable: &str) -> Result<(), WeavatrixError> {
    if path == root || !path.starts_with(root) {
        return Err(WeavatrixError::InvalidArguments(format!(
            "refactor path escapes the repository: {portable}"
        )));
    }
    Ok(())
}

fn verify_hash(path: &str, source: &str, expected: &str) -> Result<(), WeavatrixError> {
    let actual = hash_text(source);
    if actual != expected {
        return Err(WeavatrixError::InvalidArguments(format!(
            "stale refactor source hash for {path}: expected {expected}, found {actual}"
        )));
    }
    Ok(())
}

fn hash_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn retain_body(path: &str, label: &str, body: &str, warnings: &mut Vec<String>) -> String {
    if body.len() <= MAX_RETAINED_BODY_BYTES {
        return body.to_owned();
    }
    let mut end = MAX_RETAINED_BODY_BYTES;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    warnings.push(format!(
        "{label} body for {path} was truncated to {end} UTF-8 bytes"
    ));
    body[..end].to_owned()
}

fn push_unique(paths: &mut Vec<String>, path: &str) {
    if !paths.iter().any(|existing| existing == path) {
        paths.push(path.to_owned());
    }
}

fn plan_error(error: weavatrix_refactor_plan::PlanError) -> WeavatrixError {
    WeavatrixError::InvalidArguments(format!("invalid refactor plan: {error}"))
}

fn edit_error(error: weavatrix_edit::EditError) -> WeavatrixError {
    WeavatrixError::InvalidArguments(format!("invalid refactor edit: {error}"))
}

fn io_error(path: &Path, error: &std::io::Error) -> WeavatrixError {
    WeavatrixError::InvalidArguments(format!("cannot read {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use sha2::{Digest, Sha256};
    use weavatrix_refactor_plan::{
        CreateFile, DeleteFile, FileEdit, Position, Provenance, RefactorOperation, RefactorPlan,
        RenameFile, TextEdit, TextRange,
    };

    use super::preview_refactor_plan;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestRepo(PathBuf);

    impl TestRepo {
        fn new() -> Self {
            let suffix = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "cortex-weavatrix-preview-{}-{suffix}",
                std::process::id()
            ));
            std::fs::create_dir_all(path.join("src")).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, path: &str, contents: &str) {
            std::fs::write(self.0.join(path), contents).unwrap();
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let expected_prefix = std::env::temp_dir().join("cortex-weavatrix-preview-");
            assert!(self.0.parent() == expected_prefix.parent());
            assert!(self.0.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("cortex-weavatrix-preview-")
            }));
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn sha256(contents: &str) -> String {
        format!("{:x}", Sha256::digest(contents.as_bytes()))
    }

    fn encode(operation: RefactorOperation) -> Vec<u8> {
        serde_json::to_vec(&RefactorPlan::new("test", vec![operation])).unwrap()
    }

    #[test]
    fn exact_modify_is_previewed_without_writing() {
        let repo = TestRepo::new();
        let before = "fn old() {}\n";
        repo.write("src/lib.rs", before);
        let edit = TextEdit::replace(
            TextRange::new(Position::new(1, 3), Position::new(1, 6)),
            "old",
            "new",
            Provenance::EXACT_LSP,
        );
        let plan = encode(RefactorOperation::Modify(FileEdit::new(
            "src/lib.rs",
            sha256(before),
            vec![edit],
        )));

        let preview = preview_refactor_plan(repo.path(), &plan).unwrap();

        assert_eq!(preview.operation, "test");
        assert_eq!(preview.affected_paths, ["src/lib.rs"]);
        assert_eq!(preview.changes[0].before.as_deref(), Some(before));
        assert_eq!(preview.changes[0].after.as_deref(), Some("fn new() {}\n"));
        assert_eq!(
            std::fs::read_to_string(repo.path().join("src/lib.rs")).unwrap(),
            before
        );
    }

    #[test]
    fn stale_modify_hash_is_rejected() {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "fn old() {}\n");
        let plan = encode(RefactorOperation::Modify(FileEdit::new(
            "src/lib.rs",
            "0".repeat(64),
            Vec::new(),
        )));

        assert!(preview_refactor_plan(repo.path(), &plan).is_err());
    }

    #[test]
    fn path_escape_is_rejected() {
        let repo = TestRepo::new();
        let plan = encode(RefactorOperation::Create(CreateFile::new(
            "../escape.rs",
            "escaped",
        )));

        assert!(preview_refactor_plan(repo.path(), &plan).is_err());
    }

    #[test]
    fn create_at_existing_path_is_rejected() {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "existing");
        let plan = encode(RefactorOperation::Create(CreateFile::new(
            "src/lib.rs",
            "replacement",
        )));

        assert!(preview_refactor_plan(repo.path(), &plan).is_err());
    }

    #[test]
    fn delete_retains_the_exact_before_body() {
        let repo = TestRepo::new();
        let before = "obsolete\n";
        repo.write("src/old.rs", before);
        let plan = encode(RefactorOperation::Delete(DeleteFile {
            path: "src/old.rs".to_owned(),
            expected_sha256: sha256(before),
            extensions: BTreeMap::new(),
        }));

        let preview = preview_refactor_plan(repo.path(), &plan).unwrap();

        assert_eq!(preview.changes[0].kind, "delete");
        assert_eq!(preview.changes[0].before.as_deref(), Some(before));
        assert!(preview.changes[0].after.is_none());
        assert!(repo.path().join("src/old.rs").exists());
    }

    #[test]
    fn rename_previews_destination_and_optional_edits_without_writing() {
        let repo = TestRepo::new();
        let before = "pub fn old() {}\n";
        repo.write("src/old.rs", before);
        let edit = TextEdit::replace(
            TextRange::new(Position::new(1, 7), Position::new(1, 10)),
            "old",
            "new",
            Provenance::EXACT_LSP,
        );
        let plan = encode(RefactorOperation::Rename(
            RenameFile::new("src/old.rs", "src/new.rs", sha256(before)).with_edits(vec![edit]),
        ));

        let preview = preview_refactor_plan(repo.path(), &plan).unwrap();

        assert_eq!(preview.changes[0].kind, "rename");
        assert_eq!(
            preview.changes[0].destination.as_deref(),
            Some("src/new.rs")
        );
        assert_eq!(
            preview.changes[0].after.as_deref(),
            Some("pub fn new() {}\n")
        );
        assert!(repo.path().join("src/old.rs").exists());
        assert!(!repo.path().join("src/new.rs").exists());
    }
}
