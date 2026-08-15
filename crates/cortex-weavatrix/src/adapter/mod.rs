mod cleanup;
mod evidence;
mod expand;
mod gather;
mod locator;
mod render;
mod retry;
mod source_reads;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use weavatrix_rust::Weavatrix;

pub use evidence::{EvidenceBundle, EvidenceFragment, EvidenceKind};
pub(crate) use render::SEARCH_HEADER;

#[derive(Debug, Clone, Copy, Default)]
pub struct WeavatrixConfig;

#[derive(Debug)]
pub enum WeavatrixError {
    InvalidArguments(String),
    Engine(String),
    LockPoisoned,
}

impl Display for WeavatrixError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArguments(message) | Self::Engine(message) => formatter.write_str(message),
            Self::LockPoisoned => formatter.write_str("Weavatrix session lock was poisoned"),
        }
    }
}

impl std::error::Error for WeavatrixError {}

#[derive(Clone)]
pub struct WeavatrixAdapter {
    engines: Arc<Mutex<HashMap<PathBuf, Weavatrix>>>,
}

impl WeavatrixConfig {
    /// Discover the native in-process configuration.
    ///
    /// # Errors
    ///
    /// Kept fallible for API compatibility; native discovery currently has no
    /// external executable or script that can be missing.
    pub const fn discover() -> Result<Self, WeavatrixError> {
        Ok(Self)
    }
}

impl WeavatrixAdapter {
    #[must_use]
    pub fn new(_config: WeavatrixConfig) -> Self {
        Self {
            engines: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Validate and render an upstream-authored exact plan in memory.
    ///
    /// # Errors
    ///
    /// Returns [`WeavatrixError`] when the plan is malformed, stale, unsafe,
    /// or cannot be previewed under the native resource limits.
    pub fn preview_refactor(
        &self,
        repository: &Path,
        plan: &Value,
    ) -> Result<crate::RefactorPreview, WeavatrixError> {
        let _ = self;
        let encoded = serde_json::to_vec(plan).map_err(|error| {
            WeavatrixError::InvalidArguments(format!("cannot encode refactor plan: {error}"))
        })?;
        crate::preview_refactor_plan(repository, &encoded)
    }

    fn canonical_root(&self, repository: &Path) -> Result<PathBuf, WeavatrixError> {
        let _ = self;
        repository.canonicalize().map_err(|error| {
            WeavatrixError::Engine(format!("cannot open {}: {error}", repository.display()))
        })
    }

    fn session<'a>(
        sessions: &'a mut HashMap<PathBuf, Weavatrix>,
        root: &Path,
    ) -> Result<&'a mut Weavatrix, WeavatrixError> {
        if !sessions.contains_key(root) {
            let engine = Weavatrix::open(root).map_err(|error| {
                WeavatrixError::Engine(format!("Weavatrix graph build failed: {error}"))
            })?;
            sessions.insert(root.to_path_buf(), engine);
        }
        sessions.get_mut(root).ok_or_else(|| {
            WeavatrixError::Engine("native Weavatrix session was not retained".to_owned())
        })
    }
}
