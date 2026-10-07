use std::fs;
use std::path::Path;

use cortex_context::{
    ChangePlan, ChangePlanStatus, EvidenceFacet, EvidenceItem, EvidenceLocator, EvidencePriority,
    EvidenceState, SourceTarget,
};

use super::{prompt_sources, validate_draft};

fn fixture(root: &Path) -> (ChangePlan, Vec<EvidenceItem>) {
    let path = root.join("src");
    fs::create_dir_all(&path).unwrap();
    let source = "pub fn answer() -> u32 {\n    41\n}\n";
    fs::write(path.join("lib.rs"), source).unwrap();
    let mut item = EvidenceItem::new(
        "ev_source",
        "src/lib.rs:1",
        source,
        EvidencePriority::High,
        EvidenceState::Verified,
    );
    item.locator = Some(EvidenceLocator {
        path: Some("src/lib.rs".to_owned()),
        start_line: Some(1),
        end_line: Some(3),
        ..EvidenceLocator::default()
    });
    (
        ChangePlan {
            status: ChangePlanStatus::ReadyForUpstreamReview,
            source_targets: vec![SourceTarget {
                path: "src/lib.rs".to_owned(),
                start_line: 1,
                end_line: 3,
                evidence_id: "ev_source".to_owned(),
                facet: EvidenceFacet::Definition,
            }],
            test_targets: Vec::new(),
            missing_facets: Vec::new(),
            next_expansion: None,
            review_required: true,
        },
        vec![item],
    )
}

#[test]
fn exact_anchored_replacement_is_advisory_and_read_only() {
    let root = std::env::temp_dir().join(format!("cortex-coding-advice-ok-{}", std::process::id()));
    let (plan, items) = fixture(&root);
    let sources = prompt_sources(&plan, &items);
    let response = serde_json::json!({
        "edits": [{
            "evidenceId": "ev_source",
            "find": "pub fn answer() -> u32 {\n    41\n}",
            "replace": "pub fn answer() -> u32 {\n    42\n}",
            "rationale": "Matches the requested value"
        }]
    });
    let draft = validate_draft(&response.to_string(), &sources, &root).unwrap();
    assert_eq!(draft.edits.len(), 1);
    assert_eq!(draft.discarded, 0);
    assert_eq!(
        fs::read_to_string(root.join("src/lib.rs")).unwrap(),
        "pub fn answer() -> u32 {\n    41\n}\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invented_citation_and_nonmatching_source_fail_closed() {
    let root =
        std::env::temp_dir().join(format!("cortex-coding-advice-bad-{}", std::process::id()));
    let (plan, items) = fixture(&root);
    let sources = prompt_sources(&plan, &items);
    let mut response = serde_json::json!({
        "edits": [{
            "evidenceId": "ev_invented",
            "find": "pub fn answer() -> u32 {\n    41\n}",
            "replace": "pub fn answer() -> u32 {\n    42\n}",
            "rationale": "One exact change"
        }]
    });
    assert!(validate_draft(&response.to_string(), &sources, &root).is_err());
    response["edits"][0]["evidenceId"] = "ev_source".into();
    response["edits"][0]["find"] = "pub fn imaginary() -> u32 {\n    41\n}".into();
    assert!(validate_draft(&response.to_string(), &sources, &root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn changed_live_source_rejects_stale_preview() {
    let root =
        std::env::temp_dir().join(format!("cortex-coding-advice-stale-{}", std::process::id()));
    let (plan, items) = fixture(&root);
    let sources = prompt_sources(&plan, &items);
    fs::write(root.join("src/lib.rs"), "pub fn answer() -> u32 { 99 }").unwrap();
    let response = serde_json::json!({
        "edits": [{
            "evidenceId": "ev_source",
            "find": "pub fn answer() -> u32 {\n    41\n}",
            "replace": "pub fn answer() -> u32 {\n    42\n}",
            "rationale": "One exact change"
        }]
    });
    assert!(validate_draft(&response.to_string(), &sources, &root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_suggestion_is_collapsed_but_conflicting_replacement_is_rejected() {
    let root = std::env::temp_dir().join(format!(
        "cortex-coding-advice-duplicate-{}",
        std::process::id()
    ));
    let (plan, items) = fixture(&root);
    let sources = prompt_sources(&plan, &items);
    let edit = serde_json::json!({
        "evidenceId": "ev_source",
        "find": "pub fn answer() -> u32 {\n    41\n}",
        "replace": "pub fn answer() -> u32 {\n    42\n}",
        "rationale": "Matches the requested value"
    });
    let mut response = serde_json::json!({
        "edits": [edit.clone(), edit]
    });
    let draft = validate_draft(&response.to_string(), &sources, &root).unwrap();
    assert_eq!(draft.edits.len(), 1);
    assert_eq!(draft.duplicates, 1);

    response["edits"][1]["replace"] = "pub fn answer() -> u32 {\n    43\n}".into();
    assert_eq!(
        validate_draft(&response.to_string(), &sources, &root).unwrap_err(),
        "draft edits overlap"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn valid_anchor_survives_an_unverifiable_suggestion() {
    let root = std::env::temp_dir().join(format!(
        "cortex-coding-advice-partial-{}",
        std::process::id()
    ));
    let (plan, items) = fixture(&root);
    let sources = prompt_sources(&plan, &items);
    let response = serde_json::json!({
        "edits": [
            {
                "evidenceId": "ev_source",
                "find": "pub fn answer() -> u32 {\n    41\n}",
                "replace": "pub fn answer() -> u32 {\n    42\n}",
                "rationale": "One exact change"
            },
            {
                "evidenceId": "ev_source",
                "find": "pub fn imaginary() -> u32 {\n    41\n}",
                "replace": "pub fn imaginary() -> u32 {\n    42\n}",
                "rationale": "Unsupported change"
            }
        ]
    });
    let draft = validate_draft(&response.to_string(), &sources, &root).unwrap();
    assert_eq!(draft.edits.len(), 1);
    assert_eq!(draft.discarded, 1);
    assert_eq!(
        draft.first_discard_reason.as_deref(),
        Some("draft find text is not unique in cited source")
    );
    fs::remove_dir_all(root).unwrap();
}
