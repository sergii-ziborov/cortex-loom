//! Deterministic fine-tune corpus from Cortex-owned fixtures and sequences.
//!
//! Superpowers is a measured baseline, not a training source. Records are
//! Cortex-original: eval gold, typed sequence packets, and mechanism labels.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::EvalError;
use crate::fixtures::default_fixtures;

const LICENSE: &str = "MIT OR Apache-2.0";
const TRAINING_SOURCE: &str = "cortex-original";
/// Rows a fine-tune may consume.
pub const SPLIT_TRAIN: &str = "train";
/// Rows derived from gold the harness scores a gate on. Training on these and
/// then claiming the gate would measure memorisation, so they are labelled at
/// the source rather than filtered by convention downstream.
pub const SPLIT_HOLDOUT: &str = "holdout";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorpusRecord {
    pub id: String,
    pub task: String,
    pub target_role: String,
    pub instruction: String,
    pub input: String,
    pub output: String,
    pub source: String,
    pub license: String,
    pub training_source: String,
    pub split: String,
}

impl CorpusRecord {
    pub(crate) fn new(
        id: impl Into<String>,
        task: impl Into<String>,
        target_role: impl Into<String>,
        instruction: impl Into<String>,
        input: impl Into<String>,
        output: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            task: task.into(),
            target_role: target_role.into(),
            instruction: instruction.into(),
            input: input.into(),
            output: output.into(),
            source: source.into(),
            license: LICENSE.to_owned(),
            training_source: TRAINING_SOURCE.to_owned(),
            split: SPLIT_TRAIN.to_owned(),
        }
    }

    /// Mark a record as derived from gold a gate is scored on.
    #[must_use]
    pub(crate) fn into_holdout(mut self) -> Self {
        SPLIT_HOLDOUT.clone_into(&mut self.split);
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CorpusManifest {
    training_source: &'static str,
    license: &'static str,
    records: usize,
    by_task: Vec<CountRow>,
    by_role: Vec<CountRow>,
    by_split: Vec<CountRow>,
    /// The file a `micro_extract` fine-tune consumes: train rows only.
    micro_extract_train_records: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CountRow {
    name: String,
    count: usize,
}

/// Workspace `corpora/` directory.
#[must_use]
pub fn default_out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpora")
}

/// Build every Cortex-owned record, train and holdout alike.
pub fn build() -> Result<Vec<CorpusRecord>, EvalError> {
    let mut records = classification_records()?;
    records.extend(extraction_records()?);
    records.extend(crate::corpus_micro::holdout_records()?);
    records.extend(crate::corpus_micro::train_records()?);
    records.extend(compression_records()?);
    records.extend(sequence_step_records()?);
    records.extend(mechanism_records());
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

/// The `micro_extract` rows a fine-tune may consume: train split only, so the
/// shipped fixtures stay a holdout the 0.6B gate can still mean something on.
#[must_use]
pub fn micro_extract_train(records: &[CorpusRecord]) -> Vec<&CorpusRecord> {
    records
        .iter()
        .filter(|record| record.target_role == "micro_extract" && record.split == SPLIT_TRAIN)
        .collect()
}

/// Write physically split `train/` and `dev/` under `out_dir`.
/// Gold stay in `crates/cortex-eval/fixtures/`; reserved suites live in
/// workspace `eval/public` and `eval/private`, never under `corpora/`.
pub fn write_to(out_dir: &Path) -> Result<usize, EvalError> {
    let records = build()?;
    let train_all: Vec<&CorpusRecord> = records
        .iter()
        .filter(|record| record.split == SPLIT_TRAIN)
        .collect();
    let eval: Vec<&CorpusRecord> = records
        .iter()
        .filter(|record| record.split == SPLIT_HOLDOUT)
        .collect();
    crate::leakage::refuse_if_leaky(&train_all, &eval)?;
    let (train, dev): (Vec<&CorpusRecord>, Vec<&CorpusRecord>) = train_all
        .into_iter()
        .partition(|record| !is_dev_record(record));
    write_split(out_dir, "train", &train)?;
    write_split(out_dir, "dev", &dev)?;
    fs::write(out_dir.join("README.md"), readme())
        .map_err(|error| EvalError::Io(error.to_string()))?;
    let micro_train = train
        .iter()
        .filter(|record| record.target_role == "micro_extract")
        .count();
    let manifest = serde_json::to_string_pretty(&manifest_for(&records, micro_train))
        .map_err(|error| EvalError::Json(error.to_string()))?;
    fs::write(out_dir.join("manifest.json"), format!("{manifest}\n"))
        .map_err(|error| EvalError::Io(error.to_string()))?;
    Ok(train.len() + dev.len())
}

fn is_dev_record(record: &CorpusRecord) -> bool {
    let digest = Sha256::digest(record.id.as_bytes());
    digest[0].is_multiple_of(10)
}

fn write_split(out_dir: &Path, name: &str, records: &[&CorpusRecord]) -> Result<(), EvalError> {
    let dir = out_dir.join(name);
    fs::create_dir_all(&dir).map_err(|error| EvalError::Io(error.to_string()))?;
    fs::write(dir.join("sft.jsonl"), jsonl(records.iter().copied())?)
        .map_err(|error| EvalError::Io(error.to_string()))?;
    Ok(())
}

fn jsonl<'a>(records: impl Iterator<Item = &'a CorpusRecord>) -> Result<String, EvalError> {
    let body = records
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| EvalError::Json(error.to_string()))?
        .join("\n");
    Ok(body + "\n")
}

pub fn write_cli() -> Result<(), EvalError> {
    let out = default_out_dir();
    let count = write_to(&out)?;
    let records = build()?;
    println!(
        "cortex-eval corpus: {count} records ({} micro_extract train rows) -> {}",
        micro_extract_train(&records).len(),
        out.display()
    );
    Ok(())
}

fn classification_records() -> Result<Vec<CorpusRecord>, EvalError> {
    let fixtures = default_fixtures()?;
    Ok(fixtures
        .classification
        .into_iter()
        .map(|fixture| {
            CorpusRecord::new(
                format!("cls:{}", fixture.id),
                "classification",
                "classification",
                "Classify the engineering task into one Cortex TaskClass and ModelTier.",
                fixture.task,
                format!(
                    "{{\"class\":\"{}\",\"tier\":\"{}\"}}",
                    class_name(fixture.gold_class),
                    tier_name(fixture.gold_tier)
                ),
                "crates/cortex-eval/fixtures/classification.json",
            )
            .into_holdout()
        })
        .collect())
}

fn extraction_records() -> Result<Vec<CorpusRecord>, EvalError> {
    let fixtures = default_fixtures()?;
    Ok(fixtures
        .extraction
        .into_iter()
        .map(|fixture| {
            CorpusRecord::new(
                format!("ext:{}", fixture.id),
                "extraction",
                "classification",
                "Extract the action, files, and symbols as closed JSON. Invent nothing.",
                fixture.text,
                serde_json::json!({
                    "action": fixture.gold.action,
                    "files": fixture.gold.files,
                    "symbols": fixture.gold.symbols,
                })
                .to_string(),
                "crates/cortex-eval/fixtures/extraction.json",
            )
            .into_holdout()
        })
        .collect())
}

fn compression_records() -> Result<Vec<CorpusRecord>, EvalError> {
    let fixtures = default_fixtures()?;
    Ok(fixtures
        .compression
        .into_iter()
        .map(|fixture| {
            let evidence = fixture
                .evidence
                .iter()
                .map(|item| format!("[{}] {}: {}", item.id, item.source, item.content))
                .collect::<Vec<_>>()
                .join("\n");
            let cites = fixture
                .must_cite
                .iter()
                .map(|id| format!("cite {id}"))
                .collect::<Vec<_>>()
                .join("; ");
            CorpusRecord::new(
                format!("cmp:{}", fixture.id),
                "compression",
                "digest",
                "Write a digest that cites every required evidence id. Do not invent sources.",
                format!("{}\n\n{evidence}", fixture.task),
                cites,
                "crates/cortex-eval/fixtures/compression.json",
            )
            .into_holdout()
        })
        .collect())
}

fn class_name(class: cortex_router::TaskClass) -> &'static str {
    match class {
        cortex_router::TaskClass::Deterministic => "deterministic",
        cortex_router::TaskClass::RepositoryAnalysis => "repository_analysis",
        cortex_router::TaskClass::StructuredExtraction => "structured_extraction",
        cortex_router::TaskClass::ContextCompression => "context_compression",
        cortex_router::TaskClass::AdvisoryDraft => "advisory_draft",
        cortex_router::TaskClass::Implementation => "implementation",
        cortex_router::TaskClass::Security => "security",
        cortex_router::TaskClass::Authentication => "authentication",
        cortex_router::TaskClass::Concurrency => "concurrency",
        cortex_router::TaskClass::Migration => "migration",
        cortex_router::TaskClass::Release => "release",
        cortex_router::TaskClass::Deployment => "deployment",
        cortex_router::TaskClass::Publication => "publication",
        cortex_router::TaskClass::Ambiguous => "ambiguous",
    }
}

fn tier_name(tier: cortex_router::ModelTier) -> &'static str {
    match tier {
        cortex_router::ModelTier::None => "none",
        cortex_router::ModelTier::LocalSmall => "local_small",
        cortex_router::ModelTier::LocalMedium => "local_medium",
        cortex_router::ModelTier::UpstreamStrong => "upstream_strong",
    }
}

fn sequence_step_records() -> Result<Vec<CorpusRecord>, EvalError> {
    let mut records = Vec::new();
    for template in cortex_sequences::templates() {
        let graph = cortex_sequences::instantiate_template(
            template.id,
            &format!("corpus-{}", template.id),
            template.title,
        )
        .map_err(|error| EvalError::Fixture(error.to_string()))?;
        for node in &graph.nodes {
            if node.config.get("role").and_then(serde_json::Value::as_str) != Some("workflow_step")
            {
                continue;
            }
            if matches!(node.kind, cortex_domain::NodeKind::Terminal) {
                continue;
            }
            let packet = cortex_sequences::active_step_packet(&graph, &node.id, &[])
                .map_err(|error| EvalError::Fixture(error.to_string()))?;
            records.push(CorpusRecord::new(
                format!("seq:{}:{}", template.id, node.id),
                "sequence-step",
                "digest",
                "Follow only this Cortex sequence step. Do not load other steps.",
                format!(
                    "{}\nrequiredEvidence: {:?}",
                    packet.instruction, packet.required_evidence
                ),
                format!(
                    "complete step {} when {} and escalate via {:?}",
                    packet.node_id,
                    packet.completion_criteria.join("; "),
                    packet.escalation_edges
                ),
                format!("crates/cortex-sequences/templates/{}.md", template.id),
            ));
        }
    }
    Ok(records)
}

fn mechanism_records() -> Vec<CorpusRecord> {
    vec![
        CorpusRecord::new(
            "mech:t3-silent-archive",
            "mechanism-index",
            "digest",
            "Name every silent-miss mechanism present in the evidence. Use exact identifiers.",
            "pub enabled: bool\nmax_entries: usize\nmax_entry_bytes: u64\n#[cfg(feature = \"archives\")]\nfn safe_virtual_path()",
            "mechanism: enable-flag -- field `enabled`\nmechanism: size-limit -- max_entry_bytes\nmechanism: entry-count -- max_entries\nmechanism: feature-gate -- cfg(feature = \"archives\")\nmechanism: path-skip -- name `safe_virtual_path`",
            "crates/cortex-eval/fixtures/mechanism-index.md",
        )
        .into_holdout(),
        CorpusRecord::new(
            "mech:t2-multiline",
            "mechanism-index",
            "digest",
            "Name the block-join and quiet mechanisms present in the evidence.",
            "struct Block { end_line: usize }\nfn finish_block()\nfn quiet_match()",
            "mechanism: block-type -- struct `Block`\nmechanism: join-condition -- end_line\nmechanism: flush -- call `finish_block`\nmechanism: quiet-path -- quiet_match",
            "crates/cortex-eval/fixtures/mechanism-index.md",
        )
        .into_holdout(),
    ]
}

fn manifest_for(records: &[CorpusRecord], micro_extract_train_records: usize) -> CorpusManifest {
    CorpusManifest {
        training_source: TRAINING_SOURCE,
        license: LICENSE,
        records: records.len(),
        by_task: counts(records.iter().map(|record| record.task.as_str())),
        by_role: counts(records.iter().map(|record| record.target_role.as_str())),
        by_split: counts(records.iter().map(|record| record.split.as_str())),
        micro_extract_train_records,
    }
}

fn counts<'a>(names: impl Iterator<Item = &'a str>) -> Vec<CountRow> {
    let mut tallies = std::collections::BTreeMap::new();
    for name in names {
        *tallies.entry(name.to_owned()).or_insert(0) += 1;
    }
    tallies
        .into_iter()
        .map(|(name, count)| CountRow { name, count })
        .collect()
}

fn readme() -> String {
    "# Cortex fine-tune corpus\n\n\
     Generated by `cargo run -p cortex-eval -- corpus`.\n\n\
     These records are **Cortex-original**: eval gold, generated extraction \
     rows, typed sequence packets, and mechanism labels. Superpowers is a \
     token/quality baseline we measure against. We rewrote the workflows as \
     typed graphs; we do **not** train on upstream `SKILL.md` bodies and we do \
     not fork Superpowers.\n\n\
     ## Files\n\n\
     - `train/sft.jsonl` — generated train only. Gold fixtures never land here.\n\
     - `dev/sft.jsonl` — a hashed slice of generated train for early stopping.\n\
     - workspace `eval/public/` — pointer at `crates/cortex-eval/fixtures/`.\n\
     - workspace `eval/private/` — reserved hidden suite, unused by heuristics.\n\
     - `manifest.json` — counts by task, role, and split.\n\n\
     ## Splits\n\n\
     `split=holdout` marks records derived from gold a gate is scored on \
     (`fixtures/*.json`). They stay in eval and never enter `train/`. \
     `split=train` marks generated micro-extraction rows and sequence step \
     packets. Mechanism labels and gold fixtures stay holdout. The writer \
     refuses a train file that hashes or copies a gold family.\n\n\
     Split by `targetRole` when fine-tuning:\n\n\
     - `classification` -> Qwen3-8B NPU classifier (and its extraction gate)\n\
     - `micro_extract` -> Qwen3-0.6B extractor candidate\n\
     - `digest` -> `qwen3.5:9b` citation-preserving digest / step packets\n\n\
     `trainingSource` is always `cortex-original`. `license` is MIT OR Apache-2.0.\n"
        .to_owned()
}

#[cfg(test)]
#[path = "corpus_tests.rs"]
mod tests;
