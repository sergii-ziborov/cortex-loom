//! Report assembly, Markdown rendering, and JSON persistence.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::EvalError;
use crate::runner::{EmbeddingReport, ProfileReport, ProfileStatus};
use crate::sequence_suite::{SequenceLiveReport, SequenceLiveStatus};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalReport {
    pub generated_at_unix: u64,
    pub ollama_version: Option<String>,
    pub prompt_version: String,
    pub schema_version: String,
    pub profiles: Vec<ProfileReport>,
    #[serde(default)]
    pub embeddings: Vec<EmbeddingReport>,
    #[serde(default)]
    pub sequences: Vec<SequenceLiveReport>,
}

/// Write the JSON report into `directory` and return the file path.
pub fn write_json(directory: &Path, report: &EvalReport) -> Result<PathBuf, EvalError> {
    fs::create_dir_all(directory).map_err(|error| EvalError::Io(error.to_string()))?;
    let path = directory.join(format!("eval-{}.json", report.generated_at_unix));
    let serialized =
        serde_json::to_string(report).map_err(|error| EvalError::Json(error.to_string()))?;
    fs::write(&path, serialized).map_err(|error| EvalError::Io(error.to_string()))?;
    Ok(path)
}

#[must_use]
pub fn render_markdown(report: &EvalReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Cortex Loom calibration report");
    let _ = writeln!(
        out,
        "\ngenerated_at_unix: {} | ollama: {} | prompts: {} | schemas: {}",
        report.generated_at_unix,
        report.ollama_version.as_deref().unwrap_or("unreachable"),
        report.prompt_version,
        report.schema_version
    );
    for profile in &report.profiles {
        let _ = writeln!(out, "\n## {} — `{}`", profile.profile_id, profile.model);
        match profile.status {
            ProfileStatus::ModelAbsent => {
                let _ = writeln!(
                    out,
                    "status: model_absent — install explicitly to evaluate; nothing was pulled."
                );
                continue;
            }
            ProfileStatus::DiscoveryFailed => {
                let _ = writeln!(out, "status: discovery_failed — Ollama was unreachable.");
                continue;
            }
            ProfileStatus::Evaluated => {}
        }
        let _ = writeln!(
            out,
            "role: {:?} | digest: {} | device: {} | latency p50/p95/max: {}/{}/{} ms over {} calls",
            profile.tier,
            profile.digest.as_deref().unwrap_or("unknown"),
            profile.device.map_or("unknown", |device| match device {
                cortex_ollama::DevicePlacement::Cpu => "cpu",
                cortex_ollama::DevicePlacement::Gpu => "gpu",
            }),
            profile.latency.p50_ms,
            profile.latency.p95_ms,
            profile.latency.max_ms,
            profile.latency.samples
        );
        if let Some(aggregate) = &profile.classification {
            let _ = writeln!(
                out,
                "classification: {}/{} schema-valid, accuracy {:.2}, under-called {}, missed escalations {}",
                aggregate.schema_valid,
                aggregate.samples,
                aggregate.accuracy,
                aggregate.under_called,
                aggregate.missed_escalations
            );
        }
        if let Some(aggregate) = &profile.extraction {
            let _ = writeln!(
                out,
                "extraction: {}/{} schema-valid, action accuracy {:.2}, exact match {:.2}",
                aggregate.schema_valid,
                aggregate.samples,
                aggregate.action_accuracy,
                aggregate.exact_match_rate
            );
        }
        if let Some(aggregate) = &profile.compression {
            let _ = writeln!(
                out,
                "compression: {}/{} schema-valid, preserved mean {:.2} min {:.2}, hallucinated {}, mean token delta {}",
                aggregate.schema_valid,
                aggregate.samples,
                aggregate.mean_preserved_ratio,
                aggregate.min_preserved_ratio,
                aggregate.hallucinated_total,
                aggregate.mean_token_delta
            );
        }
        if profile.verdict.pass {
            let _ = writeln!(out, "verdict: PASS");
        } else {
            let _ = writeln!(out, "verdict: FAIL");
            for reason in &profile.verdict.reasons {
                let rendered = serde_json::to_string(reason).unwrap_or_default();
                let _ = writeln!(out, "- {rendered}");
            }
        }
    }
    for embedding in &report.embeddings {
        render_embedding(&mut out, embedding);
    }
    for sequence in &report.sequences {
        render_sequence(&mut out, sequence);
    }
    out
}

fn render_sequence(out: &mut String, sequence: &SequenceLiveReport) {
    let _ = writeln!(out, "\n## sequence — `{}`", sequence.model);
    let _ = writeln!(
        out,
        "status: {:?} | repetitions: {} | samples: {} | paired regressions: {} | verdict: {}",
        sequence.status,
        sequence.repetitions,
        sequence.samples.len(),
        sequence.paired_regressions.len(),
        if sequence.promoted { "PASS" } else { "FAIL" }
    );
    if sequence.status != SequenceLiveStatus::Evaluated
        && let Some(reason) = &sequence.reason
    {
        let _ = writeln!(out, "reason: {reason}");
    }
}

fn render_embedding(out: &mut String, embedding: &EmbeddingReport) {
    let _ = writeln!(out, "\n## {} — `{}`", embedding.profile_id, embedding.model);
    match embedding.status {
        ProfileStatus::ModelAbsent => {
            let _ = writeln!(
                out,
                "status: model_absent — install explicitly to evaluate; nothing was pulled."
            );
            return;
        }
        ProfileStatus::DiscoveryFailed => {
            let _ = writeln!(out, "status: discovery_failed — Ollama was unreachable.");
            return;
        }
        ProfileStatus::Evaluated => {}
    }
    if let Some(error) = &embedding.error {
        let _ = writeln!(out, "error: {error}");
        return;
    }
    let _ = writeln!(
        out,
        "role: embedding | ranking: {} | digest: {} | dims: {} | embed latency p50/p95: {}/{} ms over {} batches",
        embedding.ranking_version,
        embedding.digest.as_deref().unwrap_or("unknown"),
        embedding.dimensions.unwrap_or(0),
        embedding.latency.p50_ms,
        embedding.latency.p95_ms,
        embedding.latency.samples
    );
    for mode in &embedding.modes {
        let aggregate = &mode.aggregate;
        let _ = writeln!(
            out,
            "{:?}: {} queries, recall@3 {:.2}, recall@5 {:.2} (min {:.2}), ndcg@5 {:.2}, mrr {:.2} -> {}",
            mode.mode,
            aggregate.queries,
            aggregate.mean_recall_at_3,
            aggregate.mean_recall_at_5,
            aggregate.min_recall_at_5,
            aggregate.mean_ndcg_at_5,
            aggregate.mean_reciprocal_rank,
            if mode.verdict.pass { "PASS" } else { "FAIL" }
        );
    }
    match embedding.passing_mode() {
        Some(mode) => {
            let _ = writeln!(
                out,
                "verdict: PASS via {mode:?} — semantic selection is unblocked for this mode"
            );
        }
        None => {
            let _ = writeln!(out, "verdict: FAIL — no mode passes the gate");
        }
    }
}
