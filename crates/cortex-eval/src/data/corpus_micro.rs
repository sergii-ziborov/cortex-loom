//! `micro_extract` corpus records, split-labelled at the source.
//!
//! Both families render the same instruction and the same input shape, so the
//! only thing separating the shipped fixtures from the generated split is the
//! `split` label — which is exactly the property a fine-tune filters on.

use crate::EvalError;
use crate::corpus::CorpusRecord;
use crate::fixtures::micro_extraction_fixtures;

const EXTRACT_INSTRUCTION: &str = "Extract only the allowed fields. Every value must be a literal substring of the verified input.";
const REJECT_INSTRUCTION: &str = "Reject any extraction that invents a value, duplicates a value, or adds a field outside the allowed list.";
const REJECT_OUTPUT: &str =
    "{\"reject\":true,\"reason\":\"not a literal in verified input or outside allowed fields\"}";

const HOLDOUT_SOURCE: &str = "crates/cortex-eval/fixtures/micro-extraction.json";
const TRAIN_SOURCE: &str = "crates/cortex-eval/src/micro_cases.rs";

fn extract_record(
    id: String,
    allowed: &str,
    verified_input: &str,
    gold: &str,
    source: &str,
) -> CorpusRecord {
    CorpusRecord::new(
        id,
        "micro-extraction",
        "micro_extract",
        EXTRACT_INSTRUCTION,
        format!("allowedFields: {allowed}\nverifiedInput:\n{verified_input}"),
        gold,
        source,
    )
}

fn reject_record(
    id: String,
    allowed: &str,
    verified_input: &str,
    candidate: &str,
    source: &str,
) -> CorpusRecord {
    CorpusRecord::new(
        id,
        "micro-extraction-reject",
        "micro_extract",
        REJECT_INSTRUCTION,
        format!(
            "allowedFields: {allowed}\nverifiedInput:\n{verified_input}\ncandidate:\n{candidate}"
        ),
        REJECT_OUTPUT,
        source,
    )
}

/// The shipped adversarial fixtures. These are the 0.6B **holdout**: emitted so
/// the corpus stays a complete picture, and labelled so a fine-tune filtered on
/// `split` cannot consume them.
pub(crate) fn holdout_records() -> Result<Vec<CorpusRecord>, EvalError> {
    let mut records = Vec::new();
    for fixture in micro_extraction_fixtures()? {
        let allowed = fixture.allowed_fields.join(", ");
        records.push(
            extract_record(
                format!("micro:{}", fixture.id),
                &allowed,
                &fixture.verified_input,
                &fixture.gold.to_string(),
                HOLDOUT_SOURCE,
            )
            .into_holdout(),
        );
        for (index, rejected) in fixture.rejected_outputs.iter().enumerate() {
            records.push(
                reject_record(
                    format!("micro-reject:{}:{index}", fixture.id),
                    &allowed,
                    &fixture.verified_input,
                    &rejected.to_string(),
                    HOLDOUT_SOURCE,
                )
                .into_holdout(),
            );
        }
    }
    Ok(records)
}

/// The generated train split. Same contract, disjoint inputs, `split: train`.
pub(crate) fn train_records() -> Result<Vec<CorpusRecord>, EvalError> {
    let mut records = Vec::new();
    for case in crate::micro_train::build()? {
        let allowed = case.allowed_fields.join(", ");
        records.push(extract_record(
            format!("micro-train:{}", case.id),
            &allowed,
            &case.verified_input,
            &case.gold.to_string(),
            TRAIN_SOURCE,
        ));
        if let Some(rejected) = &case.rejected {
            records.push(reject_record(
                format!("micro-train-reject:{}", case.id),
                &allowed,
                &case.verified_input,
                &rejected.to_string(),
                TRAIN_SOURCE,
            ));
        }
    }
    Ok(records)
}
