//! Repository revision identity for evidence packets.

use std::path::Path;
use std::process::Command;
use std::{fs, io::Read};

use sha2::{Digest, Sha256};

/// `git:<commit>+dirty:<digest>`. Clean trees use `dirty:0`.
#[must_use]
pub fn repository_snapshot(root: &Path) -> String {
    let commit = git(root, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    let status = git_bytes(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .unwrap_or_default();
    let digest = if status.is_empty() {
        "0".to_owned()
    } else {
        let mut hasher = Sha256::new();
        hasher.update(&status);
        // Status records paths and staged state, but a second edit to the
        // same dirty file leaves those bytes unchanged. Include the actual
        // tracked diff and untracked file bytes in the snapshot identity.
        if let Some(diff) = git_bytes(root, &["diff", "--binary", "HEAD", "--"]) {
            hasher.update(diff);
        }
        if let Some(untracked) =
            git_bytes(root, &["ls-files", "--others", "--exclude-standard", "-z"])
        {
            for raw_path in untracked
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
            {
                hasher.update(raw_path);
                if let Ok(path) = std::str::from_utf8(raw_path) {
                    hash_path(&root.join(path), &mut hasher);
                }
            }
        }
        hex::encode(hasher.finalize()).chars().take(12).collect()
    };
    format!("git:{commit}+dirty:{digest}")
}

fn hash_path(path: &Path, hasher: &mut Sha256) {
    if let Ok(target) = fs::read_link(path) {
        hasher.update(target.to_string_lossy().as_bytes());
        return;
    }
    let Ok(mut file) = fs::File::open(path) else {
        hasher.update(b"missing-or-unreadable");
        return;
    };
    let mut buffer = [0_u8; 8192];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => hasher.update(&buffer[..count]),
            Err(_) => {
                hasher.update(b"read-error");
                break;
            }
        }
    }
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    String::from_utf8(git_bytes(root, args)?)
        .ok()
        .map(|text| text.trim().to_owned())
}

fn git_bytes(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::repository_snapshot;
    use std::{fs, path::Path, process::Command};

    #[test]
    fn snapshot_names_git_head() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let snapshot = repository_snapshot(&root);
        assert!(
            snapshot.starts_with("git:") && snapshot.contains("+dirty:"),
            "{snapshot}"
        );
    }

    #[test]
    fn second_edit_to_dirty_and_untracked_files_changes_snapshot() {
        let root = std::env::temp_dir().join(format!("cortex-snapshot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(
            Command::new("git")
                .arg("init")
                .arg("-q")
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("code.rs"), "fn one() {}\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "code.rs"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "commit",
                    "-qm",
                    "base"
                ])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("code.rs"), "fn two() {}\n").unwrap();
        let first = repository_snapshot(&root);
        fs::write(root.join("code.rs"), "fn three() {}\n").unwrap();
        assert_ne!(first, repository_snapshot(&root));
        fs::write(root.join("new.rs"), "alpha\n").unwrap();
        let untracked = repository_snapshot(&root);
        fs::write(root.join("new.rs"), "beta\n").unwrap();
        assert_ne!(untracked, repository_snapshot(&root));
        fs::remove_dir_all(root).unwrap();
    }
}
