//! A bounded, blocking client for a loopback Ollama service.

mod client;
mod quality;
mod types;

use std::fmt::{Display, Formatter};

pub use client::OllamaClient;
pub use quality::assess_local_draft;
pub use types::{
    ChatMessage, ChatRole, DevicePlacement, DraftAssessment, DraftRequest, LocalDraft, ModelInfo,
    ModelProfile, OllamaConfig, QualityFailure, RunningModel, VersionInfo,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OllamaError {
    InvalidConfiguration(String),
    UnknownProfile(String),
    InputBudgetExceeded { estimated: u32, limit: u32 },
    OutputBudgetExceeded { requested: u32, limit: u32 },
    ContextBudgetExceeded { requested: u32, limit: u32 },
    RequestTooLarge { bytes: usize, limit: usize },
    Http(String),
    Json(String),
}

impl Display for OllamaError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid Ollama configuration: {message}")
            }
            Self::UnknownProfile(profile) => {
                write!(formatter, "unknown Ollama model profile: {profile}")
            }
            Self::InputBudgetExceeded { estimated, limit } => {
                write!(
                    formatter,
                    "input budget exceeded: estimated {estimated}, limit {limit}"
                )
            }
            Self::OutputBudgetExceeded { requested, limit } => {
                write!(
                    formatter,
                    "output budget exceeded: requested {requested}, limit {limit}"
                )
            }
            Self::ContextBudgetExceeded { requested, limit } => {
                write!(
                    formatter,
                    "context budget exceeded: requested {requested}, limit {limit}"
                )
            }
            Self::RequestTooLarge { bytes, limit } => {
                write!(
                    formatter,
                    "request body too large: {bytes} bytes, limit {limit}"
                )
            }
            Self::Http(message) => write!(formatter, "Ollama HTTP error: {message}"),
            Self::Json(message) => write!(formatter, "Ollama JSON error: {message}"),
        }
    }
}

impl std::error::Error for OllamaError {}

#[cfg(test)]
mod tests;
