//! Index query service — port of `pro/src/features/dbindex/IndexQueryService.ts`.
//!
//! Phase 3g: lexical search, describe, join path, index-only sample values.
//! Embeddings / RRF fusion land in Phase 5.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::error::IndexError;
use crate::joins;
use crate::lexical::{TableCounts, candidate_refs_from_postings, score_object, tokenize};
use crate::model::{
    IndexOverrides, JoinEdge, JoinGraph, ObjectEntry, TokenIndex, ValueIndex,
};
use crate::store::IndexStore;

/// Boost applied when a query token hits the value inverted index (TS: `+ 2.0`).
const VALUE_HIT_BOOST: f64 = 2.0;

/// Ranked schema hit — matches TS `RankedHit`.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedHit {
    pub ref_: String,
    pub score: f64,
    pub kind: String,
}

/// Index-only (or optional live) sample-values result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleValuesResult {
    pub values: Vec<String>,
    /// Present when the index has no profiled samples (or values.json is empty).
    pub message: Option<String>,
}

/// Optional schema / PII gate for query methods.
///
/// Field layout mirrors `nexql_policy::PolicyFilter` so tools can map 1:1 without
/// pulling `pg_query` into this crate.
#[derive(Debug, Clone, Default)]
pub struct QueryPolicyFilter {
    pub allow_schemas: Vec<String>,
    pub deny_schemas: Vec<String>,
    pub deny_tables: Vec<String>,
    /// `schema.table.column` entries excluded from sample/search.
    pub pii_columns: Vec<String>,
}

impl QueryPolicyFilter {
    pub fn allows_schema(&self, schema: &str) -> bool {
        if self.deny_schemas.iter().any(|s| s == schema) {
            return false;
        }
        if self.allow_schemas.is_empty() {
            return true;
        }
        self.allow_schemas.iter().any(|s| s == schema)
    }

    pub fn allows_table(&self, schema: &str, table: &str) -> bool {
        if !self.allows_schema(schema) {
            return false;
        }
        !self
            .deny_tables
            .iter()
            .any(|g| table_glob_matches(g, schema, table))
    }

    pub fn is_pii_column(&self, schema: &str, table: &str, column: &str) -> bool {
        let qualified = format!("{schema}.{table}.{column}");
        self.pii_columns.iter().any(|p| p == &qualified)
    }
}

fn table_glob_matches(glob: &str, schema: &str, table: &str) -> bool {
    if let Some((gs, gt)) = glob.split_once('.') {
        let schema_ok = gs == "*" || gs == schema;
        let table_ok = gt == "*" || gt == table;
        schema_ok && table_ok
    } else {
        glob == table || glob == "*"
    }
}

/// Actionable error when `describe_object` / lookup misses.
///
/// Matches the MCP testing contract:  
/// `"Object X not found — call search_schema(...)"`.
pub fn missing_object_message(ref_: &str) -> String {
    format!("Object \"{ref_}\" not found in index — call search_schema(...) to find valid refs.")
}

/// Friendly empty-state when no profiled samples exist for `(ref, col)`.
pub fn no_samples_message(ref_: &str, col: &str) -> String {
    format!(
        "No profiled sample values in index for \"{ref_}\".\"{col}\". \
         Rebuild with stats/profiles depth, or sample from a live connection."
    )
}

/// Rank objects by TF-IDF lexical score. Returns `(object_ref, score)` descending.
///
/// Pure CPU helper — no I/O. Excluded refs / policy filtering belong in
/// [`IndexQueryService::search_schema`].
pub fn search_schema_lexical(
    query: &str,
    tokens: &TokenIndex,
    _entries: &HashMap<String, ObjectEntry>,
    counts: TableCounts,
    limit: usize,
) -> Vec<(String, f64)> {
    let mut scored = score_schema_lexical(query, tokens, counts, &HashSet::new(), None, None);
    if scored.len() > limit {
        scored.truncate(limit);
    }
    scored
}

/// Score all lexical (+ optional value-index) candidates without truncating.
fn score_schema_lexical(
    query: &str,
    tokens: &TokenIndex,
    counts: TableCounts,
    excluded: &HashSet<String>,
    value_index: Option<&ValueIndex>,
    overrides: Option<&IndexOverrides>,
) -> Vec<(String, f64)> {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let candidates = candidate_refs_from_postings(&query_tokens, tokens);
    let mut scores: HashMap<String, f64> = HashMap::new();

    for ref_ in candidates {
        if excluded.contains(&ref_) {
            continue;
        }
        let score = score_object(&ref_, &query_tokens, tokens, counts);
        if score > 0.0 {
            scores.insert(ref_, score);
        }
    }

    if let Some(value_index) = value_index {
        for token in &query_tokens {
            let Some(hits) = value_index.get(token) else {
                continue;
            };
            for hit in hits {
                if excluded.contains(&hit.ref_) {
                    continue;
                }
                if column_marked_pii(overrides, &hit.ref_, &hit.col) {
                    continue;
                }
                *scores.entry(hit.ref_.clone()).or_insert(0.0) += VALUE_HIT_BOOST;
            }
        }
    }

    let mut scored: Vec<(String, f64)> = scores.into_iter().collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

fn column_marked_pii(overrides: Option<&IndexOverrides>, ref_: &str, col: &str) -> bool {
    overrides
        .and_then(|o| o.objects.as_ref())
        .and_then(|objs| objs.get(ref_))
        .and_then(|obj| obj.columns.as_ref())
        .and_then(|cols| cols.get(col))
        .and_then(|c| c.pii)
        == Some(true)
}

fn excluded_refs(overrides: Option<&IndexOverrides>) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Some(objects) = overrides.and_then(|o| o.objects.as_ref()) {
        for (ref_, obj) in objects {
            if obj.excluded == Some(true) {
                out.insert(ref_.clone());
            }
        }
    }
    out
}

fn split_ref(ref_: &str) -> (String, String) {
    match ref_.split_once('.') {
        Some((schema, name)) => (schema.to_owned(), name.to_owned()),
        None => ("public".to_owned(), ref_.to_owned()),
    }
}

fn allows_ref(filter: Option<&QueryPolicyFilter>, ref_: &str) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let (schema, name) = split_ref(ref_);
    filter.allows_table(&schema, &name)
}

/// Loads index artifacts from [`IndexStore`] and answers schema queries.
///
/// Lexical-only this phase — pass no semantic opts (RRF is Phase 5).
pub struct IndexQueryService<'a> {
    store: &'a IndexStore,
    connection_id: String,
    database: String,
}

impl<'a> IndexQueryService<'a> {
    pub fn new(
        store: &'a IndexStore,
        connection_id: impl Into<String>,
        database: impl Into<String>,
    ) -> Self {
        Self {
            store,
            connection_id: connection_id.into(),
            database: database.into(),
        }
    }

    pub fn base_dir(&self) -> PathBuf {
        self.store.base_dir(&self.connection_id, &self.database)
    }

    /// Lexical schema search (no embeddings / RRF).
    ///
    /// Respects `overrides.json` exclusions and an optional [`QueryPolicyFilter`].
    pub fn search_schema(
        &self,
        query: &str,
        limit: usize,
        filter: Option<&QueryPolicyFilter>,
    ) -> Result<Vec<RankedHit>, IndexError> {
        let base = self.base_dir();
        let Some(manifest) = self.store.read_manifest(&base)? else {
            return Ok(Vec::new());
        };
        let Some(tokens) = self.store.read_tokens(&base, &manifest)? else {
            return Ok(Vec::new());
        };

        let overrides = self.store.read_overrides(&base)?;
        let excluded = excluded_refs(overrides.as_ref());
        let value_index = self.store.read_values(&base, &manifest)?;

        let counts = TableCounts {
            tables: manifest.counts.tables,
        };
        let mut scored = score_schema_lexical(
            query,
            &tokens,
            counts,
            &excluded,
            value_index.as_ref(),
            overrides.as_ref(),
        );

        scored.retain(|(ref_, _)| allows_ref(filter, ref_));
        if scored.len() > limit {
            scored.truncate(limit);
        }

        let mut hits = Vec::with_capacity(scored.len());
        for (ref_, score) in scored {
            let (schema, name) = split_ref(&ref_);
            let kind = self
                .store
                .get_object_entry(&base, &manifest, &schema, &name)?
                .map(|e| e.kind.as_str().to_owned())
                .unwrap_or_else(|| "table".to_owned());
            hits.push(RankedHit { ref_, score, kind });
        }
        Ok(hits)
    }

    /// Load a full [`ObjectEntry`] from shards (with overrides applied).
    ///
    /// Returns [`IndexError::Query`] with [`missing_object_message`] when absent
    /// or excluded.
    pub fn describe_object(
        &self,
        ref_: &str,
        filter: Option<&QueryPolicyFilter>,
    ) -> Result<ObjectEntry, IndexError> {
        if !allows_ref(filter, ref_) {
            return Err(IndexError::Query(format!(
                "Object \"{ref_}\" is denied by policy"
            )));
        }

        let base = self.base_dir();
        let Some(manifest) = self.store.read_manifest(&base)? else {
            return Err(IndexError::Query(missing_object_message(ref_)));
        };

        let (schema, name) = split_ref(ref_);
        let entry = self
            .store
            .get_object_entry(&base, &manifest, &schema, &name)?;

        match entry {
            Some(e) if e.excluded != Some(true) => Ok(e),
            _ => Err(IndexError::Query(missing_object_message(ref_))),
        }
    }

    /// Shortest FK join path via [`joins::get_join_path`].
    ///
    /// Unreachable paths surface as [`IndexError::Query`] with the standard
    /// "No join path found…" message.
    pub fn get_join_path(&self, a: &str, b: &str) -> Result<Vec<JoinEdge>, IndexError> {
        let graph = self.load_join_graph()?;
        joins::get_join_path(a, b, &graph).map_err(IndexError::Query)
    }

    /// Index-only sample values for `(ref, col)`.
    ///
    /// Prefers `ColumnProfile.common_values` on the object entry. When absent,
    /// returns an empty list with a friendly message (does **not** hit live DB
    /// unless `live_sample` is provided).
    pub fn sample_values(
        &self,
        ref_: &str,
        col: &str,
        filter: Option<&QueryPolicyFilter>,
        live_sample: Option<&dyn Fn(&str, &str) -> Result<Vec<String>, IndexError>>,
    ) -> Result<SampleValuesResult, IndexError> {
        if !allows_ref(filter, ref_) {
            return Err(IndexError::Query(format!(
                "Object \"{ref_}\" is denied by policy"
            )));
        }

        let (schema, name) = split_ref(ref_);
        if let Some(filter) = filter
            && filter.is_pii_column(&schema, &name, col)
        {
            return Err(IndexError::Query(format!(
                "Access Denied: Column \"{col}\" on \"{ref_}\" is flagged as PII."
            )));
        }

        let base = self.base_dir();
        let overrides = self.store.read_overrides(&base)?;
        if let Some(objects) = overrides.as_ref().and_then(|o| o.objects.as_ref())
            && let Some(obj) = objects.get(ref_)
        {
            if obj.excluded == Some(true) {
                return Err(IndexError::Query(format!(
                    "Access Denied: Object \"{ref_}\" is excluded from curation and grounding."
                )));
            }
            if column_marked_pii(overrides.as_ref(), ref_, col) {
                return Err(IndexError::Query(format!(
                    "Access Denied: Column \"{col}\" on \"{ref_}\" is flagged as PII."
                )));
            }
        }

        // Prefer profiled common values from the object shard.
        if let Some(manifest) = self.store.read_manifest(&base)?
            && let Some(entry) = self
                .store
                .get_object_entry(&base, &manifest, &schema, &name)?
        {
            if let Some(column) = entry.columns.iter().find(|c| c.name == col) {
                if column.pii == Some(true) {
                    return Err(IndexError::Query(format!(
                        "Access Denied: Column \"{col}\" on \"{ref_}\" is flagged as PII."
                    )));
                }
                if let Some(profile) = &column.profile
                    && let Some(vals) = &profile.common_values
                    && !vals.is_empty()
                {
                    return Ok(SampleValuesResult {
                        values: vals.clone(),
                        message: None,
                    });
                }
            }

            // values.json is an inverted token index (not raw samples). Presence
            // of a (ref, col) hit only confirms profiling ran — still no samples.
            let _ = self.store.read_values(&base, &manifest)?;
        }

        if let Some(live) = live_sample {
            let values = live(ref_, col)?;
            return Ok(SampleValuesResult {
                values,
                message: None,
            });
        }

        Ok(SampleValuesResult {
            values: Vec::new(),
            message: Some(no_samples_message(ref_, col)),
        })
    }

    fn load_join_graph(&self) -> Result<JoinGraph, IndexError> {
        let base = self.base_dir();
        let Some(manifest) = self.store.read_manifest(&base)? else {
            return Err(IndexError::Query(format!(
                "Index manifest not found for database \"{}\"",
                self.database
            )));
        };
        self.store
            .read_join_graph(&base, &manifest)?
            .ok_or_else(|| {
                IndexError::Query(format!(
                    "Join graph not found for database \"{}\"",
                    self.database
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        BuildDepth, BuildMode, ColumnEntry, ColumnOverride, ColumnProfile, DbObjectKind,
        IndexCounts, IndexDerived, IndexManifest, IndexScope, IndexStats, JoinEdge, ObjectOverride,
        ObjectShard, ValueHit,
    };
    use crate::store::{JOIN_GRAPH_FILE, TOKENS_FILE, VALUES_FILE};
    use tempfile::TempDir;

    const CONN: &str = "conn-1";
    const DB: &str = "appdb";

    fn sample_manifest(tables: u64) -> IndexManifest {
        IndexManifest {
            format_version: 1,
            connection_id: CONN.into(),
            database: DB.into(),
            indexed_at: "2026-07-04T00:00:00.000Z".into(),
            build_mode: BuildMode::Auto,
            build_depth: BuildDepth::Structure,
            schema_fingerprint: "1|2|3|4|5".into(),
            pg_version: "16.0".into(),
            environment: "development".into(),
            scope: IndexScope {
                included_schemas: vec!["public".into()],
                excluded_objects: vec![],
                pii_excluded_columns: vec![],
            },
            counts: IndexCounts {
                tables,
                views: 0,
                functions: 0,
                enums: 0,
            },
            shards: vec![ObjectShard {
                file: "objects-public-0.json".into(),
                schema: "public".into(),
                objects: 2,
                bytes: 512,
                hash: "abc".into(),
            }],
            derived: IndexDerived {
                tokens: TOKENS_FILE.into(),
                join_graph: JOIN_GRAPH_FILE.into(),
                values: Some(VALUES_FILE.into()),
                embeddings: None,
                embeddings_meta: None,
            },
            stats: IndexStats {
                build_ms: 1,
                queries_run: 1,
                warnings: vec![],
            },
        }
    }

    fn table_entry(oid: u32, cols: Vec<ColumnEntry>) -> ObjectEntry {
        ObjectEntry {
            kind: DbObjectKind::Table,
            oid,
            object_hash: format!("hash{oid}"),
            comment: None,
            row_estimate: 10.0,
            size_bytes: 8192,
            columns: cols,
            primary_key: Some(vec!["id".into()]),
            foreign_keys: None,
            indexes: None,
            checks: None,
            excluded: None,
            definition: None,
            signature: None,
            language: None,
            volatility: None,
            body: None,
            values: None,
            base_type: None,
            constraint: None,
        }
    }

    fn col(name: &str, profile: Option<ColumnProfile>) -> ColumnEntry {
        ColumnEntry {
            name: name.into(),
            type_name: "text".into(),
            not_null: false,
            default_value: None,
            comment: None,
            ordinal: 1,
            is_pk: None,
            profile,
            pii: None,
        }
    }

    /// Mirrors `IndexQueryService.test.ts` `createTokens` — order outranks customer.
    fn fixture_tokens() -> TokenIndex {
        TokenIndex {
            version: 1,
            df: HashMap::from([("order".into(), 1.0), ("customer".into(), 1.0)]),
            postings: HashMap::from([
                ("order".into(), vec![("public.orders".into(), 5.0)]),
                ("customer".into(), vec![("public.customers".into(), 3.0)]),
            ]),
            synonyms: HashMap::new(),
        }
    }

    fn write_fixture(store: &IndexStore) -> PathBuf {
        let base = store.base_dir(CONN, DB);
        let manifest = sample_manifest(10);
        store.write_manifest(&base, &manifest).unwrap();
        store.write_tokens(&base, &fixture_tokens()).unwrap();

        let mut shard = HashMap::new();
        shard.insert(
            "public.orders".into(),
            table_entry(1, vec![col("id", None), col("status", None)]),
        );
        shard.insert(
            "public.customers".into(),
            table_entry(
                2,
                vec![col(
                    "status",
                    Some(ColumnProfile {
                        n_distinct: 3.0,
                        null_frac: 0.0,
                        common_values: Some(vec![
                            "active".into(),
                            "churned".into(),
                            "trial".into(),
                        ]),
                        min: None,
                        max: None,
                    }),
                )],
            ),
        );
        store
            .write_shard_entries(&base, "objects-public-0.json", &shard)
            .unwrap();

        let graph = JoinGraph {
            edges: vec![JoinEdge {
                from: "public.orders".into(),
                to: "public.customers".into(),
                via: "orders_customer_id_fkey".into(),
                cols: vec![("customer_id".into(), "id".into())],
                inferred: None,
                disabled: None,
            }],
        };
        store.write_join_graph(&base, &graph).unwrap();

        let mut values = ValueIndex::new();
        values.insert(
            "active".into(),
            vec![ValueHit {
                ref_: "public.customers".into(),
                col: "status".into(),
            }],
        );
        store.write_values(&base, &values).unwrap();

        base
    }

    #[test]
    fn search_schema_ranks_order_above_customer() {
        // Port of IndexQueryService.test.ts: stays lexical by default.
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_fixture(&store);
        let svc = IndexQueryService::new(&store, CONN, DB);

        let hits = svc.search_schema("order customer", 10, None).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.ref_.as_str()).collect::<Vec<_>>(),
            vec!["public.orders", "public.customers"]
        );
        assert!(hits[0].score > hits[1].score);
        assert_eq!(hits[0].kind, "table");
    }

    #[test]
    fn search_schema_respects_excluded_override() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let base = write_fixture(&store);

        let overrides = IndexOverrides {
            joins: None,
            synonyms: None,
            objects: Some(HashMap::from([(
                "public.orders".into(),
                ObjectOverride {
                    excluded: Some(true),
                    ..Default::default()
                },
            )])),
        };
        store.write_overrides(&base, &overrides).unwrap();

        let svc = IndexQueryService::new(&store, CONN, DB);
        let hits = svc.search_schema("order customer", 10, None).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.ref_.as_str()).collect::<Vec<_>>(),
            vec!["public.customers"]
        );
    }

    #[test]
    fn search_schema_respects_policy_filter() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_fixture(&store);
        let svc = IndexQueryService::new(&store, CONN, DB);

        let filter = QueryPolicyFilter {
            deny_tables: vec!["public.orders".into()],
            ..Default::default()
        };
        let hits = svc
            .search_schema("order customer", 10, Some(&filter))
            .unwrap();
        assert_eq!(
            hits.iter().map(|h| h.ref_.as_str()).collect::<Vec<_>>(),
            vec!["public.customers"]
        );
    }

    #[test]
    fn search_schema_value_boost_can_rerank() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_fixture(&store);
        let svc = IndexQueryService::new(&store, CONN, DB);

        // Token "active" only appears in values.json → boosts customers.
        let hits = svc.search_schema("active", 10, None).unwrap();
        assert_eq!(hits[0].ref_, "public.customers");
        assert!((hits[0].score - VALUE_HIT_BOOST).abs() < 1e-9);
    }

    #[test]
    fn describe_object_loads_entry() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_fixture(&store);
        let svc = IndexQueryService::new(&store, CONN, DB);

        let entry = svc.describe_object("public.orders", None).unwrap();
        assert_eq!(entry.kind, DbObjectKind::Table);
        assert_eq!(entry.oid, 1);
    }

    #[test]
    fn describe_object_missing_has_actionable_error() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_fixture(&store);
        let svc = IndexQueryService::new(&store, CONN, DB);

        let err = svc.describe_object("public.missing", None).unwrap_err();
        let msg = err.to_string();
        assert_eq!(msg, missing_object_message("public.missing"));
        assert!(msg.contains("call search_schema"));
    }

    #[test]
    fn get_join_path_direct_fk() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_fixture(&store);
        let svc = IndexQueryService::new(&store, CONN, DB);

        let path = svc
            .get_join_path("public.orders", "public.customers")
            .unwrap();
        assert_eq!(path.len(), 1);
        assert_eq!(path[0].via, "orders_customer_id_fkey");
    }

    #[test]
    fn get_join_path_unreachable_message() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_fixture(&store);
        let svc = IndexQueryService::new(&store, CONN, DB);

        let err = svc
            .get_join_path("public.orders", "public.orphan")
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            joins::unreachable_join_path_message("public.orders", "public.orphan")
        );
    }

    #[test]
    fn sample_values_from_column_profile() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_fixture(&store);
        let svc = IndexQueryService::new(&store, CONN, DB);

        let result = svc
            .sample_values("public.customers", "status", None, None)
            .unwrap();
        assert_eq!(result.values, vec!["active", "churned", "trial"]);
        assert!(result.message.is_none());
    }

    #[test]
    fn sample_values_empty_friendly_message() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_fixture(&store);
        let svc = IndexQueryService::new(&store, CONN, DB);

        let result = svc
            .sample_values("public.orders", "status", None, None)
            .unwrap();
        assert!(result.values.is_empty());
        assert_eq!(
            result.message.as_deref(),
            Some(no_samples_message("public.orders", "status").as_str())
        );
    }

    #[test]
    fn sample_values_live_callback() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        write_fixture(&store);
        let svc = IndexQueryService::new(&store, CONN, DB);

        let live = |ref_: &str, col: &str| {
            assert_eq!(ref_, "public.orders");
            assert_eq!(col, "status");
            Ok(vec!["pending".into(), "shipped".into()])
        };
        let result = svc
            .sample_values("public.orders", "status", None, Some(&live))
            .unwrap();
        assert_eq!(result.values, vec!["pending", "shipped"]);
    }

    #[test]
    fn sample_values_denies_pii_override() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let base = write_fixture(&store);

        let overrides = IndexOverrides {
            joins: None,
            synonyms: None,
            objects: Some(HashMap::from([(
                "public.customers".into(),
                ObjectOverride {
                    columns: Some(HashMap::from([(
                        "status".into(),
                        ColumnOverride {
                            pii: Some(true),
                            ..Default::default()
                        },
                    )])),
                    ..Default::default()
                },
            )])),
        };
        store.write_overrides(&base, &overrides).unwrap();

        let svc = IndexQueryService::new(&store, CONN, DB);
        let err = svc
            .sample_values("public.customers", "status", None, None)
            .unwrap_err();
        assert!(err.to_string().contains("PII"));
    }

    #[test]
    fn search_schema_lexical_helper_truncates() {
        let tokens = fixture_tokens();
        let hits = search_schema_lexical(
            "order customer",
            &tokens,
            &HashMap::new(),
            TableCounts { tables: 10 },
            1,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "public.orders");
    }
}
