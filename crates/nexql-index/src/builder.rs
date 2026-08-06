//! Index builder — port of `pro/src/features/dbindex/IndexBuilder.ts`.
//!
//! Catalog access is abstracted behind [`CatalogDb`] so unit tests can inject
//! fixtures without Postgres. Live builds use [`PgCatalogDb`].
//!
//! Phase 3 golden gate: `tests/golden_parity.rs` + `tests/golden/expected/`.
//! Phase 5: optional embeddings via [`Embedder`] / feature `embeddings`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
#[cfg(not(feature = "embeddings"))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use async_trait::async_trait;
use tokio_postgres::Client;

use crate::catalog::{
    COLUMNS_QUERY, CONSTRAINTS_QUERY, DOMAINS_QUERY, ENUMS_QUERY, FUNCTIONS_QUERY, INDEXES_QUERY,
    RELATIONS_QUERY, RawColumnRow, RawConstraintRow, RawDomainRow, RawEnumRow, RawFunctionRow,
    RawIndexRow, RawRelationRow, RawViewRow, SCHEMA_FINGERPRINT_QUERY, VIEW_DEFINITIONS_QUERY,
    map_relkind_to_db_object_kind,
};
#[cfg(not(feature = "embeddings"))]
use crate::embed::LOCAL_MODEL_ID;
use crate::embed::{Embedder, build_object_doc, embeddings_env_local, is_embeddable_kind};
use crate::error::IndexError;
use crate::lexical::{extract_synonyms_from_comment, tokenize};
use crate::migrate::CURRENT_FORMAT_VERSION;
use crate::model::{
    BuildDepth, BuildMode, CheckEntry, ColumnEntry, EmbeddingMetaEntry, ForeignKeyEntry,
    IndexCounts, IndexDerived, IndexEntry, IndexManifest, IndexScope, IndexStats, JoinEdge,
    JoinGraph, ObjectEntry, ObjectShard, TokenIndex,
};
use crate::object_hash::{compute_definition_hash, compute_object_hash};
use crate::store::{
    EMBEDDINGS_BIN, EMBEDDINGS_META, IndexStore, JOIN_GRAPH_FILE, LOCK_FILE, TOKENS_FILE,
    VALUES_FILE, serialize_embeddings,
};

#[cfg(not(feature = "embeddings"))]
static EMBEDDINGS_FEATURE_WARNED: AtomicBool = AtomicBool::new(false);

/// Max objects per shard — matches TS IndexBuilder.
pub const MAX_OBJECTS_PER_SHARD: usize = 64;

/// Max shard UTF-8 size before flush — matches TS (`256 * 1024`).
pub const MAX_SHARD_BYTES: usize = 256 * 1024;

/// Progress events for MCP / CLI reporting (Phase 4 wires these to MCP progress).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildProgress {
    Started,
    FetchingCatalog { queries_run: u64 },
    MappingObjects { count: usize },
    WritingShards { shard_count: usize },
    WritingDerived,
    Finished { build_ms: u64 },
}

/// Parameters for a single index build.
#[derive(Debug, Clone)]
pub struct BuildRequest {
    pub connection_id: String,
    pub database: String,
    pub scope: IndexScope,
    pub depth: BuildDepth,
    pub build_mode: BuildMode,
    pub environment: String,
    /// When true (or `NEXQL_MCP_EMBEDDINGS=local`), attempt to write embeddings.
    pub embeddings: bool,
}

/// Catalog query surface used by the builder.
#[async_trait]
pub trait CatalogDb: Send + Sync {
    async fn set_statement_timeout_ms(&self, ms: u32) -> Result<(), IndexError>;
    async fn schema_fingerprint(&self) -> Result<String, IndexError>;
    async fn server_version(&self) -> Result<String, IndexError>;
    async fn relations(&self, schemas: &[String]) -> Result<Vec<RawRelationRow>, IndexError>;
    async fn columns(&self, oids: &[i32]) -> Result<Vec<RawColumnRow>, IndexError>;
    async fn constraints(&self, oids: &[i32]) -> Result<Vec<RawConstraintRow>, IndexError>;
    async fn indexes(&self, oids: &[i32]) -> Result<Vec<RawIndexRow>, IndexError>;
    async fn view_definitions(&self, oids: &[i32]) -> Result<Vec<RawViewRow>, IndexError>;
    async fn functions(&self, schemas: &[String]) -> Result<Vec<RawFunctionRow>, IndexError>;
    async fn enums(&self, schemas: &[String]) -> Result<Vec<RawEnumRow>, IndexError>;
    async fn domains(&self, schemas: &[String]) -> Result<Vec<RawDomainRow>, IndexError>;
    async fn non_system_schemas(&self) -> Result<Vec<String>, IndexError>;
}

/// Live Postgres adapter for [`CatalogDb`].
pub struct PgCatalogDb<'a> {
    client: &'a Client,
}

impl<'a> PgCatalogDb<'a> {
    pub fn new(client: &'a Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl CatalogDb for PgCatalogDb<'_> {
    async fn set_statement_timeout_ms(&self, ms: u32) -> Result<(), IndexError> {
        self.client
            .execute(&format!("SET statement_timeout = {ms}"), &[])
            .await
            .map_err(|e| IndexError::Db(format!("SET statement_timeout: {e}")))?;
        Ok(())
    }

    async fn schema_fingerprint(&self) -> Result<String, IndexError> {
        let row = self
            .client
            .query_one(SCHEMA_FINGERPRINT_QUERY, &[])
            .await
            .map_err(|e| IndexError::Db(format!("SCHEMA_FINGERPRINT_QUERY: {e}")))?;
        Ok(format_schema_fingerprint(
            &row.get::<_, String>(0),
            &row.get::<_, String>(1),
            &row.get::<_, String>(2),
            &row.get::<_, String>(3),
            &row.get::<_, String>(4),
        ))
    }

    async fn server_version(&self) -> Result<String, IndexError> {
        let row = self
            .client
            .query_one("SHOW server_version", &[])
            .await
            .map_err(|e| IndexError::Db(format!("SHOW server_version: {e}")))?;
        let full: String = row.get(0);
        Ok(full.split_whitespace().next().unwrap_or("16.0").to_owned())
    }

    async fn relations(&self, schemas: &[String]) -> Result<Vec<RawRelationRow>, IndexError> {
        let schemas: Vec<&str> = schemas.iter().map(String::as_str).collect();
        let rows = self
            .client
            .query(RELATIONS_QUERY, &[&schemas])
            .await
            .map_err(|e| IndexError::Db(format!("RELATIONS_QUERY: {e}")))?;
        Ok(rows.iter().map(map_relation_row).collect())
    }

    async fn columns(&self, oids: &[i32]) -> Result<Vec<RawColumnRow>, IndexError> {
        let oids: Vec<u32> = oids.iter().map(|&o| o as u32).collect();
        let rows = self
            .client
            .query(COLUMNS_QUERY, &[&oids])
            .await
            .map_err(|e| IndexError::Db(format!("COLUMNS_QUERY: {e}")))?;
        Ok(rows
            .iter()
            .map(|r| RawColumnRow {
                table_oid: r.get("table_oid"),
                name: r.get("name"),
                type_name: r.get("type"),
                not_null: r.get("not_null"),
                default_value: r.get("default_value"),
                comment: r.get("comment"),
                ordinal: {
                    let n: i16 = r.get("ordinal");
                    i32::from(n)
                },
            })
            .collect())
    }

    async fn constraints(&self, oids: &[i32]) -> Result<Vec<RawConstraintRow>, IndexError> {
        let oids: Vec<u32> = oids.iter().map(|&o| o as u32).collect();
        let rows = self
            .client
            .query(CONSTRAINTS_QUERY, &[&oids])
            .await
            .map_err(|e| IndexError::Db(format!("CONSTRAINTS_QUERY: {e}")))?;
        Ok(rows
            .iter()
            .map(|r| RawConstraintRow {
                table_oid: r.get("table_oid"),
                name: r.get("name"),
                type_name: pg_char_col(r, "type"),
                definition: r.get("definition"),
                ref_table_oid: r.get("ref_table_oid"),
                key_positions: r.get("key_positions"),
                ref_key_positions: r.get("ref_key_positions"),
            })
            .collect())
    }

    async fn indexes(&self, oids: &[i32]) -> Result<Vec<RawIndexRow>, IndexError> {
        let oids: Vec<u32> = oids.iter().map(|&o| o as u32).collect();
        let rows = self
            .client
            .query(INDEXES_QUERY, &[&oids])
            .await
            .map_err(|e| IndexError::Db(format!("INDEXES_QUERY: {e}")))?;
        Ok(rows
            .iter()
            .map(|r| RawIndexRow {
                table_oid: r.get("table_oid"),
                name: r.get("name"),
                unique: r.get("unique"),
                method: r.get("method"),
                definition: r.get("definition"),
                key_positions: r.get("key_positions"),
            })
            .collect())
    }

    async fn view_definitions(&self, oids: &[i32]) -> Result<Vec<RawViewRow>, IndexError> {
        let oids: Vec<u32> = oids.iter().map(|&o| o as u32).collect();
        let rows = self
            .client
            .query(VIEW_DEFINITIONS_QUERY, &[&oids])
            .await
            .map_err(|e| IndexError::Db(format!("VIEW_DEFINITIONS_QUERY: {e}")))?;
        Ok(rows
            .iter()
            .map(|r| RawViewRow {
                oid: r.get("oid"),
                definition: r.get("definition"),
            })
            .collect())
    }

    async fn functions(&self, schemas: &[String]) -> Result<Vec<RawFunctionRow>, IndexError> {
        let schemas: Vec<&str> = schemas.iter().map(String::as_str).collect();
        let rows = self.client.query(FUNCTIONS_QUERY, &[&schemas]).await?;
        Ok(rows
            .iter()
            .map(|r| RawFunctionRow {
                oid: r.get("oid"),
                schema_name: r.get("schema_name"),
                name: r.get("name"),
                arguments: r.get("arguments"),
                result_type: r.get("result_type"),
                language: r.get("language"),
                volatility: pg_char_col(r, "volatility"),
                body: r.get("body"),
                comment: r.get("comment"),
            })
            .collect())
    }

    async fn enums(&self, schemas: &[String]) -> Result<Vec<RawEnumRow>, IndexError> {
        let schemas: Vec<&str> = schemas.iter().map(String::as_str).collect();
        let rows = self.client.query(ENUMS_QUERY, &[&schemas]).await?;
        Ok(rows
            .iter()
            .map(|r| RawEnumRow {
                oid: r.get("oid"),
                schema_name: r.get("schema_name"),
                name: r.get("name"),
                value: r.get("value"),
                sort_order: r.get("sort_order"),
            })
            .collect())
    }

    async fn domains(&self, schemas: &[String]) -> Result<Vec<RawDomainRow>, IndexError> {
        let schemas: Vec<&str> = schemas.iter().map(String::as_str).collect();
        let rows = self.client.query(DOMAINS_QUERY, &[&schemas]).await?;
        Ok(rows
            .iter()
            .map(|r| RawDomainRow {
                oid: r.get("oid"),
                schema_name: r.get("schema_name"),
                name: r.get("name"),
                base_type: r.get("base_type"),
                constraint_name: r.get("constraint_name"),
                constraint_definition: r.get("constraint_definition"),
            })
            .collect())
    }

    async fn non_system_schemas(&self) -> Result<Vec<String>, IndexError> {
        let rows = self
            .client
            .query(crate::catalog::NON_SYSTEM_SCHEMAS_QUERY, &[])
            .await
            .map_err(|e| IndexError::Db(format!("NON_SYSTEM_SCHEMAS_QUERY: {e}")))?;
        Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
    }
}

/// Format fingerprint pipe-string — matches TS `fetchSchemaFingerprint`.
pub fn format_schema_fingerprint(
    object_count: &str,
    max_oid: &str,
    total_rows_estimate: &str,
    schema_count: &str,
    max_schema_oid: &str,
) -> String {
    format!("{object_count}|{max_oid}|{total_rows_estimate}|{schema_count}|{max_schema_oid}")
}

/// Build a full schema index under `store` for `req`.
///
/// When embeddings are requested (`req.embeddings` or env `NEXQL_MCP_EMBEDDINGS=local`)
/// and `embedder` is provided (or the `embeddings` feature can load MiniLM), writes
/// `embeddings.bin` + `embeddings-meta.json`. Without the feature and without an
/// injected embedder, skips with a one-time warning.
pub async fn build_index<D: CatalogDb + ?Sized>(
    store: &IndexStore,
    db: &D,
    req: &BuildRequest,
    mut progress: Option<&mut (dyn FnMut(BuildProgress) + Send)>,
    cancel: Option<&(dyn Fn() -> bool + Sync)>,
    embedder: Option<&dyn Embedder>,
) -> Result<IndexManifest, IndexError> {
    let base_dir = store.base_dir(&req.connection_id, &req.database);
    let started = Instant::now();

    let lock_file = store.acquire_lock(&base_dir)?;
    let _guard = LockGuard {
        _file: lock_file,
        base: base_dir.clone(),
    };

    report(&mut progress, BuildProgress::Started);
    check_cancel(cancel)?;

    let mut queries_run: u64 = 0;
    let mut warnings: Vec<String> = Vec::new();

    db.set_statement_timeout_ms(5_000).await?;
    queries_run += 1;

    let schema_fingerprint = db.schema_fingerprint().await?;
    queries_run += 1;

    let pg_version = db.server_version().await?;
    queries_run += 1;

    report(
        &mut progress,
        BuildProgress::FetchingCatalog { queries_run },
    );
    check_cancel(cancel)?;

    // Auto-discover non-system schemas; fall back to `public` if none found.
    let auto_discovered = req.scope.included_schemas.is_empty();
    let schemas = if req.scope.included_schemas.is_empty() {
        let discovered = db.non_system_schemas().await?;
        if discovered.is_empty() {
            vec!["public".to_owned()]
        } else {
            discovered
        }
    } else {
        req.scope.included_schemas.clone()
    };

    let relations = db.relations(&schemas).await?;
    queries_run += 1;

    // When schemas were auto-discovered but none contain user relations (e.g.
    // only extension schemas like `cron` are present), fall back to `public`
    // with a warning rather than aborting — this handles freshly provisioned
    // databases where migrations haven't run yet.
    let (schemas, relations) =
        if relations.is_empty() && auto_discovered && schemas != vec!["public".to_owned()] {
            warnings.push(format!(
                "Discovered schemas {:?} contain no user objects — retrying with 'public'",
                schemas
            ));
            let public_relations = db.relations(&["public".to_owned()]).await?;
            queries_run += 1;
            if public_relations.is_empty() {
                return Err(IndexError::Build(
                    "No objects found in any schema (tried discovered schemas and 'public')".into(),
                ));
            }
            (vec!["public".to_owned()], public_relations)
        } else if relations.is_empty() {
            return Err(IndexError::Build(
                "No objects found in specified schemas".into(),
            ));
        } else {
            (schemas, relations)
        };

    let excluded: HashSet<&str> = req
        .scope
        .excluded_objects
        .iter()
        .map(String::as_str)
        .collect();
    let filtered: Vec<RawRelationRow> = relations
        .into_iter()
        .filter(|r| !excluded.contains(format!("{}.{}", r.schema_name, r.name).as_str()))
        .collect();

    let oids: Vec<i32> = filtered.iter().map(|r| r.oid).collect();

    check_cancel(cancel)?;
    let columns = db.columns(&oids).await?;
    queries_run += 1;
    check_cancel(cancel)?;
    let constraints = db.constraints(&oids).await?;
    queries_run += 1;
    check_cancel(cancel)?;
    let indexes = db.indexes(&oids).await?;
    queries_run += 1;
    check_cancel(cancel)?;
    let views = db.view_definitions(&oids).await?;
    queries_run += 1;
    check_cancel(cancel)?;
    let functions = db.functions(&schemas).await?;
    queries_run += 1;
    check_cancel(cancel)?;
    let enums = db.enums(&schemas).await?;
    queries_run += 1;
    check_cancel(cancel)?;
    let domains = db.domains(&schemas).await?;
    queries_run += 1;

    report(
        &mut progress,
        BuildProgress::FetchingCatalog { queries_run },
    );

    let mut entries = assemble_entries(
        &filtered,
        &columns,
        &constraints,
        &indexes,
        &views,
        &functions,
        &enums,
        &domains,
        &req.scope,
    );

    if req.depth == BuildDepth::Profiles {
        warnings.push(
            "BuildDepth::Profiles value profiling not yet ported — structure-only this pass".into(),
        );
    }

    for entry in entries.values_mut() {
        entry.object_hash = compute_object_hash(entry);
    }

    report(
        &mut progress,
        BuildProgress::MappingObjects {
            count: entries.len(),
        },
    );
    check_cancel(cancel)?;

    let previous_manifest = store.read_manifest(&base_dir)?;
    let shards = write_shards(store, &base_dir, &entries, previous_manifest.as_ref())?;
    report(
        &mut progress,
        BuildProgress::WritingShards {
            shard_count: shards.len(),
        },
    );

    report(&mut progress, BuildProgress::WritingDerived);
    let token_index = build_token_index(&entries);
    store.write_tokens(&base_dir, &token_index)?;

    // Empty value index until profiling lands.
    let value_index: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    let values_bytes = serde_json::to_vec(&value_index)?;
    store.write_atomic(&base_dir.join(VALUES_FILE), &values_bytes)?;

    let join_graph = build_join_graph(&entries);
    store.write_join_graph(&base_dir, &join_graph)?;

    let counts = count_objects(&entries);
    let build_ms = started.elapsed().as_millis() as u64;

    let (embeddings_ref, embeddings_meta_ref) =
        maybe_write_embeddings(store, &base_dir, &entries, req, embedder, &mut warnings)?;

    let manifest = IndexManifest {
        format_version: CURRENT_FORMAT_VERSION,
        connection_id: req.connection_id.clone(),
        database: req.database.clone(),
        indexed_at: indexed_at_iso(),
        build_mode: req.build_mode,
        build_depth: req.depth,
        schema_fingerprint,
        pg_version,
        environment: req.environment.clone(),
        scope: req.scope.clone(),
        counts,
        shards,
        derived: IndexDerived {
            tokens: TOKENS_FILE.to_owned(),
            join_graph: JOIN_GRAPH_FILE.to_owned(),
            values: Some(VALUES_FILE.to_owned()),
            embeddings: embeddings_ref,
            embeddings_meta: embeddings_meta_ref,
        },
        stats: IndexStats {
            build_ms,
            queries_run,
            warnings,
        },
    };

    store.write_manifest(&base_dir, &manifest)?;
    store.run_garbage_collection(&base_dir, &manifest)?;

    report(&mut progress, BuildProgress::Finished { build_ms });
    Ok(manifest)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

struct LockGuard {
    _file: std::fs::File,
    base: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.base.join(LOCK_FILE));
    }
}

fn report(progress: &mut Option<&mut (dyn FnMut(BuildProgress) + Send)>, event: BuildProgress) {
    if let Some(cb) = progress.as_mut() {
        cb(event);
    }
}

fn check_cancel(cancel: Option<&(dyn Fn() -> bool + Sync)>) -> Result<(), IndexError> {
    if cancel.is_some_and(|f| f()) {
        return Err(IndexError::Cancelled);
    }
    Ok(())
}

fn want_embeddings(req: &BuildRequest) -> bool {
    req.embeddings || embeddings_env_local()
}

/// Write embeddings.bin + meta when requested. Returns (embeddings, embeddings_meta) paths.
fn maybe_write_embeddings(
    store: &IndexStore,
    base_dir: &Path,
    entries: &BTreeMap<String, ObjectEntry>,
    req: &BuildRequest,
    embedder: Option<&dyn Embedder>,
    warnings: &mut Vec<String>,
) -> Result<(Option<String>, Option<String>), IndexError> {
    if !want_embeddings(req) {
        return Ok((None, None));
    }

    if let Some(e) = embedder {
        return write_embeddings_with(store, base_dir, entries, e, warnings);
    }

    #[cfg(feature = "embeddings")]
    {
        match crate::embed::MiniLmEmbedder::load() {
            Ok(m) => write_embeddings_with(store, base_dir, entries, &m, warnings),
            Err(e) => {
                warnings.push(format!("Generating embeddings failed: {e}"));
                Ok((None, None))
            }
        }
    }
    #[cfg(not(feature = "embeddings"))]
    {
        let _ = LOCAL_MODEL_ID;
        if !EMBEDDINGS_FEATURE_WARNED.swap(true, Ordering::Relaxed) {
            let msg = "embeddings requested but nexql-index was built without the `embeddings` feature — skipping";
            warnings.push(msg.into());
            eprintln!("warning: {msg}");
        }
        Ok((None, None))
    }
}

fn write_embeddings_with(
    store: &IndexStore,
    base_dir: &Path,
    entries: &BTreeMap<String, ObjectEntry>,
    embedder: &dyn Embedder,
    warnings: &mut Vec<String>,
) -> Result<(Option<String>, Option<String>), IndexError> {
    let mut candidates: Vec<(String, String)> = entries
        .iter()
        .filter(|(_, e)| is_embeddable_kind(e.kind))
        .map(|(ref_, e)| (ref_.clone(), build_object_doc(ref_, e)))
        .collect();
    candidates.sort_by(|a, b| a.0.cmp(&b.0));

    if candidates.is_empty() {
        return Ok((None, None));
    }

    let dim = embedder.dim();
    let model = embedder.model_id().to_owned();

    let existing_map: HashMap<String, (String, Vec<f32>)> = store
        .read_manifest(base_dir)
        .ok()
        .flatten()
        .and_then(|m| store.read_embeddings(base_dir, &m).ok().flatten())
        .map(|(meta, bin)| {
            let mut map = HashMap::new();
            if bin.len() % (dim * 4) == 0 {
                let count = bin.len() / (dim * 4);
                for (idx, entry) in meta.into_iter().enumerate() {
                    if idx < count && entry.dim as usize == dim && entry.model == model {
                        let start = idx * dim * 4;
                        let end = start + dim * 4;
                        let vec: Vec<f32> = bin[start..end]
                            .chunks_exact(4)
                            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
                            .collect();
                        map.insert(entry.ref_, (entry.object_hash, vec));
                    }
                }
            }
            map
        })
        .unwrap_or_default();

    let mut vectors = Vec::with_capacity(candidates.len());
    let mut to_embed_indices = Vec::new();
    let mut to_embed_texts = Vec::new();

    for (idx, (ref_, _doc)) in candidates.iter().enumerate() {
        let current_hash = entries
            .get(ref_)
            .map(|e| e.object_hash.as_str())
            .unwrap_or_default();
        if let Some((old_hash, old_vec)) = existing_map.get(ref_) {
            if old_hash == current_hash {
                vectors.push(Some(old_vec.clone()));
                continue;
            }
        }
        vectors.push(None);
        to_embed_indices.push(idx);
        to_embed_texts.push(candidates[idx].1.as_str());
    }

    if !to_embed_texts.is_empty() {
        let new_vectors = match embedder.embed_batch(&to_embed_texts) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("generating embeddings failed: {e}"));
                return Ok((None, None));
            }
        };
        for (idx_in_batch, &cand_idx) in to_embed_indices.iter().enumerate() {
            vectors[cand_idx] = Some(new_vectors[idx_in_batch].clone());
        }
    }

    let final_vectors: Vec<Vec<f32>> = vectors.into_iter().map(|v| v.unwrap()).collect();
    let bin = serialize_embeddings(&final_vectors, dim);
    let meta: Vec<EmbeddingMetaEntry> = candidates
        .iter()
        .map(|(ref_, _)| EmbeddingMetaEntry {
            ref_: ref_.clone(),
            object_hash: entries
                .get(ref_)
                .map(|e| e.object_hash.clone())
                .unwrap_or_default(),
            model: model.clone(),
            dim: dim as u32,
        })
        .collect();

    store.write_embeddings(base_dir, &meta, &bin)?;
    Ok((
        Some(EMBEDDINGS_BIN.to_owned()),
        Some(EMBEDDINGS_META.to_owned()),
    ))
}

/// Postgres `"char"` (e.g. `relkind`, `contype`) arrives as `i8` via tokio-postgres.
fn pg_char_col(r: &tokio_postgres::Row, col: &str) -> String {
    let b: i8 = r.get(col);
    char::from(b as u8).to_string()
}

fn map_relation_row(r: &tokio_postgres::Row) -> RawRelationRow {
    let row_estimate: i64 = r.get("row_estimate");
    let size_bytes: i64 = r.get("size_bytes");
    RawRelationRow {
        oid: r.get("oid"),
        schema_name: r.get("schema_name"),
        name: r.get("name"),
        kind: pg_char_col(r, "kind"),
        comment: r.get("comment"),
        row_estimate: serde_json::json!(row_estimate),
        size_bytes: serde_json::json!(size_bytes),
    }
}

fn parse_num(v: &serde_json::Value) -> f64 {
    match v {
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
        serde_json::Value::String(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn parse_u64(v: &serde_json::Value) -> u64 {
    parse_num(v).max(0.0) as u64
}

fn object_ref(schema: &str, name: &str) -> String {
    format!("{schema}.{name}")
}

// One raw-row slice per catalog category being merged into `ObjectEntry`s.
#[allow(clippy::too_many_arguments)]
fn assemble_entries(
    relations: &[RawRelationRow],
    columns: &[RawColumnRow],
    constraints: &[RawConstraintRow],
    indexes: &[RawIndexRow],
    views: &[RawViewRow],
    functions: &[RawFunctionRow],
    enums: &[RawEnumRow],
    domains: &[RawDomainRow],
    scope: &IndexScope,
) -> BTreeMap<String, ObjectEntry> {
    let mut entries: BTreeMap<String, ObjectEntry> = BTreeMap::new();
    let mut relation_map: HashMap<i32, &RawRelationRow> = HashMap::new();

    for rel in relations {
        relation_map.insert(rel.oid, rel);
        let ref_ = object_ref(&rel.schema_name, &rel.name);
        entries.insert(
            ref_,
            ObjectEntry {
                kind: map_relkind_to_db_object_kind(&rel.kind),
                oid: rel.oid as u32,
                object_hash: String::new(),
                comment: rel.comment.clone(),
                row_estimate: parse_num(&rel.row_estimate),
                size_bytes: parse_u64(&rel.size_bytes),
                columns: Vec::new(),
                primary_key: Some(Vec::new()),
                foreign_keys: Some(Vec::new()),
                indexes: Some(Vec::new()),
                checks: Some(Vec::new()),
                excluded: None,
                definition: None,
                signature: None,
                language: None,
                volatility: None,
                body: None,
                values: None,
                base_type: None,
                constraint: None,
            },
        );
    }

    let pii: HashSet<&str> = scope
        .pii_excluded_columns
        .iter()
        .map(String::as_str)
        .collect();

    let mut col_map: HashMap<i32, Vec<ColumnEntry>> = HashMap::new();
    for col in columns {
        let Some(rel) = relation_map.get(&col.table_oid) else {
            continue;
        };
        let ref_col = format!("{}.{}.{}", rel.schema_name, rel.name, col.name);
        let mut col_entry = ColumnEntry {
            name: col.name.clone(),
            type_name: col.type_name.clone(),
            not_null: col.not_null,
            default_value: col.default_value.clone(),
            comment: col.comment.clone(),
            ordinal: col.ordinal,
            is_pk: None,
            profile: None,
            pii: None,
        };
        if pii.contains(ref_col.as_str()) {
            col_entry.pii = Some(true);
        }
        col_map.entry(col.table_oid).or_default().push(col_entry);
    }

    for (table_oid, mut cols) in col_map {
        cols.sort_by_key(|c| c.ordinal);
        if let Some(rel) = relation_map.get(&table_oid) {
            let ref_ = object_ref(&rel.schema_name, &rel.name);
            if let Some(entry) = entries.get_mut(&ref_) {
                entry.columns = cols;
            }
        }
    }

    for con in constraints {
        let Some(rel) = relation_map.get(&con.table_oid) else {
            continue;
        };
        let ref_ = object_ref(&rel.schema_name, &rel.name);
        if !entries.contains_key(&ref_) {
            continue;
        }

        match con.type_name.as_str() {
            "p" => {
                let entry = entries.get_mut(&ref_).expect("present");
                let mut pk_cols = Vec::new();
                if let Some(ref positions) = con.key_positions {
                    for pos in positions {
                        if let Some(col) = entry.columns.iter_mut().find(|c| c.ordinal == *pos) {
                            col.is_pk = Some(true);
                            pk_cols.push(col.name.clone());
                        }
                    }
                }
                entry.primary_key = Some(pk_cols);
            }
            "f" => {
                let Some(ref_oid) = con.ref_table_oid else {
                    continue;
                };
                let Some(ref_rel) = relation_map.get(&ref_oid) else {
                    continue;
                };
                let (Some(key_pos), Some(ref_key_pos)) =
                    (&con.key_positions, &con.ref_key_positions)
                else {
                    continue;
                };
                let cols_list: Vec<String> = {
                    let entry = entries.get(&ref_).expect("present");
                    key_pos
                        .iter()
                        .filter_map(|pos| {
                            entry
                                .columns
                                .iter()
                                .find(|c| c.ordinal == *pos)
                                .map(|c| c.name.clone())
                        })
                        .collect()
                };
                let ref_table = object_ref(&ref_rel.schema_name, &ref_rel.name);
                let ref_cols_list: Vec<String> = entries
                    .get(&ref_table)
                    .map(|ref_entry| {
                        ref_key_pos
                            .iter()
                            .filter_map(|pos| {
                                ref_entry
                                    .columns
                                    .iter()
                                    .find(|c| c.ordinal == *pos)
                                    .map(|c| c.name.clone())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let entry = entries.get_mut(&ref_).expect("present");
                entry
                    .foreign_keys
                    .get_or_insert_with(Vec::new)
                    .push(ForeignKeyEntry {
                        name: con.name.clone(),
                        columns: cols_list,
                        ref_table,
                        ref_columns: ref_cols_list,
                        on_delete: None,
                        inferred: None,
                    });
            }
            "c" => {
                let entry = entries.get_mut(&ref_).expect("present");
                entry.checks.get_or_insert_with(Vec::new).push(CheckEntry {
                    name: con.name.clone(),
                    expr: con.definition.clone(),
                });
            }
            _ => {}
        }
    }

    for idx in indexes {
        let Some(rel) = relation_map.get(&idx.table_oid) else {
            continue;
        };
        let ref_ = object_ref(&rel.schema_name, &rel.name);
        let Some(entry) = entries.get_mut(&ref_) else {
            continue;
        };
        let idx_cols: Vec<String> = idx
            .key_positions
            .as_ref()
            .map(|positions| {
                positions
                    .iter()
                    .filter_map(|pos| {
                        entry
                            .columns
                            .iter()
                            .find(|c| c.ordinal == *pos)
                            .map(|c| c.name.clone())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let partial = if idx.definition.contains("WHERE") {
            Some(
                idx.definition
                    .split("WHERE")
                    .nth(1)
                    .map(|s| s.trim().to_owned()),
            )
        } else {
            Some(None)
        };
        entry.indexes.get_or_insert_with(Vec::new).push(IndexEntry {
            name: idx.name.clone(),
            columns: idx_cols,
            unique: idx.unique,
            method: idx.method.clone(),
            partial,
        });
    }

    for view in views {
        if let Some(rel) = relation_map.get(&view.oid) {
            let ref_ = object_ref(&rel.schema_name, &rel.name);
            if let Some(entry) = entries.get_mut(&ref_) {
                entry.definition = Some(view.definition.clone());
            }
        }
    }

    for fn_ in functions {
        let ref_ = object_ref(&fn_.schema_name, &fn_.name);
        entries.insert(
            ref_,
            ObjectEntry {
                kind: crate::model::DbObjectKind::Function,
                oid: fn_.oid as u32,
                object_hash: String::new(),
                comment: fn_.comment.clone(),
                row_estimate: 0.0,
                size_bytes: 0,
                columns: Vec::new(),
                primary_key: None,
                foreign_keys: None,
                indexes: None,
                checks: None,
                excluded: None,
                definition: None,
                signature: Some(format!(
                    "{}({}) RETURNS {}",
                    fn_.name, fn_.arguments, fn_.result_type
                )),
                language: Some(fn_.language.clone()),
                volatility: Some(fn_.volatility.clone()),
                body: Some(Some(fn_.body.clone())),
                values: None,
                base_type: None,
                constraint: None,
            },
        );
    }

    let mut enum_groups: BTreeMap<i32, Vec<&RawEnumRow>> = BTreeMap::new();
    for en in enums {
        enum_groups.entry(en.oid).or_default().push(en);
    }
    for (oid, rows) in enum_groups {
        let Some(first) = rows.first() else {
            continue;
        };
        let ref_ = object_ref(&first.schema_name, &first.name);
        entries.insert(
            ref_,
            ObjectEntry {
                kind: crate::model::DbObjectKind::Enum,
                oid: oid as u32,
                object_hash: String::new(),
                comment: None,
                row_estimate: 0.0,
                size_bytes: 0,
                columns: Vec::new(),
                primary_key: None,
                foreign_keys: None,
                indexes: None,
                checks: None,
                excluded: None,
                definition: None,
                signature: None,
                language: None,
                volatility: None,
                body: None,
                values: Some(rows.iter().map(|r| r.value.clone()).collect()),
                base_type: None,
                constraint: None,
            },
        );
    }

    for dom in domains {
        let ref_ = object_ref(&dom.schema_name, &dom.name);
        entries.insert(
            ref_,
            ObjectEntry {
                kind: crate::model::DbObjectKind::Domain,
                oid: dom.oid as u32,
                object_hash: String::new(),
                comment: None,
                row_estimate: 0.0,
                size_bytes: 0,
                columns: Vec::new(),
                primary_key: None,
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
                base_type: Some(dom.base_type.clone()),
                constraint: dom.constraint_definition.clone(),
            },
        );
    }

    entries
}

fn write_shards(
    store: &IndexStore,
    base_dir: &Path,
    entries: &BTreeMap<String, ObjectEntry>,
    previous: Option<&IndexManifest>,
) -> Result<Vec<ObjectShard>, IndexError> {
    let mut by_schema: BTreeMap<String, BTreeMap<String, ObjectEntry>> = BTreeMap::new();
    for (ref_, entry) in entries {
        let schema = ref_.split('.').next().unwrap_or("public").to_owned();
        by_schema
            .entry(schema)
            .or_default()
            .insert(ref_.clone(), entry.clone());
    }

    let mut shards = Vec::new();

    for (schema, objects) in by_schema {
        let mut shard_index: usize = 0;
        let mut current: BTreeMap<String, ObjectEntry> = BTreeMap::new();
        let mut current_size: usize = 0;

        let mut flush = |current: &mut BTreeMap<String, ObjectEntry>,
                         current_size: &mut usize,
                         shard_index: &mut usize|
         -> Result<(), IndexError> {
            if current.is_empty() {
                return Ok(());
            }
            let file = format!("objects-{schema}-{shard_index}.json");
            let content = serde_json::to_string(current)?;
            let bytes = content.len() as u64;
            let hash = compute_definition_hash(&content);

            let prev_hash = previous
                .and_then(|m| m.shards.iter().find(|s| s.file == file))
                .map(|s| s.hash.as_str());

            if prev_hash != Some(hash.as_str()) {
                store.write_atomic(&base_dir.join(&file), content.as_bytes())?;
            }

            shards.push(ObjectShard {
                file,
                schema: schema.clone(),
                objects: current.len() as u64,
                bytes,
                hash,
            });

            *shard_index += 1;
            current.clear();
            *current_size = 0;
            Ok(())
        };

        for (ref_, entry) in objects {
            let entry_str = serde_json::to_string(&entry)?;
            let entry_size = entry_str.len();

            if current.len() >= MAX_OBJECTS_PER_SHARD || current_size + entry_size > MAX_SHARD_BYTES
            {
                flush(&mut current, &mut current_size, &mut shard_index)?;
            }

            current.insert(ref_, entry);
            current_size += entry_size;
        }

        flush(&mut current, &mut current_size, &mut shard_index)?;
    }

    Ok(shards)
}

fn add_posting(token_index: &mut TokenIndex, token: &str, ref_: &str, weight: f64) {
    let postings = token_index.postings.entry(token.to_owned()).or_default();
    if let Some(match_) = postings.iter_mut().find(|(r, _)| r == ref_) {
        match_.1 = match_.1.max(weight);
    } else {
        postings.push((ref_.to_owned(), weight));
    }
}

fn add_synonym_pair(token_index: &mut TokenIndex, a: &str, b: &str) {
    let push = |map: &mut HashMap<String, Vec<String>>, from: &str, to: &str| {
        let list = map.entry(from.to_owned()).or_default();
        if !list.iter().any(|x| x == to) {
            list.push(to.to_owned());
        }
    };
    push(&mut token_index.synonyms, a, b);
    push(&mut token_index.synonyms, b, a);
}

fn build_token_index(entries: &BTreeMap<String, ObjectEntry>) -> TokenIndex {
    let mut token_index = TokenIndex {
        version: 1,
        df: HashMap::new(),
        postings: HashMap::new(),
        synonyms: HashMap::new(),
    };

    for (ref_, entry) in entries {
        let mut obj_tokens: HashSet<String> = HashSet::new();
        let name_part = ref_.split('.').nth(1).unwrap_or(ref_);

        for t in tokenize(name_part) {
            obj_tokens.insert(t.clone());
            add_posting(&mut token_index, &t, ref_, 3.0);
        }

        if let Some(ref comment) = entry.comment {
            for t in tokenize(comment) {
                obj_tokens.insert(t.clone());
                add_posting(&mut token_index, &t, ref_, 0.5);
            }
            for syn_str in extract_synonyms_from_comment(comment) {
                let syn_tokens = tokenize(&syn_str);
                let name_tokens = tokenize(name_part);
                for st in &syn_tokens {
                    for nt in &name_tokens {
                        add_synonym_pair(&mut token_index, st, nt);
                    }
                }
            }
        }

        for col in &entry.columns {
            let col_weight = if col.is_pk == Some(true) { 1.25 } else { 1.0 };
            for t in tokenize(&col.name) {
                obj_tokens.insert(t.clone());
                add_posting(&mut token_index, &t, ref_, col_weight);
            }
            if let Some(ref comment) = col.comment {
                for t in tokenize(comment) {
                    obj_tokens.insert(t.clone());
                    add_posting(&mut token_index, &t, ref_, 0.3);
                }
                for syn_str in extract_synonyms_from_comment(comment) {
                    let syn_tokens = tokenize(&syn_str);
                    let col_tokens = tokenize(&col.name);
                    for st in &syn_tokens {
                        for ct in &col_tokens {
                            add_synonym_pair(&mut token_index, st, ct);
                        }
                    }
                }
            }
        }

        for t in obj_tokens {
            *token_index.df.entry(t).or_insert(0.0) += 1.0;
        }
    }

    token_index
}

fn is_naming_match(col_prefix: &str, table_name: &str) -> bool {
    let p = col_prefix.to_ascii_lowercase();
    let t = table_name.to_ascii_lowercase();
    if p == t {
        return true;
    }
    if t == format!("{p}s") {
        return true;
    }
    if p == format!("{t}s") {
        return true;
    }
    if p.ends_with('y') && t == format!("{}ies", &p[..p.len() - 1]) {
        return true;
    }
    if t.ends_with('y') && p == format!("{}ies", &t[..t.len() - 1]) {
        return true;
    }
    if p.ends_with("ey") && t == format!("{p}s") {
        return true;
    }
    if p.ends_with("ss") && t == format!("{p}es") {
        return true;
    }
    if t.ends_with("ss") && p == format!("{t}es") {
        return true;
    }
    false
}

fn build_join_graph(entries: &BTreeMap<String, ObjectEntry>) -> JoinGraph {
    let mut edges = Vec::new();

    for (source_table, entry) in entries {
        if let Some(ref fks) = entry.foreign_keys {
            for fk in fks {
                let cols: Vec<(String, String)> = fk
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        (
                            c.clone(),
                            fk.ref_columns.get(i).cloned().unwrap_or_default(),
                        )
                    })
                    .collect();
                edges.push(JoinEdge {
                    from: source_table.clone(),
                    to: fk.ref_table.clone(),
                    via: fk.name.clone(),
                    cols,
                    inferred: None,
                    disabled: None,
                });
            }
        }
    }

    // Infer naming-convention joins when no FK exists (matches TS).
    let table_refs: Vec<&String> = entries
        .iter()
        .filter(|(_, e)| e.kind == crate::model::DbObjectKind::Table)
        .map(|(r, _)| r)
        .collect();

    for &ref_a in &table_refs {
        let entry_a = &entries[ref_a];
        let schema_a = ref_a.split('.').next().unwrap_or("public");
        let name_a = ref_a.split('.').nth(1).unwrap_or("");

        for col in &entry_a.columns {
            let lower = col.name.to_ascii_lowercase();
            if !lower.ends_with("_id") {
                continue;
            }
            let prefix = &col.name[..col.name.len().saturating_sub(3)];
            if prefix.is_empty() {
                continue;
            }

            for &ref_b in &table_refs {
                if ref_a == ref_b {
                    continue;
                }
                let entry_b = &entries[ref_b];
                let schema_b = ref_b.split('.').next().unwrap_or("public");
                if schema_a != schema_b {
                    continue;
                }
                let name_b = ref_b.split('.').nth(1).unwrap_or("");

                if !is_naming_match(prefix, name_b) {
                    continue;
                }

                let has_explicit = entry_a.foreign_keys.as_ref().is_some_and(|fks| {
                    fks.iter().any(|fk| {
                        fk.ref_table == *ref_b && fk.columns.iter().any(|c| c == &col.name)
                    })
                });
                if has_explicit {
                    continue;
                }

                let target_col = if entry_b.primary_key.as_ref().is_some_and(|pk| pk.len() == 1) {
                    entry_b.primary_key.as_ref().unwrap()[0].clone()
                } else if entry_b
                    .columns
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case("id"))
                {
                    "id".to_owned()
                } else if let Some(matched) = entry_b.columns.iter().find(|c| {
                    c.name
                        .eq_ignore_ascii_case(&format!("{}_id", name_b.to_ascii_lowercase()))
                }) {
                    matched.name.clone()
                } else {
                    continue;
                };

                let already = edges.iter().any(|e| {
                    e.from == *ref_a
                        && e.to == *ref_b
                        && e.cols
                            .iter()
                            .any(|(a, b)| a == &col.name && b == &target_col)
                });
                if already {
                    continue;
                }

                edges.push(JoinEdge {
                    from: ref_a.clone(),
                    to: ref_b.clone(),
                    via: format!("inferred:{name_a}.{}->{name_b}.{target_col}", col.name),
                    cols: vec![(col.name.clone(), target_col)],
                    inferred: Some(true),
                    disabled: None,
                });
            }
        }
    }

    JoinGraph { edges }
}

fn count_objects(entries: &BTreeMap<String, ObjectEntry>) -> IndexCounts {
    let mut counts = IndexCounts {
        tables: 0,
        views: 0,
        functions: 0,
        enums: 0,
    };
    use crate::model::DbObjectKind::*;
    for entry in entries.values() {
        match entry.kind {
            Table => counts.tables += 1,
            View | Matview => counts.views += 1,
            Function => counts.functions += 1,
            Enum => counts.enums += 1,
            _ => {}
        }
    }
    counts
}

fn indexed_at_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    // UTC formatting without chrono dep — sufficient for manifest telemetry.
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let hour = tod / 3600;
    let min = (tod % 3600) / 60;
    let sec = tod % 60;
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{millis:03}Z")
}

/// Howard Hinnant civil-from-days (UTC).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DbObjectKind;
    use async_trait::async_trait;
    use tempfile::TempDir;

    struct FixtureDb {
        fingerprint: String,
        version: String,
        relations: Vec<RawRelationRow>,
        columns: Vec<RawColumnRow>,
        constraints: Vec<RawConstraintRow>,
        indexes: Vec<RawIndexRow>,
        views: Vec<RawViewRow>,
        functions: Vec<RawFunctionRow>,
        enums: Vec<RawEnumRow>,
        domains: Vec<RawDomainRow>,
    }

    #[async_trait]
    impl CatalogDb for FixtureDb {
        async fn set_statement_timeout_ms(&self, _ms: u32) -> Result<(), IndexError> {
            Ok(())
        }
        async fn schema_fingerprint(&self) -> Result<String, IndexError> {
            Ok(self.fingerprint.clone())
        }
        async fn server_version(&self) -> Result<String, IndexError> {
            Ok(self.version.clone())
        }
        async fn relations(&self, _schemas: &[String]) -> Result<Vec<RawRelationRow>, IndexError> {
            Ok(self.relations.clone())
        }
        async fn columns(&self, _oids: &[i32]) -> Result<Vec<RawColumnRow>, IndexError> {
            Ok(self.columns.clone())
        }
        async fn constraints(&self, _oids: &[i32]) -> Result<Vec<RawConstraintRow>, IndexError> {
            Ok(self.constraints.clone())
        }
        async fn indexes(&self, _oids: &[i32]) -> Result<Vec<RawIndexRow>, IndexError> {
            Ok(self.indexes.clone())
        }
        async fn view_definitions(&self, _oids: &[i32]) -> Result<Vec<RawViewRow>, IndexError> {
            Ok(self.views.clone())
        }
        async fn functions(&self, _schemas: &[String]) -> Result<Vec<RawFunctionRow>, IndexError> {
            Ok(self.functions.clone())
        }
        async fn enums(&self, _schemas: &[String]) -> Result<Vec<RawEnumRow>, IndexError> {
            Ok(self.enums.clone())
        }
        async fn domains(&self, _schemas: &[String]) -> Result<Vec<RawDomainRow>, IndexError> {
            Ok(self.domains.clone())
        }
        async fn non_system_schemas(&self) -> Result<Vec<String>, IndexError> {
            let mut schemas: Vec<String> = self
                .relations
                .iter()
                .map(|r| r.schema_name.clone())
                .collect();
            schemas.sort();
            schemas.dedup();
            if schemas.is_empty() {
                schemas.push("public".to_string());
            }
            Ok(schemas)
        }
    }

    fn users_orgs_fixture() -> FixtureDb {
        FixtureDb {
            fingerprint: "2|100|50|1|11".into(),
            version: "16.2".into(),
            relations: vec![
                RawRelationRow {
                    oid: 10,
                    schema_name: "public".into(),
                    name: "orgs".into(),
                    kind: "r".into(),
                    comment: Some("organizations".into()),
                    row_estimate: serde_json::json!(5),
                    size_bytes: serde_json::json!(8192),
                },
                RawRelationRow {
                    oid: 20,
                    schema_name: "public".into(),
                    name: "users".into(),
                    kind: "r".into(),
                    comment: Some("app users aka members".into()),
                    row_estimate: serde_json::json!(100),
                    size_bytes: serde_json::json!(16384),
                },
                RawRelationRow {
                    oid: 30,
                    schema_name: "public".into(),
                    name: "orders".into(),
                    kind: "r".into(),
                    comment: None,
                    row_estimate: serde_json::json!(0),
                    size_bytes: serde_json::json!(0),
                },
            ],
            columns: vec![
                RawColumnRow {
                    table_oid: 10,
                    name: "id".into(),
                    type_name: "integer".into(),
                    not_null: true,
                    default_value: None,
                    comment: None,
                    ordinal: 1,
                },
                RawColumnRow {
                    table_oid: 20,
                    name: "id".into(),
                    type_name: "integer".into(),
                    not_null: true,
                    default_value: None,
                    comment: None,
                    ordinal: 1,
                },
                RawColumnRow {
                    table_oid: 20,
                    name: "org_id".into(),
                    type_name: "integer".into(),
                    not_null: true,
                    default_value: None,
                    comment: None,
                    ordinal: 2,
                },
                RawColumnRow {
                    table_oid: 30,
                    name: "id".into(),
                    type_name: "integer".into(),
                    not_null: true,
                    default_value: None,
                    comment: None,
                    ordinal: 1,
                },
                RawColumnRow {
                    table_oid: 30,
                    name: "user_id".into(),
                    type_name: "integer".into(),
                    not_null: false,
                    default_value: None,
                    comment: None,
                    ordinal: 2,
                },
            ],
            constraints: vec![
                RawConstraintRow {
                    table_oid: 10,
                    name: "orgs_pkey".into(),
                    type_name: "p".into(),
                    definition: "PRIMARY KEY (id)".into(),
                    ref_table_oid: None,
                    key_positions: Some(vec![1]),
                    ref_key_positions: None,
                },
                RawConstraintRow {
                    table_oid: 20,
                    name: "users_pkey".into(),
                    type_name: "p".into(),
                    definition: "PRIMARY KEY (id)".into(),
                    ref_table_oid: None,
                    key_positions: Some(vec![1]),
                    ref_key_positions: None,
                },
                RawConstraintRow {
                    table_oid: 20,
                    name: "users_org_id_fkey".into(),
                    type_name: "f".into(),
                    definition: "FOREIGN KEY (org_id) REFERENCES orgs(id)".into(),
                    ref_table_oid: Some(10),
                    key_positions: Some(vec![2]),
                    ref_key_positions: Some(vec![1]),
                },
                RawConstraintRow {
                    table_oid: 30,
                    name: "orders_pkey".into(),
                    type_name: "p".into(),
                    definition: "PRIMARY KEY (id)".into(),
                    ref_table_oid: None,
                    key_positions: Some(vec![1]),
                    ref_key_positions: None,
                },
            ],
            indexes: vec![],
            views: vec![],
            functions: vec![],
            enums: vec![],
            domains: vec![],
        }
    }

    #[tokio::test]
    async fn build_index_mock_writes_artifacts() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let db = users_orgs_fixture();
        let req = BuildRequest {
            connection_id: "conn-1".into(),
            database: "appdb".into(),
            scope: IndexScope {
                included_schemas: vec!["public".into()],
                excluded_objects: vec![],
                pii_excluded_columns: vec![],
            },
            depth: BuildDepth::Structure,
            build_mode: BuildMode::Guided,
            environment: "development".into(),
            embeddings: false,
        };

        let mut events = Vec::new();
        let mut on_progress = |e: BuildProgress| events.push(e);

        let manifest = build_index(&store, &db, &req, Some(&mut on_progress), None, None)
            .await
            .expect("build");

        assert_eq!(manifest.schema_fingerprint, "2|100|50|1|11");
        assert_eq!(manifest.counts.tables, 3);
        assert!(!manifest.shards.is_empty());
        assert!(events.iter().any(|e| matches!(e, BuildProgress::Started)));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, BuildProgress::Finished { .. }))
        );

        let base = store.base_dir("conn-1", "appdb");
        assert!(base.join("manifest.json").is_file());
        assert!(base.join(TOKENS_FILE).is_file());
        assert!(base.join(JOIN_GRAPH_FILE).is_file());
        assert!(base.join(VALUES_FILE).is_file());
        assert!(!base.join(".lock").exists(), "lock released");

        let tokens = store
            .read_tokens(&base, &manifest)
            .unwrap()
            .expect("tokens");
        assert!(tokens.postings.contains_key("user") || tokens.df.contains_key("user"));

        let graph = store
            .read_join_graph(&base, &manifest)
            .unwrap()
            .expect("graph");
        assert!(
            graph
                .edges
                .iter()
                .any(|e| e.from == "public.users" && e.to == "public.orgs" && e.inferred.is_none()),
            "declared FK edge present"
        );
        assert!(
            graph.edges.iter().any(|e| {
                e.from == "public.orders" && e.to == "public.users" && e.inferred == Some(true)
            }),
            "inferred user_id → users edge"
        );

        // Shard object kinds round-trip.
        let shard = &manifest.shards[0];
        let objs = store
            .read_shard_entries(&base, &shard.file)
            .unwrap()
            .expect("shard");
        assert_eq!(objs["public.users"].kind, DbObjectKind::Table);
        assert!(!objs["public.users"].object_hash.is_empty());
    }

    #[tokio::test]
    async fn build_lock_blocks_concurrent() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let base = store.base_dir("c", "d");
        let _lock = store.acquire_lock(&base).unwrap();

        let db = users_orgs_fixture();
        let req = BuildRequest {
            connection_id: "c".into(),
            database: "d".into(),
            scope: IndexScope {
                included_schemas: vec!["public".into()],
                excluded_objects: vec![],
                pii_excluded_columns: vec![],
            },
            depth: BuildDepth::Structure,
            build_mode: BuildMode::Auto,
            environment: "dev".into(),
            embeddings: false,
        };
        let err = build_index(&store, &db, &req, None, None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, IndexError::Building(_)));
    }

    #[test]
    fn naming_match_plurals() {
        assert!(is_naming_match("user", "users"));
        assert!(is_naming_match("org", "orgs"));
        assert!(is_naming_match("category", "categories"));
        assert!(!is_naming_match("foo", "bar"));
    }

    #[test]
    fn fingerprint_format() {
        assert_eq!(
            format_schema_fingerprint("1", "2", "3", "4", "5"),
            "1|2|3|4|5"
        );
    }
}
