//! Typed fixture suites embedded in the crate.

use std::collections::HashSet;

use cortex_router::{ModelTier, TaskClass};
use serde::Deserialize;

use crate::EvalError;
use crate::comparators::is_citation_id;

pub const ALLOWED_ACTIONS: &[&str] = &[
    "add", "fix", "remove", "rename", "move", "refactor", "document", "test", "update", "other",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClassificationFixture {
    pub id: String,
    pub task: String,
    pub gold_class: TaskClass,
    pub gold_tier: ModelTier,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtractionFixture {
    pub id: String,
    pub text: String,
    pub gold: ExtractionGold,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtractionGold {
    pub action: String,
    pub files: Vec<String>,
    pub symbols: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompressionFixture {
    pub id: String,
    pub task: String,
    pub evidence: Vec<EvidenceFixture>,
    pub must_cite: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceFixture {
    pub id: String,
    pub source: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct FixtureSet {
    pub classification: Vec<ClassificationFixture>,
    pub extraction: Vec<ExtractionFixture>,
    pub compression: Vec<CompressionFixture>,
}

/// Load and validate the fixture suites embedded in the crate.
pub fn default_fixtures() -> Result<FixtureSet, EvalError> {
    load_fixtures(
        include_str!("../fixtures/classification.json"),
        include_str!("../fixtures/extraction.json"),
        include_str!("../fixtures/compression.json"),
    )
}

pub fn load_fixtures(
    classification: &str,
    extraction: &str,
    compression: &str,
) -> Result<FixtureSet, EvalError> {
    let set = FixtureSet {
        classification: parse(classification, "classification")?,
        extraction: parse(extraction, "extraction")?,
        compression: parse(compression, "compression")?,
    };
    validate(&set)?;
    Ok(set)
}

fn parse<T: for<'de> Deserialize<'de>>(source: &str, suite: &str) -> Result<Vec<T>, EvalError> {
    serde_json::from_str(source).map_err(|error| EvalError::Fixture(format!("{suite}: {error}")))
}

fn validate(set: &FixtureSet) -> Result<(), EvalError> {
    require_unique_ids("classification", set.classification.iter().map(|f| &f.id))?;
    require_unique_ids("extraction", set.extraction.iter().map(|f| &f.id))?;
    require_unique_ids("compression", set.compression.iter().map(|f| &f.id))?;

    for fixture in &set.classification {
        if fixture.task.trim().is_empty() {
            return Err(EvalError::Fixture(format!(
                "{} has an empty task",
                fixture.id
            )));
        }
    }
    for fixture in &set.extraction {
        if !ALLOWED_ACTIONS.contains(&fixture.gold.action.as_str()) {
            return Err(EvalError::Fixture(format!(
                "{} has unsupported action {}",
                fixture.id, fixture.gold.action
            )));
        }
    }
    for fixture in &set.compression {
        if fixture.evidence.is_empty() || fixture.must_cite.is_empty() {
            return Err(EvalError::Fixture(format!(
                "{} needs evidence and mustCite entries",
                fixture.id
            )));
        }
        let mut evidence_ids = HashSet::new();
        for evidence in &fixture.evidence {
            if !is_citation_id(&evidence.id) {
                return Err(EvalError::Fixture(format!(
                    "{} evidence id {} is not a citation id",
                    fixture.id, evidence.id
                )));
            }
            if evidence.content.trim().is_empty() || evidence.source.trim().is_empty() {
                return Err(EvalError::Fixture(format!(
                    "{} evidence {} has empty fields",
                    fixture.id, evidence.id
                )));
            }
            if !evidence_ids.insert(evidence.id.as_str()) {
                return Err(EvalError::Fixture(format!(
                    "{} repeats evidence id {}",
                    fixture.id, evidence.id
                )));
            }
        }
        for id in &fixture.must_cite {
            if !evidence_ids.contains(id.as_str()) {
                return Err(EvalError::Fixture(format!(
                    "{} requires citation of unknown id {id}",
                    fixture.id
                )));
            }
        }
    }
    Ok(())
}

fn require_unique_ids<'a>(
    suite: &str,
    ids: impl Iterator<Item = &'a String>,
) -> Result<(), EvalError> {
    let mut seen = HashSet::new();
    for id in ids {
        if id.trim().is_empty() {
            return Err(EvalError::Fixture(format!("{suite} contains an empty id")));
        }
        if !seen.insert(id.as_str()) {
            return Err(EvalError::Fixture(format!("{suite} repeats id {id}")));
        }
    }
    Ok(())
}
