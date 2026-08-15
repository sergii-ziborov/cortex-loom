use super::*;

#[test]
fn generated_train_does_not_leak_eval_gold() {
    let records = build().expect("corpus");
    let train: Vec<&CorpusRecord> = records
        .iter()
        .filter(|record| record.split == SPLIT_TRAIN)
        .collect();
    let eval: Vec<&CorpusRecord> = records
        .iter()
        .filter(|record| record.split == SPLIT_HOLDOUT)
        .collect();
    crate::leakage::refuse_if_leaky(&train, &eval).expect("train must not overlap eval gold");
}

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
fn the_shipped_micro_fixtures_never_enter_the_micro_train_file() {
    let records = build().expect("corpus");
    let train = micro_extract_train(&records);
    assert!(train.len() >= 300, "train split too small: {}", train.len());
    let holdout_inputs: Vec<String> = crate::fixtures::micro_extraction_fixtures()
        .expect("holdout")
        .into_iter()
        .map(|fixture| fixture.verified_input)
        .collect();
    for record in &train {
        assert_eq!(record.split, SPLIT_TRAIN);
        assert_eq!(record.target_role, "micro_extract");
        assert!(
            !holdout_inputs
                .iter()
                .any(|input| record.input.contains(input.as_str())),
            "{} carries a holdout verified input",
            record.id
        );
    }
    assert!(
        records
            .iter()
            .any(|record| record.split == SPLIT_HOLDOUT && record.target_role == "micro_extract")
    );
}

#[test]
fn write_to_emits_jsonl_readme_and_manifest() {
    let dir = std::env::temp_dir().join(format!("cortex-corpus-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let count = write_to(&dir).expect("write");
    let body = fs::read_to_string(dir.join("train/sft.jsonl")).expect("train jsonl");
    let dev = fs::read_to_string(dir.join("dev/sft.jsonl")).expect("dev jsonl");
    assert_eq!(body.lines().count() + dev.lines().count(), count);
    assert!(!body.contains("\"split\":\"holdout\""));
    assert!(!dev.contains("\"split\":\"holdout\""));
    assert!(body.contains("micro_extract"));
    assert!(
        !dir.join("eval").exists(),
        "eval suites are not under corpora/"
    );
    let readme = fs::read_to_string(dir.join("README.md")).expect("readme");
    assert!(readme.contains("cortex-original"));
    assert!(!readme.to_ascii_lowercase().contains("using-superpowers"));
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).expect("manifest"))
            .expect("manifest json");
    assert!(manifest["records"].as_u64().unwrap() >= count as u64);
    assert!(manifest["microExtractTrainRecords"].as_u64().unwrap() > 0);
    let _ = fs::remove_dir_all(&dir);
}
