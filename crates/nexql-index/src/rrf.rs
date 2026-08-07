// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Reciprocal Rank Fusion + cosine helpers (always available; no candle).
//!
//! Port of `fuseRrf` / `cosineSimilarity` from Pro `IndexQueryService.ts` /
//! `embeddings.ts`.

/// RRF constant — matches TS `RRF_K`.
pub const RRF_K: f64 = 60.0;

/// Rank assigned when a ref is missing from one list — matches TS `RRF_MISSING_RANK`.
pub const RRF_MISSING_RANK: usize = 10_000;

/// Cosine similarity between two equal-length vectors.
///
/// Returns `0.0` when dims mismatch, either vector is empty, or either norm is 0
/// (matches TS `cosineSimilarity`).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

/// Reciprocal Rank Fusion of two ranked lists (both sorted descending by score).
///
/// The union of both lists is ranked, so semantic-only refs survive fusion.
pub fn fuse_rrf(
    lexical: &[(String, f64)],
    semantic: &[(String, f64)],
    limit: usize,
) -> Vec<(String, f64)> {
    let lexical_rank: std::collections::HashMap<&str, usize> = lexical
        .iter()
        .enumerate()
        .map(|(i, (r, _))| (r.as_str(), i))
        .collect();
    let semantic_rank: std::collections::HashMap<&str, usize> = semantic
        .iter()
        .enumerate()
        .map(|(i, (r, _))| (r.as_str(), i))
        .collect();

    let mut merged: std::collections::HashSet<&str> = lexical_rank.keys().copied().collect();
    merged.extend(semantic_rank.keys().copied());

    let mut rrf_scores: Vec<(String, f64)> = merged
        .into_iter()
        .map(|ref_| {
            let r_l = lexical_rank.get(ref_).copied().unwrap_or(RRF_MISSING_RANK);
            let r_s = semantic_rank.get(ref_).copied().unwrap_or(RRF_MISSING_RANK);
            let score = (1.0 / (RRF_K + r_l as f64)) + (1.0 / (RRF_K + r_s as f64));
            (ref_.to_owned(), score)
        })
        .collect();

    rrf_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    if rrf_scores.len() > limit {
        rrf_scores.truncate(limit);
    }
    rrf_scores
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(ref_: &str, score: f64) -> (String, f64) {
        (ref_.into(), score)
    }

    #[test]
    fn fuse_rrf_reciprocal_rank_with_sentinel_keeps_semantic_only() {
        // Port of IndexQueryService.test.ts fuseRrf.
        let lexical = vec![hit("a", 5.0), hit("b", 3.0)];
        let semantic = vec![hit("b", 1.0), hit("c", 0.5)];

        let fused = fuse_rrf(&lexical, &semantic, 10);
        let scores: std::collections::HashMap<&str, f64> =
            fused.iter().map(|(r, s)| (r.as_str(), *s)).collect();

        assert!((scores["b"] - (1.0 / 61.0 + 1.0 / 60.0)).abs() < 1e-9);
        assert!((scores["a"] - (1.0 / 60.0 + 1.0 / 10060.0)).abs() < 1e-9);
        assert!((scores["c"] - (1.0 / 10060.0 + 1.0 / 61.0)).abs() < 1e-9);
        assert_eq!(
            fused.iter().map(|(r, _)| r.as_str()).collect::<Vec<_>>(),
            vec!["b", "a", "c"]
        );
    }

    #[test]
    fn fuse_rrf_respects_limit() {
        let lexical = vec![hit("a", 2.0), hit("b", 1.0)];
        assert_eq!(fuse_rrf(&lexical, &[], 1).len(), 1);
    }

    #[test]
    fn identical_unit_vectors_score_one() {
        let v = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn orthogonal_vectors_score_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn mismatched_dims_score_zero() {
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
    }
}
