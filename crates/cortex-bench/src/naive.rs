//! The no-intelligence arm: read the candidate files whole.
//!
//! This is what an agent does when it greps for a keyword and opens every
//! file that matched. The fixture supplies the directories, which makes the
//! arm cheaper than reality — the search that finds them is not charged here.

use std::fs;
use std::path::{Path, PathBuf};

/// Directories never worth reading as source evidence.
const SKIPPED_DIRECTORIES: &[&str] = &[
    ".git",
    ".cortex-loom",
    "node_modules",
    "target",
    "dist",
    ".vite",
];

/// Files above this size are recorded as skipped rather than read, so one
/// lockfile cannot dominate a measurement.
pub const MAX_FILE_BYTES: u64 = 512 * 1024;

/// The files a naive sweep would have open, in stable path order.
#[derive(Debug, Clone, Default)]
pub struct NaiveScan {
    /// Repository-relative path and full contents, sorted by path.
    pub files: Vec<(String, String)>,
    /// Paths that matched but were not read, with the reason.
    pub skipped: Vec<String>,
}

impl NaiveScan {
    /// Everything the arm would paste into the conversation.
    #[must_use]
    pub fn context(&self) -> String {
        let mut context = String::new();
        for (path, contents) in &self.files {
            context.push_str("// ");
            context.push_str(path);
            context.push('\n');
            context.push_str(contents);
            context.push_str("\n\n");
        }
        context
    }
}

/// Read every file under `root` whose repository-relative path matches one of
/// `patterns`.
///
/// # Errors
///
/// Returns the first filesystem error that prevents walking `root`.
pub fn scan(root: &Path, patterns: &[&str]) -> std::io::Result<NaiveScan> {
    let mut scan = NaiveScan::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let Some(relative) = relative_path(root, &path) else {
                continue;
            };
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                if !is_skipped_directory(&path) {
                    stack.push(path);
                }
                continue;
            }
            if !metadata.is_file() || !patterns.iter().any(|pattern| matches(pattern, &relative)) {
                continue;
            }
            if metadata.len() > MAX_FILE_BYTES {
                scan.skipped
                    .push(format!("{relative} (over {MAX_FILE_BYTES} bytes)"));
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(contents) => scan.files.push((relative, contents)),
                Err(error) => scan.skipped.push(format!("{relative} ({error})")),
            }
        }
    }
    scan.files.sort_by(|left, right| left.0.cmp(&right.0));
    scan.skipped.sort();
    Ok(scan)
}

/// Append `git log` so a history fixture has the same facts an engineer
/// would paste after `git log -- path`.
///
/// # Errors
///
/// Returns when `git` cannot be started. A non-zero git exit is recorded
/// as a skipped path rather than a hard failure.
pub fn append_git_log(scan: &mut NaiveScan, root: &Path, paths: &[&str]) -> std::io::Result<()> {
    let mut command = std::process::Command::new("git");
    command
        .current_dir(root)
        .args(["log", "-20", "--format=%h %an %s", "--"]);
    for path in paths {
        command.arg(path);
    }
    let output = command.output()?;
    if !output.status.success() {
        scan.skipped.push(format!("git log ({})", output.status));
        return Ok(());
    }
    let body = String::from_utf8_lossy(&output.stdout).into_owned();
    if !body.trim().is_empty() {
        scan.files.push(("git-log".to_owned(), body));
        scan.files.sort_by(|left, right| left.0.cmp(&right.0));
    }
    Ok(())
}

fn is_skipped_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| SKIPPED_DIRECTORIES.contains(&name))
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative: PathBuf = path.strip_prefix(root).ok()?.to_path_buf();
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// Match a repository-relative path against a `/`-separated glob.
///
/// `*` matches within one segment, `**` matches any number of segments.
/// Deliberately small: the fixtures only need directory sweeps, and a full
/// glob dependency would be more surface than the benchmark can justify.
#[must_use]
pub fn matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    match_segments(&pattern, &path)
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| match_segments(rest, &path[skip..])),
        Some((head, rest)) => match path.split_first() {
            Some((segment, tail)) if match_segment(head, segment) => match_segments(rest, tail),
            _ => false,
        },
    }
}

fn match_segment(pattern: &str, segment: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let Some((first, rest)) = parts.split_first() else {
        return pattern == segment;
    };
    let Some((last, middle)) = rest.split_last() else {
        // No `*` in the pattern at all: an exact segment.
        return pattern == segment;
    };
    let Some(mut remaining) = segment.strip_prefix(first) else {
        return false;
    };
    for part in middle {
        let Some(position) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[position + part.len()..];
    }
    last.is_empty() || remaining.ends_with(last)
}
