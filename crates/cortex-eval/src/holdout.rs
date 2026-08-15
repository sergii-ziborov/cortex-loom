//! Leave-one-out release folds over public fixtures.
//!
//! A label of `split=holdout` is not a release gate. These partitions prove
//! a reported number was not computed on the same repository, language, or
//! task family the model (or heuristic) just saw.

use std::collections::{BTreeMap, BTreeSet};

use crate::corpus::CorpusRecord;
use crate::fixtures::{ExtractionFixture, default_fixtures};

/// Axis a release number must be held out on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldoutAxis {
    Repository,
    Language,
    TaskFamily,
}

/// One fold: everything except `held`, plus the held-out key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaveOneOutFold {
    pub axis: &'static str,
    pub held: String,
    pub train_ids: Vec<String>,
    pub eval_ids: Vec<String>,
}

/// Partition records so each key is scored only against the other keys.
#[must_use]
pub fn folds(records: &[&CorpusRecord], axis: HoldoutAxis) -> Vec<LeaveOneOutFold> {
    let mut buckets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for record in records {
        buckets
            .entry(key(record, axis))
            .or_default()
            .push(record.id.clone());
    }
    let axis_name = match axis {
        HoldoutAxis::Repository => "repository",
        HoldoutAxis::Language => "language",
        HoldoutAxis::TaskFamily => "task_family",
    };
    buckets
        .iter()
        .map(|(held, eval_ids)| {
            let train_ids = buckets
                .iter()
                .filter(|(name, _)| *name != held)
                .flat_map(|(_, ids)| ids.clone())
                .collect();
            LeaveOneOutFold {
                axis: axis_name,
                held: held.clone(),
                train_ids,
                eval_ids: eval_ids.clone(),
            }
        })
        .collect()
}

/// True when no train id appears in the fold's eval set.
#[must_use]
pub fn fold_is_disjoint(fold: &LeaveOneOutFold) -> bool {
    let eval: BTreeSet<&str> = fold.eval_ids.iter().map(String::as_str).collect();
    fold.train_ids.iter().all(|id| !eval.contains(id.as_str()))
}

fn key(record: &CorpusRecord, axis: HoldoutAxis) -> String {
    match axis {
        HoldoutAxis::TaskFamily => record.task.clone(),
        HoldoutAxis::Repository => repository_of(record),
        HoldoutAxis::Language => language_of(record),
    }
}

fn repository_of(record: &CorpusRecord) -> String {
    record
        .source
        .split('/')
        .nth(1)
        .unwrap_or("unknown")
        .to_owned()
}

fn language_of(record: &CorpusRecord) -> String {
    record
        .input
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.' && ch != '_' && ch != '-')
        .find_map(|token| {
            token.rsplit_once('.').and_then(|(_, ext)| {
                matches!(
                    ext,
                    "rs" | "ts" | "js" | "py" | "go" | "java" | "md" | "json"
                )
                .then_some(ext)
            })
        })
        .unwrap_or("unknown")
        .to_owned()
}

/// Extraction fixtures grouped by first gold file extension.
pub fn extraction_language_folds() -> Result<Vec<LeaveOneOutFold>, crate::EvalError> {
    let fixtures = default_fixtures()?.extraction;
    let records: Vec<CorpusRecord> = fixtures.iter().map(extraction_as_record).collect();
    let refs: Vec<&CorpusRecord> = records.iter().collect();
    Ok(folds(&refs, HoldoutAxis::Language))
}

fn extraction_as_record(fixture: &ExtractionFixture) -> CorpusRecord {
    CorpusRecord::new(
        format!("ext:{}", fixture.id),
        "extraction",
        "classification",
        "x",
        fixture.text.clone(),
        fixture.gold.files.join(" "),
        fixture
            .gold
            .files
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{SPLIT_HOLDOUT, build};

    #[test]
    fn leave_one_task_family_out_is_disjoint() {
        let records = build().expect("corpus");
        let eval: Vec<&CorpusRecord> = records
            .iter()
            .filter(|record| record.split == SPLIT_HOLDOUT)
            .collect();
        let families = folds(&eval, HoldoutAxis::TaskFamily);
        assert!(families.len() >= 3, "too few families: {}", families.len());
        for fold in &families {
            assert!(fold_is_disjoint(fold), "{fold:?}");
            assert!(!fold.eval_ids.is_empty());
        }
    }

    #[test]
    fn leave_one_language_out_covers_rust() {
        let folds = extraction_language_folds().expect("extraction");
        assert!(folds.iter().any(|fold| fold.held == "rs"));
        assert!(folds.iter().all(fold_is_disjoint));
    }
}
