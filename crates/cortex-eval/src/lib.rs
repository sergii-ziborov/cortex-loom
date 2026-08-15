//! Offline benchmark and calibration for local model profiles.
//!
//! The harness measures candidate Ollama profiles on typed fixtures for the
//! three roadmap suites: routing classification, structured extraction, and
//! citation-preserving compression. Comparators are pure functions so shadow
//! mode can reuse them later. The harness never pulls models: an absent model
//! is reported as absent, and a calibration verdict is measurement data, never
//! a workflow authority.

use std::fmt::{Display, Formatter};

use serde::Serialize;

pub mod backend;
pub mod comparators;
pub mod corpus;
mod corpus_micro;
pub mod fixtures;
pub mod holdout;
pub mod leakage;
pub mod metrics;
mod micro_cases;
pub mod micro_train;
mod micro_vocab;
pub mod openai_backend;
mod profile_suites;
pub mod prompts;
pub mod report;
pub mod runner;
pub mod sequence_suite;
pub mod verdict;

pub use cortex_context::ranking;

/// Pinned prompt revision recorded in every report.
pub const PROMPT_VERSION: &str = "eval-prompts-v5";
/// Pinned output-schema revision recorded in every report.
pub const SCHEMA_VERSION: &str = "eval-schemas-v1";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SuiteKind {
    Classification,
    Extraction,
    Compression,
    Retrieval,
    MicroExtraction,
    Sequence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    Config(String),
    Fixture(String),
    Io(String),
    Json(String),
}

impl Display for EvalError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(message) => write!(formatter, "invalid eval configuration: {message}"),
            Self::Fixture(message) => write!(formatter, "invalid eval fixture: {message}"),
            Self::Io(message) => write!(formatter, "eval report I/O error: {message}"),
            Self::Json(message) => write!(formatter, "eval JSON error: {message}"),
        }
    }
}

impl std::error::Error for EvalError {}

#[cfg(test)]
mod micro_tests;
#[cfg(test)]
mod tests;
