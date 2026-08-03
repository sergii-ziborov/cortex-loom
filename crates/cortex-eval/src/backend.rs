//! Backend abstraction so the runner is testable without a live model.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

use cortex_ollama::{ModelInfo, OllamaClient, RunningModel, StructuredChatRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedContent {
    pub content: String,
    pub latency_ms: u64,
}

pub trait EvalBackend {
    fn version(&self) -> Result<String, String>;
    fn installed_models(&self) -> Result<Vec<ModelInfo>, String>;
    fn running_models(&self) -> Result<Vec<RunningModel>, String>;
    fn structured(&self, request: &StructuredChatRequest) -> Result<TimedContent, String>;
}

/// Live backend over the bounded loopback Ollama client. It never pulls a
/// model: absent models surface as absent.
pub struct OllamaEvalBackend {
    client: OllamaClient,
}

impl OllamaEvalBackend {
    #[must_use]
    pub const fn new(client: OllamaClient) -> Self {
        Self { client }
    }
}

impl EvalBackend for OllamaEvalBackend {
    fn version(&self) -> Result<String, String> {
        self.client
            .version()
            .map(|info| info.version)
            .map_err(|error| error.to_string())
    }

    fn installed_models(&self) -> Result<Vec<ModelInfo>, String> {
        self.client.tags().map_err(|error| error.to_string())
    }

    fn running_models(&self) -> Result<Vec<RunningModel>, String> {
        self.client
            .running_models()
            .map_err(|error| error.to_string())
    }

    fn structured(&self, request: &StructuredChatRequest) -> Result<TimedContent, String> {
        let started = Instant::now();
        let content = self
            .client
            .structured_chat(request)
            .map_err(|error| error.to_string())?;
        Ok(TimedContent {
            content,
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}

/// Deterministic scripted backend for tests.
pub struct ScriptedBackend {
    models: Vec<ModelInfo>,
    responses: Mutex<VecDeque<Result<String, String>>>,
}

impl ScriptedBackend {
    #[must_use]
    pub fn new(models: Vec<ModelInfo>, responses: Vec<Result<String, String>>) -> Self {
        Self {
            models,
            responses: Mutex::new(responses.into()),
        }
    }

    /// Number of scripted responses not yet consumed.
    pub fn remaining(&self) -> usize {
        self.responses.lock().map_or(0, |queue| queue.len())
    }
}

impl EvalBackend for ScriptedBackend {
    fn version(&self) -> Result<String, String> {
        Ok("scripted".to_owned())
    }

    fn installed_models(&self) -> Result<Vec<ModelInfo>, String> {
        Ok(self.models.clone())
    }

    fn running_models(&self) -> Result<Vec<RunningModel>, String> {
        Ok(Vec::new())
    }

    fn structured(&self, _request: &StructuredChatRequest) -> Result<TimedContent, String> {
        let response = self
            .responses
            .lock()
            .map_err(|_| "scripted backend lock poisoned".to_owned())?
            .pop_front()
            .ok_or_else(|| "scripted backend has no responses left".to_owned())?;
        response.map(|content| TimedContent {
            content,
            latency_ms: 1,
        })
    }
}
