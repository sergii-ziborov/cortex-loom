//! Dynamic language inventory for one repository revision.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use crate::fold::SOURCE_SUFFIXES;
use crate::repository_snapshot;

const MAX_WALK_FILES: usize = 4_000;
const MAX_CACHE: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageInventory {
    pub snapshot: String,
    pub by_suffix: BTreeMap<String, u32>,
}

impl LanguageInventory {
    /// Search glob covering suffixes that actually exist here.
    #[must_use]
    pub fn glob(&self) -> String {
        let mut suffixes: Vec<&str> = self
            .by_suffix
            .keys()
            .map(String::as_str)
            .filter(|suffix| SOURCE_SUFFIXES.contains(suffix))
            .collect();
        if suffixes.is_empty() {
            return crate::fold::DEFAULT_SOURCE_GLOB.to_owned();
        }
        suffixes.sort_unstable();
        if suffixes.len() == 1 {
            return format!("**/*{}", suffixes[0]);
        }
        let inner = suffixes
            .iter()
            .map(|suffix| suffix.trim_start_matches('.'))
            .collect::<Vec<_>>()
            .join(",");
        format!("**/*.{{{inner}}}")
    }
}

#[derive(Default)]
struct Cache {
    slots: BTreeMap<PathBuf, (Instant, LanguageInventory)>,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache {
    slots: BTreeMap::new(),
});

/// Inventory for `root` at its current snapshot. Cached per revision.
#[must_use]
pub fn inventory(root: &Path) -> LanguageInventory {
    let snapshot = repository_snapshot(root);
    if let Ok(cache) = CACHE.lock()
        && let Some((_, stored)) = cache.slots.get(root)
        && stored.snapshot == snapshot
    {
        return stored.clone();
    }
    let discovered = discover(root, snapshot);
    if let Ok(mut cache) = CACHE.lock() {
        while cache.slots.len() >= MAX_CACHE {
            let oldest = cache
                .slots
                .iter()
                .min_by_key(|(_, (seen, _))| *seen)
                .map(|(path, _)| path.clone());
            if let Some(path) = oldest {
                cache.slots.remove(&path);
            } else {
                break;
            }
        }
        cache
            .slots
            .insert(root.to_path_buf(), (Instant::now(), discovered.clone()));
    }
    discovered
}

fn discover(root: &Path, snapshot: String) -> LanguageInventory {
    let mut by_suffix = BTreeMap::new();
    let mut seen = 0_usize;
    walk(root, &mut by_suffix, &mut seen);
    LanguageInventory {
        snapshot,
        by_suffix,
    }
}

fn walk(dir: &Path, counts: &mut BTreeMap<String, u32>, seen: &mut usize) {
    if *seen >= MAX_WALK_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *seen >= MAX_WALK_FILES {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.')
            || matches!(
                name.as_ref(),
                "target" | "node_modules" | "dist" | "build" | "vendor" | "__pycache__"
            )
        {
            continue;
        }
        if path.is_dir() {
            walk(&path, counts, seen);
            continue;
        }
        *seen += 1;
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        let suffix = format!(".{ext}");
        if SOURCE_SUFFIXES.contains(&suffix.as_str()) {
            *counts.entry(suffix).or_insert(0) += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::inventory;
    use std::path::Path;

    #[test]
    fn this_workspace_lists_rust() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let found = inventory(&root);
        assert!(
            found.by_suffix.keys().any(|suffix| suffix == ".rs"),
            "{found:?}"
        );
        assert!(found.glob().contains("rs"), "{}", found.glob());
    }
}
