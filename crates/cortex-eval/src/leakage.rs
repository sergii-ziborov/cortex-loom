//! Automatic train/eval leakage guards.
//!
//! A label of `split=holdout` is not a defence. These checks refuse a
//! generated corpus when train and eval share an exact hash, a near-duplicate
//! shingle, a task family that came from gold fixtures, a repository path, or
//! a named symbol/mechanism.

use std::collections::{BTreeSet, HashSet};

use sha2::{Digest, Sha256};

use crate::EvalError;
use crate::corpus::CorpusRecord;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeakageReport {
    pub exact_hashes: Vec<String>,
    pub near_duplicates: Vec<String>,
    pub task_families: Vec<String>,
    pub repositories: Vec<String>,
    pub symbols: Vec<String>,
}

impl LeakageReport {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.exact_hashes.is_empty()
            && self.near_duplicates.is_empty()
            && self.task_families.is_empty()
            && self.repositories.is_empty()
            && self.symbols.is_empty()
    }
}

/// Compare train records against eval/holdout records.
#[must_use]
pub fn detect(train: &[&CorpusRecord], eval: &[&CorpusRecord]) -> LeakageReport {
    let eval_hashes: HashSet<String> = eval.iter().map(|record| body_hash(record)).collect();
    let eval_shingles: Vec<BTreeSet<u64>> = eval.iter().map(|record| shingles(record)).collect();
    let eval_repos: HashSet<String> = eval
        .iter()
        .filter_map(|record| repository(record))
        .collect();
    let eval_symbols: HashSet<String> = eval.iter().flat_map(|record| symbols(record)).collect();
    let eval_gold_families: HashSet<String> = eval
        .iter()
        .filter(|record| record.split == crate::corpus::SPLIT_HOLDOUT)
        .map(|record| record.task.clone())
        .collect();

    let mut report = LeakageReport::default();
    for record in train {
        let hash = body_hash(record);
        if eval_hashes.contains(&hash) {
            report.exact_hashes.push(record.id.clone());
        }
        if is_near_duplicate(&shingles(record), &eval_shingles)
            || is_minhash_near_duplicate(record, eval)
        {
            report.near_duplicates.push(record.id.clone());
        }
        if record.source.contains("fixtures/")
            && record.split != crate::corpus::SPLIT_HOLDOUT
            && eval_gold_families.contains(&record.task)
        {
            report.task_families.push(record.task.clone());
        }
        if let Some(repo) = repository(record)
            && eval_repos.contains(&repo)
            && record.source.contains("fixtures/")
        {
            report.repositories.push(record.id.clone());
        }
        for symbol in symbols(record) {
            if eval_symbols.contains(&symbol) && record.source.contains("fixtures/") {
                report.symbols.push(format!("{}:{symbol}", record.id));
            }
        }
    }
    report.exact_hashes.sort();
    report.exact_hashes.dedup();
    report.near_duplicates.sort();
    report.near_duplicates.dedup();
    report.task_families.sort();
    report.task_families.dedup();
    report.repositories.sort();
    report.repositories.dedup();
    report.symbols.sort();
    report.symbols.dedup();
    report
}

/// Refuse to emit a train file that overlaps eval gold.
pub fn refuse_if_leaky(train: &[&CorpusRecord], eval: &[&CorpusRecord]) -> Result<(), EvalError> {
    let report = detect(train, eval);
    if report.exact_hashes.is_empty()
        && report.repositories.is_empty()
        && report.task_families.is_empty()
    {
        return Ok(());
    }
    Err(EvalError::Fixture(format!(
        "train/eval leakage: exact={} repos={} families={}",
        report.exact_hashes.join(","),
        report.repositories.join(","),
        report.task_families.join(","),
    )))
}

fn body_hash(record: &CorpusRecord) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalize(&record.input).as_bytes());
    hasher.update([0xff]);
    hasher.update(normalize(&record.output).as_bytes());
    format!("{:x}", hasher.finalize())
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

fn is_near_duplicate(train: &BTreeSet<u64>, eval: &[BTreeSet<u64>]) -> bool {
    if train.len() < 16 {
        return false;
    }
    eval.iter().any(|other| {
        if other.len() < 16 {
            return false;
        }
        let inter = train.intersection(other).count();
        let union = train.union(other).count();
        union > 0 && inter * 100 / union >= 80
    })
}

fn shingles(record: &CorpusRecord) -> BTreeSet<u64> {
    let blob = normalize(&record.input);
    if blob.chars().count() < 40 {
        return BTreeSet::new();
    }
    let chars: Vec<char> = blob.chars().collect();
    let mut out = BTreeSet::new();
    if chars.len() < 12 {
        return out;
    }
    for window in chars.windows(12) {
        let mut hasher = Sha256::new();
        for ch in window {
            let mut buf = [0; 4];
            hasher.update(ch.encode_utf8(&mut buf).as_bytes());
        }
        let digest = hasher.finalize();
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&digest[..8]);
        out.insert(u64::from_le_bytes(bytes));
        if out.len() >= 64 {
            break;
        }
    }
    out
}

/// 16-permutation `MinHash` over the same 12-grams. Used as a second
/// near-duplicate detector so a long shared prefix cannot hide behind
/// Jaccard on a truncated shingle set.
fn minhash(record: &CorpusRecord) -> [u64; 16] {
    let grams = shingles(record);
    let mut signature = [u64::MAX; 16];
    if grams.is_empty() {
        return signature;
    }
    for gram in grams {
        for (lane, slot) in signature.iter_mut().enumerate() {
            let mixed = gram.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(
                u64::try_from(lane)
                    .unwrap_or(0)
                    .wrapping_mul(0xbf58_476d_1ce4_e5b9),
            );
            *slot = (*slot).min(mixed);
        }
    }
    signature
}

fn is_minhash_near_duplicate(train: &CorpusRecord, eval: &[&CorpusRecord]) -> bool {
    let left = minhash(train);
    if left.iter().all(|value| *value == u64::MAX) {
        return false;
    }
    eval.iter().any(|other| {
        let right = minhash(other);
        if right.iter().all(|value| *value == u64::MAX) {
            return false;
        }
        let same = left
            .iter()
            .zip(right.iter())
            .filter(|(a, b)| a == b)
            .count();
        same * 100 / 16 >= 80
    })
}

fn repository(record: &CorpusRecord) -> Option<String> {
    record.source.split('/').nth(1).map(str::to_owned)
}

fn symbols(record: &CorpusRecord) -> Vec<String> {
    record
        .input
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|token| {
            token.len() >= 4 && token.chars().any(|ch| ch.is_ascii_uppercase() || ch == '_')
        })
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::CorpusRecord;

    fn record(id: &str, input: &str, source: &str) -> CorpusRecord {
        CorpusRecord::new(
            id,
            "classification",
            "classification",
            "x",
            input,
            "y",
            source,
        )
    }

    #[test]
    fn exact_normalized_overlap_is_caught() {
        let train = record("t", "Rename  ArchiveOptions", "crates/x/src/a.rs");
        let eval = record(
            "e",
            "rename archiveoptions",
            "crates/cortex-eval/fixtures/x.json",
        );
        let report = detect(&[&train], &[&eval]);
        assert!(!report.exact_hashes.is_empty());
    }

    #[test]
    fn a_copied_holdout_input_is_a_near_duplicate() {
        let long = "The verified ArchiveOptions struct defaults enabled to false and max_entries to 32 in crates/cortex-weavatrix/src/options.rs";
        let train = record("t", long, "crates/cortex-eval/src/micro_train.rs");
        let eval = record("e", long, "crates/cortex-eval/fixtures/extraction.json");
        let report = detect(&[&train], &[&eval]);
        assert!(!report.exact_hashes.is_empty() || !report.near_duplicates.is_empty());
    }

    #[test]
    fn disjoint_generated_train_is_clean() {
        let train = record(
            "t",
            "train-only identifier FOO_BAR_UNIQUE_99",
            "crates/cortex-eval/src/micro_train.rs",
        );
        let eval = record(
            "e",
            "holdout identifier ARCHIVE_OPTIONS",
            "crates/cortex-eval/fixtures/extraction.json",
        );
        let report = detect(&[&train], &[&eval]);
        assert!(report.is_clean(), "{report:?}");
    }

    #[test]
    fn gold_fixture_family_in_train_is_leakage() {
        let mut train = record(
            "t",
            "train-only identifier FOO_BAR_UNIQUE_99",
            "crates/cortex-eval/fixtures/classification.json",
        );
        train.task = "classification".to_owned();
        let mut eval = record(
            "e",
            "holdout identifier ARCHIVE_OPTIONS",
            "crates/cortex-eval/fixtures/classification.json",
        );
        eval.split = crate::corpus::SPLIT_HOLDOUT.to_owned();
        eval.task = "classification".to_owned();
        let report = detect(&[&train], &[&eval]);
        assert!(!report.task_families.is_empty(), "{report:?}");
        assert!(refuse_if_leaky(&[&train], &[&eval]).is_err());
    }
}
