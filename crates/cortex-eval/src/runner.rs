//! Suite execution against one candidate profile.

use cortex_ollama::DevicePlacement;
use cortex_router::ModelTier;
use serde::Serialize;

use std::collections::BTreeMap;

use cortex_ollama::EmbedRequest;

use crate::backend::EvalBackend;
use crate::comparators::{citation_metrics, classification_outcome, token_delta};
use crate::fixtures::{
    ClassificationFixture, CompressionFixture, ExtractionFixture, FixtureSet, RetrievalFixtures,
};
use crate::metrics::{
    ClassificationAggregate, ClassificationSample, CompressionAggregate, CompressionSample,
    ExtractionAggregate, ExtractionMatches, ExtractionSample, LatencyStats, RetrievalAggregate,
    RetrievalSample, aggregate_classification, aggregate_compression, aggregate_extraction,
    aggregate_retrieval, latency_stats, ndcg_at_k, rank_by_similarity, recall_at_k,
    reciprocal_rank,
};
use crate::prompts::{
    EvidenceBlock, classification_request, compression_request, extraction_request,
    parse_compression, parse_extraction, parse_tier,
};
use crate::ranking::{Bm25Index, build_adjacency, graph_boost, rrf_fuse};
use crate::verdict::{CalibrationVerdict, judge, judge_retrieval};

const EMBED_BATCH: usize = 16;

// A flags struct: each suite is independently selectable.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuiteSelection {
    pub classification: bool,
    pub extraction: bool,
    pub compression: bool,
    pub retrieval: bool,
}

impl SuiteSelection {
    #[must_use]
    pub const fn all() -> Self {
        Self {
            classification: true,
            extraction: true,
            compression: true,
            retrieval: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvalProfile {
    /// Profile name registered in the Ollama client configuration.
    pub id: String,
    pub tier: ModelTier,
    /// Exact model tag; never substituted.
    pub model: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStatus {
    Evaluated,
    ModelAbsent,
    DiscoveryFailed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReport {
    pub profile_id: String,
    pub tier: ModelTier,
    pub model: String,
    pub status: ProfileStatus,
    pub digest: Option<String>,
    pub device: Option<DevicePlacement>,
    pub classification: Option<ClassificationAggregate>,
    pub extraction: Option<ExtractionAggregate>,
    pub compression: Option<CompressionAggregate>,
    pub latency: LatencyStats,
    pub verdict: CalibrationVerdict,
    pub classification_samples: Vec<ClassificationSample>,
    pub extraction_samples: Vec<ExtractionSample>,
    pub compression_samples: Vec<CompressionSample>,
}

#[derive(Debug, Clone)]
pub struct EmbeddingProfile {
    /// Profile name registered in the Ollama client configuration.
    pub id: String,
    /// Exact model tag; never substituted.
    pub model: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    /// Pure cosine ranking over embeddings.
    Embedding,
    /// Reciprocal-rank fusion of the embedding and BM25 rankings.
    Hybrid,
    /// Hybrid plus a structural neighbor bonus from declared relatedness.
    HybridGraph,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalModeReport {
    pub mode: RetrievalMode,
    pub aggregate: RetrievalAggregate,
    pub verdict: CalibrationVerdict,
    pub samples: Vec<RetrievalSample>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingReport {
    pub profile_id: String,
    pub model: String,
    pub status: ProfileStatus,
    pub digest: Option<String>,
    pub dimensions: Option<usize>,
    pub ranking_version: String,
    pub modes: Vec<RetrievalModeReport>,
    pub latency: LatencyStats,
    pub error: Option<String>,
}

impl EmbeddingReport {
    /// The gate opens when any mode passes.
    #[must_use]
    pub fn passing_mode(&self) -> Option<RetrievalMode> {
        self.modes
            .iter()
            .find(|mode| mode.verdict.pass)
            .map(|mode| mode.mode)
    }
}

/// Run the retrieval suite for one embedding profile in three modes: pure
/// embedding, hybrid (embedding + BM25 fused by reciprocal rank), and hybrid
/// with a structural graph boost. Each mode is judged against the
/// semantic-selection gate. Absent models are skipped without pulling.
pub fn run_embedding_profile(
    backend: &dyn EvalBackend,
    profile: &EmbeddingProfile,
    fixtures: &RetrievalFixtures,
    limit: Option<usize>,
) -> EmbeddingReport {
    let mut report = EmbeddingReport {
        profile_id: profile.id.clone(),
        model: profile.model.clone(),
        status: ProfileStatus::Evaluated,
        digest: None,
        dimensions: None,
        ranking_version: crate::ranking::RANKING_VERSION.to_owned(),
        modes: Vec::new(),
        latency: latency_stats(&[]),
        error: None,
    };
    let installed = match backend.installed_models() {
        Ok(models) => models,
        Err(error) => {
            eprintln!("[cortex-eval] {}: discovery failed: {error}", profile.id);
            report.status = ProfileStatus::DiscoveryFailed;
            return report;
        }
    };
    let Some(model) = installed
        .iter()
        .find(|model| model.model == profile.model || model.name == profile.model)
    else {
        eprintln!(
            "[cortex-eval] {}: model {} is not installed; skipping (no hidden pull)",
            profile.id, profile.model
        );
        report.status = ProfileStatus::ModelAbsent;
        return report;
    };
    report.digest = Some(model.digest.clone());

    let mut latencies = Vec::new();
    let corpus_texts: Vec<String> = fixtures.corpus.iter().map(|doc| doc.text.clone()).collect();
    let corpus_vectors = match embed_texts(backend, &profile.id, &corpus_texts, &mut latencies) {
        Ok(vectors) => vectors,
        Err(error) => {
            report.error = Some(error);
            report.latency = latency_stats(&latencies);
            return report;
        }
    };
    report.dimensions = corpus_vectors.first().map(Vec::len);

    let queries: Vec<_> = fixtures
        .queries
        .iter()
        .take(limit.unwrap_or(fixtures.queries.len()))
        .collect();
    let query_texts: Vec<String> = queries.iter().map(|query| query.text.clone()).collect();
    let query_vectors = match embed_texts(backend, &profile.id, &query_texts, &mut latencies) {
        Ok(vectors) => vectors,
        Err(error) => {
            report.error = Some(error);
            report.latency = latency_stats(&latencies);
            return report;
        }
    };

    let corpus_ids: Vec<&str> = fixtures.corpus.iter().map(|doc| doc.id.as_str()).collect();
    let adjacency = build_adjacency(&corpus_ids, &fixtures.related);
    let bm25 = Bm25Index::build(&corpus_texts);
    let mut mode_samples: BTreeMap<RetrievalMode, Vec<RetrievalSample>> = BTreeMap::new();
    for (query, vector) in queries.iter().zip(&query_vectors) {
        let embedding_ranking = rank_by_similarity(vector, &corpus_vectors);
        let lexical_ranking = bm25.rank(&query.text);
        let hybrid_ranking = rrf_fuse(
            &[embedding_ranking.as_slice(), lexical_ranking.as_slice()],
            fixtures.corpus.len(),
        );
        let graph_ranking = graph_boost(&hybrid_ranking, &adjacency);
        for (mode, ranking) in [
            (RetrievalMode::Embedding, &embedding_ranking),
            (RetrievalMode::Hybrid, &hybrid_ranking),
            (RetrievalMode::HybridGraph, &graph_ranking),
        ] {
            mode_samples.entry(mode).or_default().push(retrieval_sample(
                query,
                ranking,
                &corpus_ids,
            ));
        }
        eprintln!("[cortex-eval] {} retrieval {}", profile.id, query.id);
    }
    for (mode, samples) in mode_samples {
        let aggregate = aggregate_retrieval(&samples);
        let verdict = judge_retrieval(Some(&aggregate));
        report.modes.push(RetrievalModeReport {
            mode,
            aggregate,
            verdict,
            samples,
        });
    }
    report.latency = latency_stats(&latencies);
    report
}

fn retrieval_sample(
    query: &crate::fixtures::RetrievalQuery,
    ranking: &[usize],
    corpus_ids: &[&str],
) -> RetrievalSample {
    let ranked_ids: Vec<&str> = ranking.iter().map(|index| corpus_ids[*index]).collect();
    RetrievalSample {
        query_id: query.id.clone(),
        recall_at_3: recall_at_k(&ranked_ids, &query.relevant, 3),
        recall_at_5: recall_at_k(&ranked_ids, &query.relevant, 5),
        ndcg_at_5: ndcg_at_k(&ranked_ids, &query.relevant, 5),
        reciprocal_rank: reciprocal_rank(&ranked_ids, &query.relevant),
        top: ranked_ids
            .iter()
            .take(5)
            .map(|id| (*id).to_owned())
            .collect(),
    }
}

fn embed_texts(
    backend: &dyn EvalBackend,
    profile: &str,
    texts: &[String],
    latencies: &mut Vec<u64>,
) -> Result<Vec<Vec<f32>>, String> {
    let mut vectors = Vec::with_capacity(texts.len());
    for batch in texts.chunks(EMBED_BATCH) {
        let timed = backend.embed(&EmbedRequest {
            profile: profile.to_owned(),
            inputs: batch.to_vec(),
        })?;
        latencies.push(timed.latency_ms);
        vectors.extend(timed.vectors);
    }
    if vectors.len() == texts.len() {
        Ok(vectors)
    } else {
        Err(format!(
            "embedding count mismatch: {} vectors for {} inputs",
            vectors.len(),
            texts.len()
        ))
    }
}

/// Run the selected suites for one profile. Absent models are skipped
/// fail-closed: no pull, no substitute, an explicit status instead.
pub fn run_profile(
    backend: &dyn EvalBackend,
    profile: &EvalProfile,
    fixtures: &FixtureSet,
    selection: SuiteSelection,
    limit: Option<usize>,
) -> ProfileReport {
    let mut report = empty_report(profile);
    let installed = match backend.installed_models() {
        Ok(models) => models,
        Err(error) => {
            eprintln!("[cortex-eval] {}: discovery failed: {error}", profile.id);
            report.status = ProfileStatus::DiscoveryFailed;
            return report;
        }
    };
    let Some(model) = installed
        .iter()
        .find(|model| model.model == profile.model || model.name == profile.model)
    else {
        eprintln!(
            "[cortex-eval] {}: model {} is not installed; skipping (no hidden pull)",
            profile.id, profile.model
        );
        report.status = ProfileStatus::ModelAbsent;
        return report;
    };
    report.digest = Some(model.digest.clone());

    let mut latencies = Vec::new();
    if selection.classification {
        let taken = bounded(&fixtures.classification, limit);
        for (index, fixture) in taken.iter().enumerate() {
            let sample = run_classification(backend, &profile.id, fixture);
            progress(
                &profile.id,
                "classification",
                index,
                taken.len(),
                sample.error.as_deref(),
            );
            if sample.error.is_none() {
                latencies.push(sample.latency_ms);
            }
            report.classification_samples.push(sample);
        }
        report.classification = Some(aggregate_classification(&report.classification_samples));
    }
    if selection.extraction {
        let taken = bounded(&fixtures.extraction, limit);
        for (index, fixture) in taken.iter().enumerate() {
            let sample = run_extraction(backend, &profile.id, fixture);
            progress(
                &profile.id,
                "extraction",
                index,
                taken.len(),
                sample.error.as_deref(),
            );
            if sample.error.is_none() {
                latencies.push(sample.latency_ms);
            }
            report.extraction_samples.push(sample);
        }
        report.extraction = Some(aggregate_extraction(&report.extraction_samples));
    }
    if selection.compression {
        let taken = bounded(&fixtures.compression, limit);
        for (index, fixture) in taken.iter().enumerate() {
            let sample = run_compression(backend, &profile.id, fixture);
            progress(
                &profile.id,
                "compression",
                index,
                taken.len(),
                sample.error.as_deref(),
            );
            if sample.error.is_none() {
                latencies.push(sample.latency_ms);
            }
            report.compression_samples.push(sample);
        }
        report.compression = Some(aggregate_compression(&report.compression_samples));
    }

    report.device = backend.running_models().ok().and_then(|running| {
        running
            .iter()
            .find(|entry| entry.model == profile.model || entry.name == profile.model)
            .map(|entry| entry.placement)
    });
    report.latency = latency_stats(&latencies);
    report.verdict = judge(
        profile.tier,
        report.classification.as_ref(),
        report.extraction.as_ref(),
        report.compression.as_ref(),
    );
    report
}

fn empty_report(profile: &EvalProfile) -> ProfileReport {
    ProfileReport {
        profile_id: profile.id.clone(),
        tier: profile.tier,
        model: profile.model.clone(),
        status: ProfileStatus::Evaluated,
        digest: None,
        device: None,
        classification: None,
        extraction: None,
        compression: None,
        latency: latency_stats(&[]),
        verdict: judge(profile.tier, None, None, None),
        classification_samples: Vec::new(),
        extraction_samples: Vec::new(),
        compression_samples: Vec::new(),
    }
}

fn bounded<T>(fixtures: &[T], limit: Option<usize>) -> &[T] {
    let count = limit.unwrap_or(fixtures.len()).min(fixtures.len());
    &fixtures[..count]
}

fn progress(profile: &str, suite: &str, index: usize, total: usize, error: Option<&str>) {
    match error {
        None => eprintln!("[cortex-eval] {profile} {suite} {}/{total}", index + 1),
        Some(error) => eprintln!(
            "[cortex-eval] {profile} {suite} {}/{total}: {error}",
            index + 1
        ),
    }
}

fn run_classification(
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
/// backticks or quotes). Content differences — invented or missing entries —
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

fn run_extraction(
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

fn run_compression(
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
