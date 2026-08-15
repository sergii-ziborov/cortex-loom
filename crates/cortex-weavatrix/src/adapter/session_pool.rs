//! Per-repository Weavatrix sessions. The map lock is only held to look up
//! or insert a slot; two agents on different repos do not block each other.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use weavatrix_rust::Weavatrix;

use super::WeavatrixError;

const MAX_SESSIONS: usize = 8;
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Default)]
pub(super) struct SessionPool {
    inner: Arc<Mutex<HashMap<PathBuf, Slot>>>,
}

struct Slot {
    engine: Arc<Mutex<Weavatrix>>,
    last_used: Instant,
}

impl SessionPool {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn slot(&self, root: &Path) -> Result<Arc<Mutex<Weavatrix>>, WeavatrixError> {
        let mut map = self
            .inner
            .lock()
            .map_err(|_| WeavatrixError::LockPoisoned)?;
        evict_stale(&mut map);
        if let Some(slot) = map.get_mut(root) {
            slot.last_used = Instant::now();
            return Ok(Arc::clone(&slot.engine));
        }
        evict_lru(&mut map);
        let engine = Weavatrix::open(root).map_err(|error| {
            WeavatrixError::Engine(format!("Weavatrix graph build failed: {error}"))
        })?;
        let handle = Arc::new(Mutex::new(engine));
        map.insert(
            root.to_path_buf(),
            Slot {
                engine: Arc::clone(&handle),
                last_used: Instant::now(),
            },
        );
        Ok(handle)
    }
}

fn evict_stale(map: &mut HashMap<PathBuf, Slot>) {
    let now = Instant::now();
    map.retain(|_, slot| now.duration_since(slot.last_used) < SESSION_TTL);
}

fn evict_lru(map: &mut HashMap<PathBuf, Slot>) {
    while map.len() >= MAX_SESSIONS {
        let oldest = map
            .iter()
            .min_by_key(|(_, slot)| slot.last_used)
            .map(|(path, _)| path.clone());
        if let Some(path) = oldest {
            map.remove(&path);
        } else {
            break;
        }
    }
}
