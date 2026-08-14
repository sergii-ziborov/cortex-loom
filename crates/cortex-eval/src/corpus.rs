//! Deterministic fine-tune corpus from Cortex-owned fixtures and sequences.
//!
//! Superpowers is a measured baseline, not a training source. Records are
//! Cortex-original: eval gold, typed sequence packets, and mechanism labels.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::EvalError;
use crate::fixtures::{default_fixtures, micro_extraction_fixtures};

const LICENSE: &str = "MIT OR Apache-2.0";
const TRAINING_SOURCE: &str = "cortex-original";

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
}

impl CorpusRecord {
    fn new(
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
        }
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

/// Build every Cortex-owned training record.
pub fn build() -> Result<Vec<CorpusRecord>, EvalError> {
    let mut records = classification_records()?;
    records.extend(extraction_records()?);
    records.extend(micro_records()?);
    records.extend(compression_records()?);
    records.extend(sequence_step_records()?);
    records.extend(mechanism_records());
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

/// Write JSONL plus a short README. Does not pull models or Superpowers text.
pub fn write_to(out_dir: &Path) -> Result<usize, EvalError> {
    let records = build()?;
    fs::create_dir_all(out_dir).map_err(|error| EvalError::Io(error.to_string()))?;
    let body = records
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| EvalError::Json(error.to_string()))?
        .join("\n")
        + "\n";
    fs::write(out_dir.join("sft.jsonl"), body).map_err(|error| EvalError::Io(error.to_string()))?;
    fs::write(out_dir.join("README.md"), readme())
        .map_err(|error| EvalError::Io(error.to_string()))?;
    let manifest = serde_json::to_string_pretty(&manifest_for(&records))
        .map_err(|error| EvalError::Json(error.to_string()))?;
    fs::write(out_dir.join("manifest.json"), format!("{manifest}\n"))
        .map_err(|error| EvalError::Io(error.to_string()))?;
    Ok(records.len())
}

pub fn write_cli() -> Result<(), EvalError> {
    let out = default_out_dir();
    let count = write_to(&out)?;
    println!("cortex-eval corpus: {count} records -> {}", out.display());
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
        })
        .collect())
}

fn micro_records() -> Result<Vec<CorpusRecord>, EvalError> {
    let mut records = Vec::new();
    for fixture in micro_extraction_fixtures()? {
        let allowed = fixture.allowed_fields.join(", ");
        records.push(CorpusRecord::new(
            format!("micro:{}", fixture.id),
            "micro-extraction",
            "micro_extract",
            "Extract only the allowed fields. Every value must be a literal substring of the verified input.",
            format!(
                "allowedFields: {allowed}\nverifiedInput:\n{}",
                fixture.verified_input
            ),
            fixture.gold.to_string(),
            "crates/cortex-eval/fixtures/micro-extraction.json",
        ));
        for (index, rejected) in fixture.rejected_outputs.iter().enumerate() {
            records.push(CorpusRecord::new(
                format!("micro-reject:{}:{index}", fixture.id),
                "micro-extraction-reject",
                "micro_extract",
                "Reject any extraction that invents a value, duplicates a value, or adds a field outside the allowed list.",
                format!(
                    "allowedFields: {allowed}\nverifiedInput:\n{}\ncandidate:\n{rejected}",
                    fixture.verified_input
                ),
                "{\"reject\":true,\"reason\":\"not a literal in verified input or outside allowed fields\"}"
                    .to_owned(),
                "crates/cortex-eval/fixtures/micro-extraction.json",
            ));
        }
    }
    Ok(records)
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
            "mechanism: enable-flag — field `enabled`\nmechanism: size-limit — max_entry_bytes\nmechanism: entry-count — max_entries\nmechanism: feature-gate — cfg(feature = \"archives\")\nmechanism: path-skip — name `safe_virtual_path`",
            "crates/cortex-weavatrix/src/context.rs",
        ),
        CorpusRecord::new(
            "mech:t2-multiline",
            "mechanism-index",
            "digest",
            "Name the block-join and quiet mechanisms present in the evidence.",
            "struct Block { end_line: usize }\nfn finish_block()\nfn quiet_match()",
            "mechanism: block-type — struct `Block`\nmechanism: join-condition — end_line\nmechanism: flush — call `finish_block`\nmechanism: quiet-path — quiet_match",
            "crates/cortex-weavatrix/src/context.rs",
        ),
    ]
}

fn manifest_for(records: &[CorpusRecord]) -> CorpusManifest {
    CorpusManifest {
        training_source: TRAINING_SOURCE,
        license: LICENSE,
        records: records.len(),
        by_task: counts(records.iter().map(|record| record.task.as_str())),
        by_role: counts(records.iter().map(|record| record.target_role.as_str())),
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
     These records are **Cortex-original**: eval gold, typed sequence packets, \
     and mechanism labels. Superpowers is a token/quality baseline we measure \
     against. We rewrote the workflows as typed graphs; we do **not** train on \
     upstream `SKILL.md` bodies and we do not fork Superpowers.\n\n\
     Split by `targetRole` when fine-tuning:\n\n\
     - `classification` → Qwen3-8B NPU classifier (and its extraction gate)\n\
     - `micro_extract` → future Qwen3-0.6B NPU extractor (not installed)\n\
     - `digest` → `qwen3.5:9b` citation-preserving digest / step packets\n\n\
     `trainingSource` is always `cortex-original`. `license` is MIT OR Apache-2.0.\n"
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_is_cortex_owned_and_excludes_superpowers_bodies() {
        let records = build().expect("corpus");
        assert!(records.len() >= 60, "too few records: {}", records.len());
        assert!(records.iter().any(|record| record.task == "classification"));
        assert!(records.iter().any(|record| record.task == "extraction"));
        assert!(
            records
                .iter()
                .any(|record| record.task == "micro-extraction")
        );
        assert!(records.iter().any(|record| record.task == "compression"));
        assert!(records.iter().any(|record| record.task == "sequence-step"));
        assert!(
            records
                .iter()
                .any(|record| record.id == "mech:t2-multiline")
        );
        let inventory = include_str!("../../../config/model-inventory.json");
        assert!(inventory.contains("qwen3-8b-ovms-npu"));
        assert!(inventory.contains("xiyan-sql-7b-ollama"));
        assert!(inventory.contains("\"needed\": false"));
        for record in &records {
            assert_eq!(record.training_source, TRAINING_SOURCE);
            assert_eq!(record.license, LICENSE);
            assert!(
                matches!(
                    record.target_role.as_str(),
                    "classification" | "micro_extract" | "digest"
                ),
                "{} has unknown role {}",
                record.id,
                record.target_role
            );
            let blob = format!("{} {} {}", record.instruction, record.input, record.output);
            assert!(
                !blob.to_ascii_lowercase().contains("using-superpowers"),
                "{} leaked Superpowers bootstrap",
                record.id
            );
        }
    }

    #[test]
    fn write_to_emits_jsonl_readme_and_manifest() {
        let dir = std::env::temp_dir().join(format!("cortex-corpus-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let count = write_to(&dir).expect("write");
        let body = fs::read_to_string(dir.join("sft.jsonl")).expect("jsonl");
        assert_eq!(body.lines().count(), count);
        let readme = fs::read_to_string(dir.join("README.md")).expect("readme");
        assert!(readme.contains("cortex-original"));
        assert!(!readme.to_ascii_lowercase().contains("using-superpowers"));
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).expect("manifest"))
                .expect("manifest json");
        assert_eq!(manifest["records"], count);
        let _ = fs::remove_dir_all(&dir);
    }
}
