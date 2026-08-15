use cortex_llm::MicroExtractRequest;

use crate::backend::EvalBackend;
use crate::comparators::{
    MicroExtractionOutcome, citation_metrics, classification_outcome, micro_extraction_outcome,
    token_delta,
};
use crate::fixtures::{
    ClassificationFixture, CompressionFixture, ExtractionFixture, MicroExtractionFixture,
};
use crate::metrics::{
    ClassificationSample, CompressionSample, ExtractionMatches, ExtractionSample,
    MicroExtractionSample,
};
use crate::prompts::{
    EvidenceBlock, classification_request, compression_request, extraction_request,
    micro_extraction_request, parse_compression, parse_extraction, parse_tier,
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

/// Ask one closed-schema literal extraction and score it under the exact
/// contract the provider enforces. The reply is never repaired: fences, prose
/// and truncated JSON are schema failures here because they are schema
/// failures at the product boundary too.
pub(crate) fn run_micro_extraction(
    backend: &dyn EvalBackend,
    profile: &str,
    fixture: &MicroExtractionFixture,
) -> MicroExtractionSample {
    let fields: Vec<&str> = fixture.allowed_fields.iter().map(String::as_str).collect();
    let request = match MicroExtractRequest::new(&fixture.verified_input, &fields) {
        Ok(request) => request,
        Err(error) => {
            return MicroExtractionSample {
                fixture_id: fixture.id.clone(),
                answered: false,
                outcome: MicroExtractionOutcome::default(),
                reply: String::new(),
                latency_ms: 0,
                error: Some(error.to_string()),
            };
        }
    };
    let (content, latency_ms, answered, error) =
        match backend.structured(&micro_extraction_request(profile, &request)) {
            Ok(timed) => (timed.content, timed.latency_ms, true, None),
            Err(error) => (String::new(), 0, false, Some(error)),
        };
    let outcome = micro_extraction_outcome(&request, &fixture.gold, &content);
    let reply: String = content
        .trim()
        .replace('\n', " ")
        .chars()
        .take(240)
        .collect();
    let error = error.or_else(|| {
        (!outcome.schema_valid).then(|| "the provider would refuse this reply".to_owned())
    });
    MicroExtractionSample {
        fixture_id: fixture.id.clone(),
        answered,
        outcome,
        reply,
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
    let (schema_valid, citations, delta, claim_ok, claim_errors, latency_ms, error) = match call {
        Ok(timed) => match parse_compression(&timed.content) {
            Ok(parsed) => {
                let citations = citation_metrics(
                    &supplied,
                    &fixture.must_cite,
                    &parsed.summary,
                    &parsed.evidence_ids,
                );
                let delta = token_delta(&contents, &parsed.summary);
                let evidence: Vec<cortex_llm::ClaimEvidence<'_>> = fixture
                    .evidence
                    .iter()
                    .map(|item| cortex_llm::ClaimEvidence {
                        id: item.id.as_str(),
                        source: item.source.as_str(),
                        content: item.content.as_str(),
                    })
                    .collect();
                let (claim_ok, claim_errors) =
                    if fixture.must_preserve.is_empty() && parsed.claims.is_empty() {
                        (None, None)
                    } else {
                        let check = cortex_llm::verify_claims(&parsed.claims, &evidence);
                        let missing =
                            cortex_llm::missing_required(&fixture.must_preserve, &parsed.claims);
                        let mut errors = check.errors;
                        for (subject, relation) in missing {
                            errors.push(format!("missing required claim {subject}/{relation}"));
                        }
                        (Some(errors.is_empty()), Some(errors))
                    };
                (
                    true,
                    Some(citations),
                    Some(delta),
                    claim_ok,
                    claim_errors,
                    timed.latency_ms,
                    None,
                )
            }
            Err(parse_error) => (
                false,
                None,
                None,
                None,
                None,
                timed.latency_ms,
                Some(parse_error),
            ),
        },
        Err(error) => (false, None, None, None, None, 0, Some(error)),
    };
    CompressionSample {
        fixture_id: fixture.id.clone(),
        schema_valid,
        citations,
        token_delta: delta,
        claim_ok,
        claim_errors,
        latency_ms,
        error,
    }
}
