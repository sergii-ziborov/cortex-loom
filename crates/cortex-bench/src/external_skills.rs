//! Explicit, bounded loading of an external skill library for benchmarks.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::sequence::ExternalLibraryStamp;

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_SKILLS: usize = 128;

pub(crate) struct ExternalSkillLibrary {
    skills: BTreeMap<String, String>,
    pub(crate) stamp: Option<ExternalLibraryStamp>,
    unavailable_reason: Option<String>,
}

impl ExternalSkillLibrary {
    pub(crate) fn load(root: Option<&Path>) -> Self {
        let Some(root) = root else {
            return Self {
                skills: BTreeMap::new(),
                stamp: None,
                unavailable_reason: Some("--superpowers-root was not supplied".to_owned()),
            };
        };
        match load_root(root) {
            Ok((skills, stamp)) => Self {
                skills,
                stamp: Some(stamp),
                unavailable_reason: None,
            },
            Err(reason) => Self {
                skills: BTreeMap::new(),
                stamp: None,
                unavailable_reason: Some(reason),
            },
        }
    }

    pub(crate) fn body(&self, skill: &str) -> Result<&str, String> {
        if self.stamp.is_none() {
            return Err(self
                .unavailable_reason
                .clone()
                .unwrap_or_else(|| "external library unavailable".to_owned()));
        }
        self.skills
            .get(skill)
            .map(String::as_str)
            .ok_or_else(|| format!("upstream skill is absent: {skill}"))
    }
}

fn load_root(root: &Path) -> Result<(BTreeMap<String, String>, ExternalLibraryStamp), String> {
    let license = read_bounded_regular_file(&root.join("LICENSE"))?;
    let skills_root = root.join("skills");
    if !skills_root.is_dir() {
        return Err("root must contain LICENSE and skills/".to_owned());
    }
    let mut entries: Vec<_> = std::fs::read_dir(&skills_root)
        .map_err(|error| format!("could not read {}: {error}", skills_root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("could not enumerate skills: {error}"))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    if entries.len() > MAX_SKILLS {
        return Err(format!("upstream library exceeds {MAX_SKILLS} entries"));
    }
    let mut skills = BTreeMap::new();
    let mut skill_sha256 = BTreeMap::new();
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect upstream skill: {error}"))?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let Some(id) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        let path = entry.path().join("SKILL.md");
        if !path.is_file() {
            continue;
        }
        let body = read_bounded_regular_file(&path)?;
        skill_sha256.insert(id.clone(), digest(&body));
        skills.insert(id, body);
    }
    let root_label = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("external-library")
        .to_owned();
    Ok((
        skills,
        ExternalLibraryStamp {
            root_label: root_label.clone(),
            version: Some(root_label),
            license_sha256: digest(&license),
            skill_sha256,
        },
    ))
}

fn read_bounded_regular_file(path: &Path) -> Result<String, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| format!("missing bounded upstream file: {}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "upstream input is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!(
            "upstream input exceeds 256 KiB: {}",
            path.display()
        ));
    }
    std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
