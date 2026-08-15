//! Repository revision identity for evidence packets.

use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

/// `git:<commit>+dirty:<digest>`. Clean trees use `dirty:0`.
#[must_use]
pub fn repository_snapshot(root: &Path) -> String {
    let commit = git(root, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    let dirty = git(root, &["status", "--porcelain"]).unwrap_or_default();
    let digest = if dirty.trim().is_empty() {
        "0".to_owned()
    } else {
        let mut hasher = Sha256::new();
        hasher.update(dirty.as_bytes());
        format!("{:x}", hasher.finalize())
            .chars()
            .take(12)
            .collect()
    };
    format!("git:{commit}+dirty:{digest}")
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::repository_snapshot;
    use std::path::Path;

    #[test]
    fn snapshot_names_git_head() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let snapshot = repository_snapshot(&root);
        assert!(
            snapshot.starts_with("git:") && snapshot.contains("+dirty:"),
            "{snapshot}"
        );
    }
}
