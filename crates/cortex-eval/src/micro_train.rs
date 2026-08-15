//! Deterministic Cortex-original **train** split for the `micro_extract` role.
//!
//! `fixtures/micro-extraction.json` is the **holdout** the 0.6B gate is scored
//! on. Nothing generated here may reproduce one of those cases, so [`build`]
//! fails if a generated row reuses a holdout `verifiedInput`. Rows are composed
//! in [`crate::micro_cases`] from the repository's own vocabulary — no upstream
//! skill text is involved.
//!
//! Every row is validated through [`MicroExtractRequest`] exactly as the
//! provider validates a live reply, so a row the product path would reject
//! cannot enter the corpus.

use cortex_llm::MicroExtractRequest;

use crate::EvalError;
use crate::fixtures::micro_extraction_fixtures;
use crate::micro_cases::generate;

pub use crate::micro_cases::MicroTrainCase;

/// Generate the train split and prove every row against the live contract.
///
/// # Errors
///
/// Fails when a generated row is not accepted by [`MicroExtractRequest`], when
/// a rejected candidate would in fact validate, when two rows share an id or a
/// verified input, or when a row reuses a holdout `verifiedInput`.
pub fn build() -> Result<Vec<MicroTrainCase>, EvalError> {
    let cases = generate();
    verify(&cases)?;
    Ok(cases)
}

fn verify(cases: &[MicroTrainCase]) -> Result<(), EvalError> {
    let holdout = micro_extraction_fixtures()?;
    let mut seen_ids = std::collections::BTreeSet::new();
    let mut seen_inputs = std::collections::BTreeSet::new();
    for case in cases {
        if !seen_ids.insert(case.id.as_str()) {
            return Err(EvalError::Fixture(format!("train repeats id {}", case.id)));
        }
        if !seen_inputs.insert(case.verified_input.as_str()) {
            return Err(EvalError::Fixture(format!(
                "{} repeats a verified input",
                case.id
            )));
        }
        if holdout
            .iter()
            .any(|fixture| fixture.verified_input == case.verified_input)
        {
            return Err(EvalError::Fixture(format!(
                "{} reuses a holdout verified input",
                case.id
            )));
        }
        let fields: Vec<&str> = case.allowed_fields.iter().map(String::as_str).collect();
        let request = MicroExtractRequest::new(&case.verified_input, &fields)
            .map_err(|error| EvalError::Fixture(format!("{}: {error}", case.id)))?;
        request
            .validate_output(&case.gold)
            .map_err(|error| EvalError::Fixture(format!("{} gold: {error}", case.id)))?;
        if let Some(rejected) = &case.rejected
            && request.validate_output(rejected).is_ok()
        {
            return Err(EvalError::Fixture(format!(
                "{} rejected candidate is actually valid",
                case.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build, micro_extraction_fixtures};

    #[test]
    fn the_train_split_is_large_disjoint_and_covers_every_required_category() {
        let cases = build().expect("train split");
        assert!(cases.len() >= 250, "too few train rows: {}", cases.len());
        for prefix in [
            "train-ident-env-",
            "train-files-",
            "train-json-keys-",
            "train-labels-",
            "train-unicode-",
            "train-empty-",
            "train-injection-",
            "train-repeat-",
            "train-unused-field-",
            "train-routing-bait-",
            "train-unicode-symbol-",
            "train-key-file-",
            "train-crate-symbols-",
            "train-single-env-",
            "train-three-files-",
            "train-prose-noun-",
            "train-labels-empty-",
        ] {
            assert!(
                cases.iter().any(|case| case.id.starts_with(prefix)),
                "missing category {prefix}"
            );
        }
        let rejects = cases.iter().filter(|case| case.rejected.is_some()).count();
        assert!(rejects >= 60, "too few judge rows: {rejects}");
        assert!(
            rejects * 2 < cases.len(),
            "judge rows must stay a minority: {rejects} of {}",
            cases.len()
        );
        // Empty-gold rows teach omission, which is where small models invent.
        assert!(
            cases
                .iter()
                .filter(|case| case.gold.as_object().is_some_and(serde_json::Map::is_empty))
                .count()
                >= 12
        );
        let holdout = micro_extraction_fixtures().expect("holdout");
        for fixture in &holdout {
            assert!(
                !cases
                    .iter()
                    .any(|case| case.verified_input == fixture.verified_input),
                "{} leaked into the train split",
                fixture.id
            );
        }
    }
}
