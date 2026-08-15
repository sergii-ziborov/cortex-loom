use super::*;

#[test]
fn cosine_rejects_unequal_or_non_finite_vectors() {
    assert!(cosine_similarity(&[1.0, 0.0], &[1.0], None).is_err());
    assert!(cosine_similarity(&[1.0, f32::NAN], &[1.0, 0.0], None).is_err());
    assert!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0], Some(8)).is_err());
    assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0], Some(2)).unwrap() - 1.0).abs() < 1e-9);
    assert!(
        cosine_checked(
            &[1.0, 0.0],
            &[1.0, 0.0],
            &CosineSpec {
                expected_dim: Some(2),
                model_digest: Some("embed-v1"),
                observed_digest: Some("embed-v2"),
                require_unit: false,
            },
        )
        .is_err()
    );
    assert!(
        cosine_checked(
            &[2.0, 0.0],
            &[1.0, 0.0],
            &CosineSpec {
                expected_dim: Some(2),
                model_digest: Some("embed-v1"),
                observed_digest: Some("embed-v1"),
                require_unit: true,
            },
        )
        .is_err()
    );
}

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
    let first = vec![1, 0, 2];
    let second = vec![1, 2, 0];
    let fused = rrf_fuse(&[&first, &second], 3);
    assert_eq!(fused[0], 1);
    assert_eq!(fused, rrf_fuse(&[&first, &second], 3), "deterministic");
    let left = [0, 1, 2];
    let right = [2, 1, 0];
    let convex = rrf_fuse(&[&left, &right], 3);
    assert_eq!(convex, [0, 2, 1]);
}

#[test]
fn graph_boost_lifts_a_neighbor_without_overtaking_the_benefactor() {
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
    assert!(pairs.iter().any(
        |pair| pair.contains(&"WX-SYMBOL".to_owned()) && pair.contains(&"WX-SOURCE".to_owned())
    ));
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
