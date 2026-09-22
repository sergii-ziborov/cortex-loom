use cortex_ollama::EmbedRequest;

use crate::backend::EvalBackend;
use crate::metrics::{RetrievalSample, latency_stats, ndcg_at_k, recall_at_k, reciprocal_rank};
use crate::verdict::judge;

use super::{EMBED_BATCH, EvalProfile, ProfileReport, ProfileStatus};

pub(super) fn empty_report(profile: &EvalProfile) -> ProfileReport {
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

pub(super) fn bounded<T>(fixtures: &[T], limit: Option<usize>) -> &[T] {
    let count = limit.unwrap_or(fixtures.len()).min(fixtures.len());
    &fixtures[..count]
}

pub(super) fn progress(
    profile: &str,
    suite: &str,
    index: usize,
    total: usize,
    error: Option<&str>,
) {
    match error {
        None => eprintln!("[cortex-eval] {profile} {suite} {}/{total}", index + 1),
        Some(error) => eprintln!(
            "[cortex-eval] {profile} {suite} {}/{total}: {error}",
            index + 1
        ),
    }
}

pub(super) fn retrieval_sample(
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

pub(super) fn embed_texts(
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
