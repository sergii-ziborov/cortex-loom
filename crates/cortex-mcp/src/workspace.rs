//! Workspace allowlist. Required for a non-loopback bind.

use std::path::{Path, PathBuf};

/// Roots a remote (or explicitly restricted) host may open.
#[derive(Debug, Clone, Default)]
pub struct WorkspacePolicy {
    roots: Vec<PathBuf>,
    require_allowlist: bool,
}

impl WorkspacePolicy {
    /// Build from `--workspace` paths plus `CORTEX_WORKSPACE_ALLOWLIST`.
    ///
    /// # Errors
    ///
    /// Remote bind with an empty allowlist.
    pub fn new(remote: bool, extra: Vec<PathBuf>) -> Result<Self, String> {
        let mut roots = extra;
        if let Ok(raw) = std::env::var("CORTEX_WORKSPACE_ALLOWLIST") {
            for part in raw.split([';', '|']) {
                let trimmed = part.trim();
                if !trimmed.is_empty() {
                    roots.push(PathBuf::from(trimmed));
                }
            }
        }
        if remote && roots.is_empty() {
            return Err(
                "remote bind requires --workspace PATH or CORTEX_WORKSPACE_ALLOWLIST".to_owned(),
            );
        }
        Ok(Self {
            roots,
            require_allowlist: remote,
        })
    }

    /// Open any local path. Used by stdio and loopback.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self::default()
    }

    /// Refuse a repository outside the allowlist when one is configured.
    pub fn check(&self, repository: &Path) -> Result<(), String> {
        if self.roots.is_empty() && !self.require_allowlist {
            return Ok(());
        }
        let candidate = canonicalize_or_parent(repository);
        for root in &self.roots {
            let allowed = canonicalize_or_parent(root);
            if candidate.starts_with(&allowed) {
                return Ok(());
            }
        }
        Err(format!(
            "repository {} is outside the workspace allowlist",
            repository.display()
        ))
    }
}

fn canonicalize_or_parent(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    if let Some(parent) = path.parent()
        && let Ok(canonical) = parent.canonicalize()
    {
        return canonical.join(path.file_name().unwrap_or_default());
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::WorkspacePolicy;
    use std::path::PathBuf;

    #[test]
    fn remote_without_roots_is_refused() {
        // Isolate from a developer-set allowlist env var.
        let result = WorkspacePolicy {
            roots: Vec::new(),
            require_allowlist: true,
        }
        .check(&PathBuf::from("/tmp/somewhere"));
        assert!(result.is_err());
    }

    #[test]
    fn loopback_without_roots_is_open() {
        let policy = WorkspacePolicy::unrestricted();
        assert!(policy.check(&PathBuf::from("/tmp/somewhere")).is_ok());
    }
}
