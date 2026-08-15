//! Deterministic retrieval rankers: cosine similarity, lexical BM25,
//! reciprocal-rank fusion, and structural graph boosting.
//!
//! Every function here is pure and parameter-pinned, and none of them calls
//! a model — embedding vectors come from the caller. Because the parameters
//! are fixed and versioned by [`RANKING_VERSION`], two runs over the same
//! inputs produce the same ranking, which makes these rankings measurable
//! against a fixture set before you let them influence anything.

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Identifies the pinned ranking parameters, so a measured result can be
/// attributed to the exact algorithm that produced it.
pub const RANKING_VERSION: &str = "retrieval-ranking-v2";
/// Identity of the retrieval fixture set this ranking is measured against.
pub const RANKING_FIXTURE_SET: &str = "retrieval-fixtures-v1";
/// Production graph features: file spans and split siblings, not fixture pairs.
pub const ADJACENCY_KIND: &str = "evidence_spans";
const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;
const RRF_K: f64 = 60.0;
/// A doc gets a neighbor bonus only from fused top-`TOP_M` documents.
const GRAPH_TOP_M: usize = 3;
/// A neighbor of a top document is treated as ranked just behind it — it can
/// overtake unrelated mid-ranked documents but never its benefactor.
const GRAPH_RANK_PENALTY: f64 = 1.5;

/// Cosine similarity computed in f64; zero vectors compare as 0.
#[must_use]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += f64::from(*x) * f64::from(*y);
        norm_a += f64::from(*x) * f64::from(*x);
        norm_b += f64::from(*y) * f64::from(*y);
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a.sqrt() * norm_b.sqrt())
    }
}

/// Corpus indices ranked by descending similarity; ties break by index for
/// determinism.
#[must_use]
pub fn rank_by_similarity(query: &[f32], corpus: &[Vec<f32>]) -> Vec<usize> {
    rank_by_scores(
        &corpus
            .iter()
            .map(|vector| cosine_similarity(query, vector))
            .collect::<Vec<_>>(),
    )
}

/// Lowercased alphanumeric tokens.
#[must_use]
pub fn tokenize(text: &str) -> Vec<String> {
    text.chars()
        .map(|ch| {
            if ch.is_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// BM25 index over a fixed corpus.
pub struct Bm25Index {
    documents: Vec<HashMap<String, usize>>,
    lengths: Vec<usize>,
    average_length: f64,
    document_frequency: HashMap<String, usize>,
}

impl Bm25Index {
    #[must_use]
    pub fn build(corpus: &[String]) -> Self {
        let mut documents = Vec::with_capacity(corpus.len());
        let mut lengths = Vec::with_capacity(corpus.len());
        let mut document_frequency: HashMap<String, usize> = HashMap::new();
        for text in corpus {
            let tokens = tokenize(text);
            lengths.push(tokens.len());
            let mut frequencies: HashMap<String, usize> = HashMap::new();
            for token in tokens {
                *frequencies.entry(token).or_insert(0) += 1;
            }
            for term in frequencies.keys() {
                *document_frequency.entry(term.clone()).or_insert(0) += 1;
            }
            documents.push(frequencies);
        }
        let average_length = if lengths.is_empty() {
            0.0
        } else {
            lengths.iter().map(|length| to_f64(*length)).sum::<f64>() / to_f64(lengths.len())
        };
        Self {
            documents,
            lengths,
            average_length,
            document_frequency,
        }
    }

    /// BM25 score of one document for a tokenized query.
    #[must_use]
    pub fn score(&self, query_tokens: &[String], document: usize) -> f64 {
        let Some(frequencies) = self.documents.get(document) else {
            return 0.0;
        };
        let total = to_f64(self.documents.len());
        let length = to_f64(self.lengths[document]);
        let mut score = 0.0;
        for term in query_tokens {
            let Some(term_frequency) = frequencies.get(term) else {
                continue;
            };
            let document_frequency = to_f64(*self.document_frequency.get(term).unwrap_or(&0));
            let idf = ((total - document_frequency + 0.5) / (document_frequency + 0.5) + 1.0).ln();
            let tf = to_f64(*term_frequency);
            let denominator =
                tf + BM25_K1 * (1.0 - BM25_B + BM25_B * length / self.average_length.max(1.0));
            score += idf * (tf * (BM25_K1 + 1.0)) / denominator;
        }
        score
    }

    /// Corpus indices ranked by descending BM25 score; ties break by index.
    #[must_use]
    pub fn rank(&self, query: &str) -> Vec<usize> {
        let tokens = tokenize(query);
        rank_by_scores(
            &(0..self.documents.len())
                .map(|document| self.score(&tokens, document))
                .collect::<Vec<_>>(),
        )
    }
}

/// Reciprocal-rank fusion of several rankings over the same corpus.
#[must_use]
pub fn rrf_fuse(rankings: &[&[usize]], corpus_len: usize) -> Vec<usize> {
    let mut scores = vec![0.0_f64; corpus_len];
    for ranking in rankings {
        for (rank, document) in ranking.iter().enumerate() {
            if let Some(score) = scores.get_mut(*document) {
                *score += 1.0 / (RRF_K + to_f64(rank + 1));
            }
        }
    }
    rank_by_scores(&scores)
}

/// Re-rank a fused ranking with a structural neighbor lift in rank space:
/// a document related to one of the fused top-`GRAPH_TOP_M` documents is
/// treated as ranked `GRAPH_RANK_PENALTY` behind its best benefactor. It can
/// overtake unrelated mid-ranked documents but never the benefactor itself.
#[must_use]
pub fn graph_boost(fused: &[usize], adjacency: &BTreeMap<usize, BTreeSet<usize>>) -> Vec<usize> {
    let mut rank_of = vec![usize::MAX; fused.len()];
    for (rank, document) in fused.iter().enumerate() {
        if let Some(slot) = rank_of.get_mut(*document) {
            *slot = rank;
        }
    }
    let mut scores = vec![0.0_f64; fused.len()];
    for (rank, document) in fused.iter().enumerate() {
        let own_rank = to_f64(rank + 1);
        let best_benefactor = adjacency
            .get(document)
            .into_iter()
            .flatten()
            .filter_map(|neighbor| rank_of.get(*neighbor).copied())
            .filter(|neighbor_rank| *neighbor_rank < GRAPH_TOP_M)
            .min();
        let effective_rank = best_benefactor.map_or(own_rank, |benefactor_rank| {
            own_rank.min(to_f64(benefactor_rank + 1) + GRAPH_RANK_PENALTY)
        });
        scores[*document] = 1.0 / (RRF_K + effective_rank);
    }
    rank_by_scores(&scores)
}

/// Symmetric adjacency over corpus indices from declared related pairs;
/// malformed or unknown entries are ignored.
#[must_use]
pub fn build_adjacency(ids: &[&str], related: &[Vec<String>]) -> BTreeMap<usize, BTreeSet<usize>> {
    let index_of: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect();
    let mut adjacency: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for pair in related {
        let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if let (Some(&left), Some(&right)) =
            (index_of.get(first.as_str()), index_of.get(second.as_str()))
        {
            adjacency.entry(left).or_default().insert(right);
            adjacency.entry(right).or_default().insert(left);
        }
    }
    adjacency
}

/// One evidence fragment for production adjacency extraction.
pub struct EvidenceLink<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub content: &'a str,
}

/// Structural pairs from evidence: split siblings and the same source file.
///
/// This is the production stand-in for eval's declared `related` pairs. It
/// is intentionally conservative: prefix-sharing split parts, and fragments
/// that name the same source path. It does not invent crate-level edges.
#[must_use]
pub fn evidence_adjacency(items: &[EvidenceLink<'_>]) -> Vec<Vec<String>> {
    let mut pairs = Vec::new();
    for (left_index, left) in items.iter().enumerate() {
        for right in items.iter().skip(left_index + 1) {
            if parent_id(left.id) == parent_id(right.id)
                || same_source_file(left.source, right.source)
            {
                pairs.push(vec![left.id.to_owned(), right.id.to_owned()]);
            }
        }
    }
    pairs
}

/// `WX-VERIFY-2` → `WX-VERIFY`; ids without a trailing `-<digits>` stay as-is.
#[must_use]
pub fn parent_id(id: &str) -> &str {
    if let Some(position) = id.rfind('-') {
        let suffix = &id[position + 1..];
        if !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit()) {
            return &id[..position];
        }
    }
    id
}

fn same_source_file(left: &str, right: &str) -> bool {
    match (source_file(left), source_file(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn source_file(source: &str) -> Option<&str> {
    source.split([',', ' ', ';']).find_map(|part| {
        let trimmed = part.trim();
        let path = trimmed.split_once(':').map_or(trimmed, |(path, _)| path);
        path.contains('.')
            .then_some(path)
            .filter(|value| value.contains('/') || value.contains('\\'))
    })
}

/// The `hybrid_graph` pipeline: cosine + BM25 RRF, then structural boost.
///
/// Eval and production must call this with the same adjacency extractor.
#[must_use]
pub fn rank_hybrid_graph(
    query: &[f32],
    corpus: &[Vec<f32>],
    corpus_texts: &[String],
    query_text: &str,
    corpus_ids: &[&str],
    related: &[Vec<String>],
) -> Vec<usize> {
    let embedding = rank_by_similarity(query, corpus);
    let lexical = Bm25Index::build(corpus_texts).rank(query_text);
    let fused = rrf_fuse(
        &[embedding.as_slice(), lexical.as_slice()],
        corpus_texts.len(),
    );
    graph_boost(&fused, &build_adjacency(corpus_ids, related))
}

fn rank_by_scores(scores: &[f64]) -> Vec<usize> {
    let mut ranked: Vec<usize> = (0..scores.len()).collect();
    ranked.sort_by(|left, right| {
        scores[*right]
            .partial_cmp(&scores[*left])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.cmp(right))
    });
    ranked
}

fn to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bm25_prefers_term_overlap_and_is_deterministic() {
        let corpus = vec![
            "the routing policy decides the upstream target".to_owned(),
            "graph persistence with optimistic revisions".to_owned(),
            "routing guards and routing reasons".to_owned(),
        ];
        let index = Bm25Index::build(&corpus);
        let ranking = index.rank("routing policy");
        assert_eq!(ranking[0], 0, "both query terms match document 0");
        assert_eq!(ranking, index.rank("routing policy"), "deterministic");
        assert!(index.score(&tokenize("unrelated words"), 1).abs() < 1e-12);
    }

    #[test]
    fn rrf_fusion_is_deterministic_and_rewards_the_shared_leader() {
        // Doc 1 leads both rankings and must lead the fusion.
        let first = vec![1, 0, 2];
        let second = vec![1, 2, 0];
        let fused = rrf_fuse(&[&first, &second], 3);
        assert_eq!(fused[0], 1);
        assert_eq!(fused, rrf_fuse(&[&first, &second], 3), "deterministic");
        // Harmonic convexity: ranks (1,3) beat ranks (2,2); the tie between
        // doc 0 and doc 2 breaks by index.
        let left = [0, 1, 2];
        let right = [2, 1, 0];
        let convex = rrf_fuse(&[&left, &right], 3);
        assert_eq!(convex, [0, 2, 1]);
    }

    #[test]
    fn graph_boost_lifts_a_neighbor_without_overtaking_the_benefactor() {
        // Fused order: 0, 1, 2, 3. Doc 3 is related to doc 0 (rank 1): with
        // the 1.5 rank penalty it lands between ranks 2 and 3 — past the
        // unrelated doc 2, never past doc 0.
        let fused = vec![0, 1, 2, 3];
        let ids = ["a", "b", "c", "d"];
        let related = vec![vec!["a".to_owned(), "d".to_owned()]];
        let adjacency = build_adjacency(&ids, &related);
        let boosted = graph_boost(&fused, &adjacency);
        assert_eq!(boosted, [0, 1, 3, 2]);
    }

    #[test]
    fn evidence_adjacency_uses_files_not_just_split_prefixes() {
        let items = [
            EvidenceLink {
                id: "WX-SYMBOL",
                source: "src/options/types.rs:41",
                content: "enabled",
            },
            EvidenceLink {
                id: "WX-SOURCE",
                source: "src/options/types.rs:80",
                content: "impl",
            },
            EvidenceLink {
                id: "WX-VERIFY-1",
                source: "weavatrix:change_plan",
                content: "plan",
            },
            EvidenceLink {
                id: "WX-VERIFY-2",
                source: "weavatrix:change_plan",
                content: "tail",
            },
        ];
        let pairs = evidence_adjacency(&items);
        assert!(
            pairs
                .iter()
                .any(|pair| pair.contains(&"WX-SYMBOL".to_owned())
                    && pair.contains(&"WX-SOURCE".to_owned()))
        );
        assert!(
            pairs
                .iter()
                .any(|pair| pair.contains(&"WX-VERIFY-1".to_owned())
                    && pair.contains(&"WX-VERIFY-2".to_owned()))
        );
    }

    #[test]
    fn adjacency_is_symmetric_and_ignores_unknown_ids() {
        let ids = ["a", "b"];
        let related = vec![
            vec!["a".to_owned(), "b".to_owned()],
            vec!["a".to_owned(), "ghost".to_owned()],
        ];
        let adjacency = build_adjacency(&ids, &related);
        assert!(adjacency[&0].contains(&1));
        assert!(adjacency[&1].contains(&0));
        assert_eq!(adjacency[&0].len(), 1);
    }
}
