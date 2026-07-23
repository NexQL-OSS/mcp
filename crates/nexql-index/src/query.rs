//! Lexical-only schema search (Phase 3g stub).
//!
//! Full query service (budget trimming, embeddings/RRF) lands later.
//! TODO: port IndexQueryService.test.ts cases; golden-file parity.

use std::collections::HashMap;

use crate::lexical::{TableCounts, candidate_refs_from_postings, score_object, tokenize};
use crate::model::{ObjectEntry, TokenIndex};

/// Rank objects by TF-IDF lexical score. Returns `(object_ref, score)` descending.
pub fn search_schema_lexical(
    query: &str,
    tokens: &TokenIndex,
    _entries: &HashMap<String, ObjectEntry>,
    counts: TableCounts,
    limit: usize,
) -> Vec<(String, f64)> {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let candidates = candidate_refs_from_postings(&query_tokens, tokens);
    let mut scored: Vec<(String, f64)> = candidates
        .into_iter()
        .map(|ref_| {
            let score = score_object(&ref_, &query_tokens, tokens, counts);
            (ref_, score)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    if scored.len() > limit {
        scored.truncate(limit);
    }
    scored
}
