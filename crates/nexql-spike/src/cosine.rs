//! Cosine similarity helpers for Phase 0 ranking smoke tests.

/// Cosine similarity between two equal-length L2-normalized (or raw) vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "vector dims must match");
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

/// Return indices of the `k` highest cosine scores against `query`, descending.
pub fn top_k(query: &[f32], corpus: &[Vec<f32>], k: usize) -> Vec<(usize, f32)> {
    let mut scored: Vec<(usize, f32)> = corpus
        .iter()
        .enumerate()
        .map(|(i, v)| (i, cosine_similarity(query, v)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn top_k_ranks_closest_first() {
        let query = vec![1.0, 0.0, 0.0];
        let corpus = vec![
            vec![0.0, 1.0, 0.0],
            vec![0.9, 0.1, 0.0],
            vec![-1.0, 0.0, 0.0],
        ];
        let ranked = top_k(&query, &corpus, 2);
        assert_eq!(ranked[0].0, 1);
        assert!(ranked[0].1 > ranked[1].1);
    }
}
