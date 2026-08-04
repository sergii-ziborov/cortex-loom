//! Shadow observation of local model profiles with zero workflow influence.
//!
//! The deterministic result is always computed and returned exactly as
//! before; the shadow runner only ever receives a copy of the inputs plus the
//! already-final deterministic outcome. It cannot modify routing, context,
//! citations, or `requiresUpstream`. Samples are append-only measurement data.

use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

use cortex_eval::backend::OllamaEvalBackend;
use cortex_ollama::{OllamaClient, OllamaConfig};
use cortex_router::{ModelTier, RiskLevel, TaskClass};
use cortex_store::ShadowStore;
use serde::Serialize;

mod worker;

pub use worker::{SHADOW_MEDIUM_PROFILE, SHADOW_SMALL_PROFILE};

/// Explicit runtime configuration; shadow mode is off unless
/// `CORTEX_SHADOW=1` and at least one exact model tag is supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowConfig {
    pub enabled: bool,
    /// Exact tag observed for `route_classification`.
    pub small_model: Option<String>,
    /// Exact tag observed for `context_compression`.
    pub medium_model: Option<String>,
    pub timeout_ms: u64,
    pub queue_capacity: usize,
    /// Compression observations larger than this estimated input are skipped
    /// and counted instead of queued: on-CPU latency for large payloads does
    /// not produce comparable samples (dogfood finding — a real 7.5k-token
    /// packet timed out where 200-token fixtures succeeded).
    pub max_compression_input_tokens: u32,
}

impl ShadowConfig {
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let non_empty = |key: &str| {
            lookup(key).and_then(|value| {
                let trimmed = value.trim().to_owned();
                (!trimmed.is_empty()).then_some(trimmed)
            })
        };
        Self {
            enabled: non_empty("CORTEX_SHADOW").is_some_and(|value| value == "1"),
            small_model: non_empty("CORTEX_SHADOW_SMALL"),
            medium_model: non_empty("CORTEX_SHADOW_MEDIUM"),
            timeout_ms: non_empty("CORTEX_SHADOW_TIMEOUT_MS")
                .and_then(|value| value.parse().ok())
                .unwrap_or(30_000),
            queue_capacity: non_empty("CORTEX_SHADOW_QUEUE")
                .and_then(|value| value.parse().ok())
                .unwrap_or(64),
            max_compression_input_tokens: non_empty("CORTEX_SHADOW_MAX_COMPRESSION_TOKENS")
                .and_then(|value| value.parse().ok())
                .unwrap_or(2_048),
        }
    }

    /// True only when explicitly enabled with at least one model to observe.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && (self.small_model.is_some() || self.medium_model.is_some())
    }
}

/// Snapshot of an already-final routing decision.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoutingSnapshot {
    pub tier: ModelTier,
    pub class: TaskClass,
    pub risk: RiskLevel,
}

/// Snapshot of an already-final deterministic context packet.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompressionSnapshot {
    pub included_ids: Vec<String>,
    pub omitted_ids: Vec<String>,
    pub selected_estimated_tokens: u32,
    pub requires_upstream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowEvidence {
    pub id: String,
    pub source: String,
    pub content: String,
}

/// A self-contained observation task; nothing in it can reach back into the
/// hot path.
#[derive(Debug, Clone, PartialEq)]
pub enum ShadowTask {
    RouteClassification {
        task: String,
        deterministic: RoutingSnapshot,
    },
    ContextCompression {
        task: String,
        evidence: Vec<ShadowEvidence>,
        deterministic: CompressionSnapshot,
    },
}

#[derive(Debug)]
pub enum ShadowError {
    InvalidConfiguration(String),
    WorkerSpawn(String),
}

impl Display for ShadowError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid shadow configuration: {message}")
            }
            Self::WorkerSpawn(message) => {
                write!(formatter, "shadow worker failed to start: {message}")
            }
        }
    }
}

impl std::error::Error for ShadowError {}

/// Cheap cloneable handle for fire-and-forget observation.
pub struct ShadowHandle {
    sender: SyncSender<ShadowTask>,
    dropped: Arc<AtomicU64>,
    oversize_skipped: Arc<AtomicU64>,
    small_model: Option<String>,
    medium_model: Option<String>,
    max_compression_input_tokens: u32,
}

impl ShadowHandle {
    /// Enqueue an observation without ever blocking. A full queue drops the
    /// sample and increments the drop counter; an operation without a
    /// configured model is ignored entirely; a compression payload above the
    /// input cap is skipped and counted instead of producing an incomparable
    /// slow sample.
    pub fn observe(&self, task: ShadowTask) {
        let configured = match &task {
            ShadowTask::RouteClassification { .. } => self.small_model.is_some(),
            ShadowTask::ContextCompression { .. } => self.medium_model.is_some(),
        };
        if !configured {
            return;
        }
        if let ShadowTask::ContextCompression { task, evidence, .. } = &task {
            let estimated = evidence
                .iter()
                .map(|item| cortex_context::estimate_tokens(&item.content))
                .fold(cortex_context::estimate_tokens(task), u32::saturating_add);
            if estimated > self.max_compression_input_tokens {
                self.oversize_skipped.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        if let Err(TrySendError::Full(_)) = self.sender.try_send(task) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Samples dropped by this process because the queue was full.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Compression observations skipped because the payload exceeded the cap.
    #[must_use]
    pub fn oversize_skipped(&self) -> u64 {
        self.oversize_skipped.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn small_model(&self) -> Option<&str> {
        self.small_model.as_deref()
    }

    #[must_use]
    pub fn medium_model(&self) -> Option<&str> {
        self.medium_model.as_deref()
    }
}

/// Start the shadow runner. Returns `Ok(None)` when shadow mode is inactive:
/// no thread, no queue, no samples.
pub fn spawn(
    config: ShadowConfig,
    store: ShadowStore,
) -> Result<Option<ShadowHandle>, ShadowError> {
    if !config.is_active() {
        return Ok(None);
    }
    let timeout = Duration::from_millis(config.timeout_ms.max(1));
    let mut ollama = OllamaConfig {
        request_timeout: timeout,
        read_timeout: timeout,
        write_timeout: timeout,
        ..OllamaConfig::default()
    };
    if let Some(model) = &config.small_model {
        ollama = ollama.with_profile(SHADOW_SMALL_PROFILE, worker::shadow_profile(model));
    }
    if let Some(model) = &config.medium_model {
        ollama = ollama.with_profile(SHADOW_MEDIUM_PROFILE, worker::shadow_profile(model));
    }
    let client = OllamaClient::new(ollama)
        .map_err(|error| ShadowError::InvalidConfiguration(error.to_string()))?;
    let backend = OllamaEvalBackend::new(client);
    spawn_with_backend(config, store, backend)
}

/// Backend-generic runner start; tests supply a scripted backend.
pub fn spawn_with_backend<B>(
    config: ShadowConfig,
    store: ShadowStore,
    backend: B,
) -> Result<Option<ShadowHandle>, ShadowError>
where
    B: cortex_eval::backend::EvalBackend + Send + 'static,
{
    if !config.is_active() {
        return Ok(None);
    }
    let (sender, receiver) = sync_channel::<ShadowTask>(config.queue_capacity.max(1));
    let models = worker::WorkerModels {
        small: config.small_model.clone(),
        medium: config.medium_model.clone(),
    };
    thread::Builder::new()
        .name("cortex-shadow".to_owned())
        .spawn(move || worker::run(&receiver, &backend, &store, &models))
        .map_err(|error| ShadowError::WorkerSpawn(error.to_string()))?;
    Ok(Some(ShadowHandle {
        sender,
        dropped: Arc::new(AtomicU64::new(0)),
        oversize_skipped: Arc::new(AtomicU64::new(0)),
        small_model: config.small_model,
        medium_model: config.medium_model,
        max_compression_input_tokens: config.max_compression_input_tokens,
    }))
}

#[cfg(test)]
mod tests;
