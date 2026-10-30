// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Lexical tokenization, stemming, synonyms, and TF-IDF scoring.
//!
//! Port of `pro/src/features/dbindex/lexical.ts`. Pure CPU — no I/O.

use std::collections::HashSet;

use crate::model::TokenIndex;

/// Synonym postings are down-weighted relative to direct lexical hits (TS: `0.7`).
const SYNONYM_WEIGHT_PENALTY: f64 = 0.7;

/// Fallback corpus size when `counts.tables` is zero (TS: `counts.tables || 100`).
const DEFAULT_TABLE_COUNT: f64 = 100.0;

/// Score deducted for refs that look like system / backup tables.
const SYSTEM_TABLE_PENALTY: f64 = 1.0;

/// Max parenthetical length accepted when mining synonyms from comments.
const MAX_PAREN_SYNONYM_LEN: usize = 25;

/// Max words inside a mined parenthetical synonym phrase.
const MAX_PAREN_SYNONYM_WORDS: usize = 2;

/// Query term found in the object's own name (`orders` → `orders.orders`).
const NAME_MATCH_BONUS: f64 = 2.0;

/// Synonym of a query term found in the object's name (`revenue` → `mv_daily_sales_summary`).
const NAME_SYNONYM_BONUS: f64 = 1.0;

/// Query term found in the object's schema (`orders` → `orders.mv_daily_sales_summary`).
const SCHEMA_MATCH_BONUS: f64 = 1.0;

/// Every word of the object's name appears in the query.
const FULL_NAME_BONUS: f64 = 1.0;

/// Query words that never help find a table.
const QUERY_STOPWORDS: &[&str] = &[
    "all", "and", "are", "by", "each", "every", "find", "for", "from", "get", "how", "in", "is",
    "list", "many", "me", "much", "my", "of", "on", "or", "our", "per", "show", "the", "their",
    "to", "what", "which", "who", "with",
];

/// Time grains. Any timestamp column can be grouped by month, so a column
/// such as `exp_month` is not evidence; these only count when they appear in
/// an object's name (`mv_monthly_revenue`).
const TIME_GRAIN_WORDS: &[&str] = &[
    "annual",
    "day",
    "daily",
    "hour",
    "hourly",
    "month",
    "monthly",
    "quarter",
    "quarterly",
    "week",
    "weekly",
    "year",
    "yearly",
];

/// Subset of manifest counts needed for IDF (TS `{ tables: number }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableCounts {
    pub tables: u64,
}

/// Common DB identifier abbreviations → expansion tokens (TS `ABBREVIATIONS`).
pub fn abbreviations(word: &str) -> Option<&'static [&'static str]> {
    Some(match word {
        "qty" => &["quantity"],
        "amt" => &["amount"],
        "dt" => &["date"],
        "addr" => &["address"],
        "org" => &["organization"],
        "usr" => &["user"],
        "desc" => &["description"],
        "num" => &["number"],
        "fk" => &["foreign", "key"],
        "pk" => &["primary", "key"],
        _ => return None,
    })
}

/// Built-in synonym map (TS `SYNONYMS`).
pub fn builtin_synonyms(token: &str) -> Option<&'static [&'static str]> {
    Some(match token {
        "customer" => &["user", "client", "buyer", "member"],
        "user" => &["customer", "client", "member", "account"],
        "order" => &["purchase", "transaction", "sale", "deal"],
        "purchase" => &["order", "transaction", "sale"],
        // Not "payment": it pulled `payment_methods` to the top of revenue questions.
        "revenue" => &["amount", "price", "sales", "income"],
        "payment" => &["revenue", "charge", "invoice"],
        "product" => &["item", "goods", "sku"],
        "item" => &["product", "goods"],
        "auth" => &["login", "credential", "user"],
        "config" => &["setting", "preference", "option"],
        _ => return None,
    })
}

/// Tokenize by camelCase / digit / non-alnum splits, expand abbreviations, stem.
///
/// Matches TS `tokenize` apart from the stemmer (see [`stem_word`]).
pub fn tokenize(text: &str) -> Vec<String> {
    tokenize_with(text, stem_word)
}

fn tokenize_with(text: &str, stem: fn(&str) -> String) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let camel_split = split_camel_case(text);
    let digit_split = split_digit_boundaries(&camel_split);
    let normalized: String = digit_split
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();

    let words: Vec<String> = normalized
        .to_ascii_lowercase()
        .split_whitespace()
        .filter(|w| w.len() > 1 || (w.len() == 1 && w.chars().all(|c| c.is_ascii_digit())))
        .map(str::to_owned)
        .collect();

    let mut expanded = Vec::with_capacity(words.len());
    for word in words {
        if let Some(expansion) = abbreviations(&word) {
            expanded.extend(expansion.iter().map(|s| (*s).to_owned()));
        } else {
            expanded.push(word);
        }
    }

    expanded.into_iter().map(|w| stem(&w)).collect()
}

/// Plural stemmer for DB identifiers.
///
/// **Divergence:** TS `stemWord` stripped "-es" from everything except
/// sibilant endings, so `warehouses` became `warehous` and `addresses`
/// became `addresse`, and neither matched a query for "warehouse" or
/// "address". Here only sibilant endings lose "-es".
pub fn stem_word(word: &str) -> String {
    if let Some(stem) = word.strip_suffix("ies") {
        return format!("{stem}y");
    }
    let sibilant = ["sses", "shes", "ches", "xes"]
        .iter()
        .any(|s| word.ends_with(s))
        || (word.ends_with("uses") && !word.ends_with("ouses") && !word.ends_with("auses"));
    if sibilant {
        return word[..word.len() - 2].to_owned();
    }
    if word.ends_with('s')
        && !word.ends_with("ss")
        && !word.ends_with("us")
        && !word.ends_with("as")
        && !word.ends_with("is")
    {
        return word[..word.len() - 1].to_owned();
    }
    word.to_owned()
}

/// TS `stemWord`, which built indexes written before [`stem_word`] changed.
/// Queries also look up these stems so older indexes keep matching.
fn legacy_stem_word(word: &str) -> String {
    if let Some(stem) = word.strip_suffix("ies") {
        return format!("{stem}y");
    }
    if word.ends_with("es")
        && !word.ends_with("sses")
        && !word.ends_with("shes")
        && !word.ends_with("ches")
    {
        return word[..word.len() - 2].to_owned();
    }
    if word.ends_with('s')
        && !word.ends_with("ss")
        && !word.ends_with("us")
        && !word.ends_with("as")
    {
        return word[..word.len() - 1].to_owned();
    }
    word.to_owned()
}

/// Candidate object refs from direct + synonym postings (TS `candidateRefsFromPostings`).
///
/// **Divergence:** result is sorted ascending for determinism. TS returns `Set`
/// insertion order.
pub fn candidate_refs_from_postings(
    query_tokens: &[String],
    token_index: &TokenIndex,
) -> Vec<String> {
    let mut candidates: HashSet<String> = HashSet::new();

    for token in query_tokens {
        if let Some(postings) = token_index.postings.get(token) {
            for (ref_, _) in postings {
                candidates.insert(ref_.clone());
            }
        }

        for syn in resolve_synonyms(token, token_index) {
            if let Some(syn_postings) = token_index.postings.get(&syn) {
                for (ref_, _) in syn_postings {
                    candidates.insert(ref_.clone());
                }
            }
        }
    }

    let mut out: Vec<String> = candidates.into_iter().collect();
    out.sort();
    out
}

/// TF-IDF score of `object_ref` against query tokens (TS `scoreObject`).
pub fn score_object(
    object_ref: &str,
    query_tokens: &[String],
    token_index: &TokenIndex,
    counts: TableCounts,
) -> f64 {
    let mut score = 0.0;
    let n = if counts.tables == 0 {
        DEFAULT_TABLE_COUNT
    } else {
        counts.tables as f64
    };

    let idf = |token: &str| {
        let df = token_index.df.get(token).copied().unwrap_or(1.0);
        (1.0 + n / df).ln()
    };

    for token in query_tokens {
        let token_idf = idf(token);
        if let Some(postings) = token_index.postings.get(token)
            && let Some((_, weight)) = postings.iter().find(|(r, _)| r == object_ref)
        {
            score += weight * token_idf;
        }

        for syn in resolve_synonyms(token, token_index) {
            if let Some(syn_postings) = token_index.postings.get(&syn)
                && let Some((_, weight)) = syn_postings.iter().find(|(r, _)| r == object_ref)
            {
                // A rare synonym must not outweigh the word the user typed:
                // "order" should not rank `transactions` above `orders`.
                let weight = weight * SYNONYM_WEIGHT_PENALTY;
                score += weight * idf(&syn).min(token_idf);
            }
        }
    }

    if object_ref.contains("audit")
        || object_ref.contains("_bak")
        || object_ref.contains("_tmp")
        || object_ref.contains("backup")
    {
        score -= SYSTEM_TABLE_PENALTY;
    }

    score.max(0.0)
}

/// A search query split into the terms used for scoring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryTerms {
    /// Terms looked up in the postings: no stopwords or time grains, plus
    /// legacy stems for indexes built before [`stem_word`] changed.
    pub scoring: Vec<String>,
    /// Terms compared with object and schema names (time grains included).
    pub name: Vec<String>,
}

/// Split a natural-language query into [`QueryTerms`].
pub fn query_terms(query: &str) -> QueryTerms {
    let current = tokenize(query);
    let legacy = tokenize_with(query, legacy_stem_word);
    let mut scoring = Vec::new();
    let mut name = Vec::new();
    for (term, legacy_term) in current.into_iter().zip(legacy) {
        if QUERY_STOPWORDS.contains(&term.as_str()) {
            continue;
        }
        if !TIME_GRAIN_WORDS.contains(&term.as_str()) {
            if legacy_term != term && !scoring.contains(&legacy_term) {
                scoring.push(legacy_term);
            }
            if !scoring.contains(&term) {
                scoring.push(term.clone());
            }
        }
        if !name.contains(&term) {
            name.push(term);
        }
    }
    if scoring.is_empty() {
        scoring = name.clone();
    }
    QueryTerms { scoring, name }
}

/// Bonus for query terms that name the object or its schema. TF-IDF alone
/// scores `orders.orders` low for "orders" because many tables have an
/// `order_id` column, which makes the word common.
pub fn name_match_bonus(object_ref: &str, name_terms: &[String], token_index: &TokenIndex) -> f64 {
    let (schema, name) = object_ref.split_once('.').unwrap_or(("", object_ref));
    let name_tokens = tokenize(name);
    let schema_tokens = tokenize(schema);
    let mut bonus = 0.0;
    let mut covered = 0;
    for term in name_terms {
        if name_tokens.contains(term) {
            bonus += NAME_MATCH_BONUS;
            covered += 1;
        } else if resolve_synonyms(term, token_index)
            .iter()
            .any(|syn| name_tokens.contains(syn))
        {
            bonus += NAME_SYNONYM_BONUS;
        }
        if schema_tokens.contains(term) {
            bonus += SCHEMA_MATCH_BONUS;
        }
    }
    let distinct_name_tokens: HashSet<&String> = name_tokens.iter().collect();
    if !distinct_name_tokens.is_empty() && covered >= distinct_name_tokens.len() {
        bonus += FULL_NAME_BONUS;
    }
    bonus
}

/// Mine synonyms from object comments (TS `extractSynonymsFromComment`).
pub fn extract_synonyms_from_comment(comment: &str) -> Vec<String> {
    let mut results: Vec<String> = Vec::new();

    // 1. aka / also known as patterns (case-insensitive)
    mine_aka_patterns(comment, &mut results);

    // 2. Short parentheticals
    for content in parentheticals(comment) {
        let content = content.trim();
        if content.is_empty() || content.len() > MAX_PAREN_SYNONYM_LEN {
            continue;
        }
        let cleaned = strip_aka_prefix(content);
        if !is_simple_identifier_phrase(&cleaned) {
            continue;
        }
        let words: Vec<&str> = cleaned.split_whitespace().collect();
        if words.len() <= MAX_PAREN_SYNONYM_WORDS {
            push_unique(&mut results, cleaned);
        }
    }

    results
}

fn resolve_synonyms(token: &str, token_index: &TokenIndex) -> Vec<String> {
    let mut syns: HashSet<String> = HashSet::new();
    if let Some(builtin) = builtin_synonyms(token) {
        // Postings hold stemmed tokens, so "sales" must be looked up as "sale".
        for s in builtin {
            syns.insert(stem_word(s));
        }
    }
    if let Some(mined) = token_index.synonyms.get(token) {
        for s in mined {
            syns.insert(s.clone());
        }
    }
    let mut out: Vec<String> = syns.into_iter().collect();
    out.sort();
    out
}

fn split_camel_case(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 4);
    for i in 0..chars.len() {
        if i > 0 {
            let prev = chars[i - 1];
            let curr = chars[i];
            if prev.is_ascii_lowercase() && curr.is_ascii_uppercase() {
                out.push(' ');
            }
        }
        out.push(chars[i]);
    }
    out
}

fn split_digit_boundaries(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 4);
    for i in 0..chars.len() {
        if i > 0 {
            let prev = chars[i - 1];
            let curr = chars[i];
            let letter_digit = prev.is_ascii_alphabetic() && curr.is_ascii_digit();
            let digit_letter = prev.is_ascii_digit() && curr.is_ascii_alphabetic();
            if letter_digit || digit_letter {
                out.push(' ');
            }
        }
        out.push(chars[i]);
    }
    out
}

fn mine_aka_patterns(comment: &str, results: &mut Vec<String>) {
    let lower = comment.to_ascii_lowercase();
    let needles = ["aka", "also known as"];
    let bytes = comment.as_bytes();
    let lower_bytes = lower.as_bytes();

    for needle in needles {
        let mut start = 0;
        while let Some(rel) = find_subslice(&lower_bytes[start..], needle.as_bytes()) {
            let at = start + rel;
            let after = at + needle.len();
            // Require word-ish boundary before needle (start or non-alnum).
            if at > 0 {
                let prev = lower_bytes[at - 1];
                if prev.is_ascii_alphanumeric() {
                    start = at + 1;
                    continue;
                }
            }
            let rest = skip_ascii_whitespace(bytes, after);
            if let Some(ident) = take_ident(bytes, rest) {
                let ident_len = ident.len();
                push_unique(results, ident);
                start = rest + ident_len;
            } else {
                start = after;
            }
        }
    }
}

fn parentheticals(comment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = comment.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b')' {
                i += 1;
            }
            if i < bytes.len() {
                // Parentheses are ASCII; interior is a substring of UTF-8 `comment`.
                out.push(String::from_utf8_lossy(&bytes[start..i]).into_owned());
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

fn strip_aka_prefix(content: &str) -> String {
    let lower = content.to_ascii_lowercase();
    for prefix in ["aka ", "also known as "] {
        if lower.starts_with(prefix) {
            return content[prefix.len()..].trim().to_owned();
        }
    }
    content.trim().to_owned()
}

fn is_simple_identifier_phrase(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c.is_ascii_whitespace())
}

fn push_unique(results: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !results.iter().any(|r| r == &value) {
        results.push(value);
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn skip_ascii_whitespace(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn take_ident(bytes: &[u8], start: usize) -> Option<String> {
    if start >= bytes.len() {
        return None;
    }
    let mut end = start;
    while end < bytes.len() {
        let c = bytes[end];
        if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' {
            end += 1;
        } else {
            break;
        }
    }
    if end == start {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes[start..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn empty_index() -> TokenIndex {
        TokenIndex {
            version: 1,
            df: HashMap::new(),
            postings: HashMap::new(),
            synonyms: HashMap::new(),
        }
    }

    #[test]
    fn tokenize_table_driven() {
        let cases: &[(&str, &[&str])] = &[
            ("", &[]),
            ("users", &["user"]),
            ("UserOrders", &["user", "order"]),
            ("order_items", &["order", "item"]),
            ("qty_amt", &["quantity", "amount"]),
            ("fk_user_id", &["foreign", "key", "user", "id"]),
            ("pk", &["primary", "key"]),
            ("col2Name", &["col", "2", "name"]),
            ("a", &[]),    // single letter dropped
            ("1", &["1"]), // single digit kept
            ("classes", &["class"]),
            ("addresses", &["address"]),
            ("companies", &["company"]),
            ("warehouses", &["warehouse"]),
        ];

        for (input, expected) in cases {
            let got = tokenize(input);
            assert_eq!(
                got,
                expected.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
                "tokenize({input:?})"
            );
        }
    }

    #[test]
    fn stem_word_table_driven() {
        let cases: &[(&str, &str)] = &[
            ("companies", "company"),
            ("addresses", "address"),
            ("buses", "bus"),
            ("statuses", "status"),
            ("users", "user"),
            ("class", "class"),
            ("classes", "class"),
            ("status", "status"),
            ("atlas", "atlas"),
            ("analysis", "analysis"),
            ("ashes", "ash"),
            ("boxes", "box"),
            ("matches", "match"),
            ("warehouses", "warehouse"),
            ("houses", "house"),
            ("invoices", "invoice"),
            ("sales", "sale"),
            ("sizes", "size"),
        ];
        for (input, expected) in cases {
            assert_eq!(stem_word(input), *expected, "stem({input})");
        }
    }

    #[test]
    fn legacy_stems_match_indexes_built_by_ts() {
        assert_eq!(legacy_stem_word("addresses"), "addresse");
        assert_eq!(legacy_stem_word("warehouses"), "warehous");
        assert_eq!(legacy_stem_word("invoices"), "invoic");
    }

    #[test]
    fn query_terms_drop_filler_and_keep_time_grains_for_names() {
        let terms = query_terms("Monthly revenue from orders");
        assert_eq!(terms.scoring, vec!["revenue", "order"]);
        assert_eq!(terms.name, vec!["monthly", "revenue", "order"]);

        // Older indexes stored "warehous"; look that up too.
        let terms = query_terms("stock by warehouses");
        assert_eq!(terms.scoring, vec!["stock", "warehous", "warehouse"]);

        // A query of only time grains still searches.
        assert_eq!(query_terms("monthly").scoring, vec!["monthly"]);
    }

    #[test]
    fn name_match_bonus_prefers_the_named_table() {
        let index = empty_index();
        let terms = query_terms("cancelled orders").name;
        let orders = name_match_bonus("orders.orders", &terms, &index);
        let items = name_match_bonus("orders.order_items", &terms, &index);
        let other = name_match_bonus("billing.transactions", &terms, &index);
        // name + schema + whole name covered
        assert_eq!(
            orders,
            NAME_MATCH_BONUS + SCHEMA_MATCH_BONUS + FULL_NAME_BONUS
        );
        assert_eq!(items, NAME_MATCH_BONUS + SCHEMA_MATCH_BONUS);
        // "transaction" is a built-in synonym of "order"
        assert_eq!(other, NAME_SYNONYM_BONUS);
    }

    #[test]
    fn rare_synonym_does_not_outscore_the_typed_word() {
        let mut index = empty_index();
        index.df.insert("order".into(), 8.0);
        index.df.insert("transaction".into(), 1.0);
        index
            .postings
            .insert("order".into(), vec![("orders.orders".into(), 3.0)]);
        index.postings.insert(
            "transaction".into(),
            vec![("billing.transactions".into(), 3.0)],
        );
        let tokens = vec!["order".to_owned()];
        let counts = TableCounts { tables: 20 };
        let direct = score_object("orders.orders", &tokens, &index, counts);
        let via_syn = score_object("billing.transactions", &tokens, &index, counts);
        assert!(direct > via_syn, "{direct} <= {via_syn}");
    }

    #[test]
    fn score_object_direct_and_synonym() {
        let mut index = empty_index();
        index.df.insert("user".into(), 2.0);
        index.df.insert("customer".into(), 1.0);
        index
            .postings
            .insert("user".into(), vec![("public.users".into(), 1.0)]);
        index
            .postings
            .insert("customer".into(), vec![("public.customers".into(), 1.0)]);

        let counts = TableCounts { tables: 10 };
        let tokens = vec!["user".to_owned()];

        let direct = score_object("public.users", &tokens, &index, counts);
        let via_syn = score_object("public.customers", &tokens, &index, counts);
        assert!(direct > 0.0);
        assert!(via_syn > 0.0);
        // Synonym path applies 0.7 penalty on weight; same df shape → lower score.
        assert!(direct > via_syn);

        let n = 10.0_f64;
        let expected_direct = 1.0 * (1.0 + n / 2.0).ln();
        assert!((direct - expected_direct).abs() < 1e-12);
    }

    #[test]
    fn score_object_penalizes_backup_refs() {
        let mut index = empty_index();
        index.df.insert("user".into(), 1.0);
        index.postings.insert(
            "user".into(),
            vec![
                ("public.users".into(), 1.0),
                ("public.users_bak".into(), 1.0),
            ],
        );
        let tokens = vec!["user".to_owned()];
        let counts = TableCounts { tables: 10 };
        let normal = score_object("public.users", &tokens, &index, counts);
        let bak = score_object("public.users_bak", &tokens, &index, counts);
        assert!(normal > bak);
        assert!((normal - bak - SYSTEM_TABLE_PENALTY).abs() < 1e-12 || bak == 0.0);
    }

    #[test]
    fn candidate_refs_include_synonym_hits() {
        let mut index = empty_index();
        index
            .postings
            .insert("customer".into(), vec![("public.customers".into(), 1.0)]);
        index
            .postings
            .insert("user".into(), vec![("public.users".into(), 1.0)]);
        let tokens = vec!["user".to_owned()];
        let refs = candidate_refs_from_postings(&tokens, &index);
        assert_eq!(refs, vec!["public.customers", "public.users"]);
    }

    #[test]
    fn extract_synonyms_from_comment_table_driven() {
        let cases: &[(&str, &[&str])] = &[
            ("Customer account aka client", &["client"]),
            ("Also known as buyer_id here", &["buyer_id"]),
            ("Primary key (cust)", &["cust"]),
            ("Note (aka member)", &["member"]),
            ("Too long (this parenthetical is way too long to keep)", &[]),
            ("Three words (one two three)", &[]),
            ("Dup aka client and (client)", &["client"]),
        ];
        for (comment, expected) in cases {
            let got = extract_synonyms_from_comment(comment);
            assert_eq!(
                got,
                expected.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
                "comment={comment:?}"
            );
        }
    }

    #[test]
    fn default_table_count_when_zero() {
        let mut index = empty_index();
        index.df.insert("x".into(), 1.0);
        index
            .postings
            .insert("x".into(), vec![("public.t".into(), 2.0)]);
        let tokens = vec!["x".to_owned()];
        let score = score_object("public.t", &tokens, &index, TableCounts { tables: 0 });
        let expected = 2.0 * (1.0 + DEFAULT_TABLE_COUNT / 1.0).ln();
        assert!((score - expected).abs() < 1e-12);
    }
}
