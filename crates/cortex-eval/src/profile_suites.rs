use crate::backend::EvalBackend;
use crate::comparators::{citation_metrics, classification_outcome, token_delta};
use crate::fixtures::{ClassificationFixture, CompressionFixture, ExtractionFixture};
use crate::metrics::{
    ClassificationSample, CompressionSample, ExtractionMatches, ExtractionSample,
};
use crate::prompts::{
    EvidenceBlock, classification_request, compression_request, extraction_request,
    parse_compression, parse_extraction, parse_tier,
};

pub(crate) fn run_classification(
    backend: &dyn EvalBackend,
    profile: &str,
    fixture: &ClassificationFixture,
) -> ClassificationSample {
    let call = backend.structured(&classification_request(profile, &fixture.task));
    let (observed, latency_ms, error) = match call {
        Ok(timed) => match parse_tier(&timed.content) {
            Ok(tier) => (Some(tier), timed.latency_ms, None),
            Err(parse_error) => (None, timed.latency_ms, Some(parse_error)),
        },
        Err(error) => (None, 0, Some(error)),
    };
    ClassificationSample {
        fixture_id: fixture.id.clone(),
        gold_tier: fixture.gold_tier,
        observed_tier: observed,
        schema_valid: observed.is_some(),
        outcome: classification_outcome(fixture.gold_tier, observed),
        latency_ms,
        error,
    }
}

/// Sort, dedup, and strip formatting noise (whitespace plus surrounding
/// backticks or quotes). Content differences â€” invented or missing entries â€”
/// still fail the comparison.
fn normalized(values: &[String]) -> Vec<String> {
    let mut sorted: Vec<String> = values
        .iter()
        .map(|value| {
            value
                .trim()
                .trim_matches(|ch| matches!(ch, '`' | '"' | '\''))
                .to_owned()
        })
        .collect();
    sorted.sort();
    sorted.dedup();
    sorted
}

pub(crate) fn run_extraction(
    backend: &dyn EvalBackend,
    profile: &str,
    fixture: &ExtractionFixture,
) -> ExtractionSample {
    let call = backend.structured(&extraction_request(profile, &fixture.text));
    let no_match = ExtractionMatches {
        action: false,
        files: false,
        symbols: false,
    };
    let (schema_valid, matches, latency_ms, error) = match call {
        Ok(timed) => match parse_extraction(&timed.content) {
            Ok(parsed) => {
                let matches = ExtractionMatches {
                    action: parsed.action.eq_ignore_ascii_case(&fixture.gold.action),
                    files: normalized(&parsed.files) == normalized(&fixture.gold.files),
                    symbols: normalized(&parsed.symbols) == normalized(&fixture.gold.symbols),
                };
                (true, matches, timed.latency_ms, None)
            }
            Err(parse_error) => (false, no_match, timed.latency_ms, Some(parse_error)),
        },
        Err(error) => (false, no_match, 0, Some(error)),
    };
    ExtractionSample {
        fixture_id: fixture.id.clone(),
        schema_valid,
        matches,
        latency_ms,
        error,
    }
}

pub(crate) fn run_compression(
    backend: &dyn EvalBackend,
    profile: &str,
    fixture: &CompressionFixture,
) -> CompressionSample {
    let blocks: Vec<EvidenceBlock<'_>> = fixture
        .evidence
        .iter()
        .map(|evidence| EvidenceBlock {
            id: &evidence.id,
            source: &evidence.source,
            content: &evidence.content,
        })
        .collect();
    let call = backend.structured(&compression_request(profile, &fixture.task, &blocks));
    let supplied: Vec<String> = fixture
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect();
    let contents: Vec<&str> = fixture
        .evidence
        .iter()
        .map(|evidence| evidence.content.as_str())
        .collect();
    let (schema_valid, citations, delta, latency_ms, error) = match call {
        Ok(timed) => match parse_compression(&timed.content) {
            Ok(parsed) => {
                let citations = citation_metrics(
                    &supplied,
                    &fixture.must_cite,
                    &parsed.summary,
                    &parsed.evidence_ids,
                );
                let delta = token_delta(&contents, &parsed.summary);
                (true, Some(citations), Some(delta), timed.latency_ms, None)
            }
            Err(parse_error) => (false, None, None, timed.latency_ms, Some(parse_error)),
        },
        Err(error) => (false, None, None, 0, Some(error)),
    };
    CompressionSample {
        fixture_id: fixture.id.clone(),
        schema_valid,
        citations,
        token_delta: delta,
        latency_ms,
        error,
    }
}
