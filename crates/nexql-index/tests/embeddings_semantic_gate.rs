// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Phase 5 exit gate: "semantic search beats lexical on synonym fixture" —
//! using the real MiniLM embedder (candle), not the `FakeEmbedder` RRF-fusion
//! tests in `query.rs`. Only compiled/run with `--features embeddings`.
//!
//! Downloads MiniLM from the Hugging Face hub on first run (cached after).
//! Skips (not fails) when the model can't be loaded — no network, or
//! `NEXQL_SKIP_MODEL_DOWNLOAD` set — matching `embed.rs`'s existing pattern,
//! so CI degrades gracefully rather than flaking on network access.
#![cfg(feature = "embeddings")]

use std::collections::HashMap;

use nexql_index::{
    BuildDepth, BuildMode, ColumnEntry, DbObjectKind, Embedder, EmbeddingMetaEntry, IndexCounts,
    IndexDerived, IndexManifest, IndexQueryService, IndexScope, IndexStats, IndexStore,
    JOIN_GRAPH_FILE, MiniLmEmbedder, ObjectEntry, ObjectShard, SearchOptions, TOKENS_FILE,
    TokenIndex, VALUES_FILE, build_object_doc, serialize_embeddings,
};
use tempfile::TempDir;

const CONN: &str = "semantic-gate-conn";
const DB: &str = "appdb";

fn col(name: &str) -> ColumnEntry {
    ColumnEntry {
        name: name.into(),
        type_name: "text".into(),
        not_null: false,
        default_value: None,
        comment: None,
        ordinal: 1,
        is_pk: None,
        profile: None,
        pii: None,
    }
}

fn table_entry(oid: u32, comment: &str, cols: Vec<ColumnEntry>) -> ObjectEntry {
    ObjectEntry {
        kind: DbObjectKind::Table,
        oid,
        object_hash: format!("hash{oid}"),
        comment: Some(comment.into()),
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

fn empty_manifest() -> IndexManifest {
    IndexManifest {
        format_version: 1,
        connection_id: CONN.into(),
        database: DB.into(),
        indexed_at: "2026-08-06T00:00:00.000Z".into(),
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
            tables: 3,
            views: 0,
            functions: 0,
            enums: 0,
        },
        shards: vec![ObjectShard {
            file: "objects-public-0.json".into(),
            schema: "public".into(),
            objects: 3,
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

/// A synonym-style query token ("client") that never appears in the lexical
/// index at all — only real customer/order/payment vocabulary does. Lexical
/// search for it must miss entirely; only semantic search can bridge it.
fn synonym_free_tokens() -> TokenIndex {
    TokenIndex {
        version: 1,
        df: HashMap::from([
            ("order".into(), 1.0),
            ("product".into(), 1.0),
            ("payment".into(), 1.0),
        ]),
        postings: HashMap::from([
            ("order".into(), vec![("public.orders".into(), 5.0)]),
            ("product".into(), vec![("public.orders".into(), 3.0)]),
            ("payment".into(), vec![("public.payments".into(), 5.0)]),
        ]),
        synonyms: HashMap::new(),
    }
}

#[test]
fn real_minilm_semantic_search_beats_lexical_on_synonym_query() {
    if std::env::var_os("NEXQL_SKIP_MODEL_DOWNLOAD").is_some() {
        eprintln!("skip: NEXQL_SKIP_MODEL_DOWNLOAD set");
        return;
    }
    let embedder = match MiniLmEmbedder::load() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("skip: MiniLM load failed (no network?): {e}");
            return;
        }
    };

    let tmp = TempDir::new().unwrap();
    let store = IndexStore::new(tmp.path());
    let base = store.base_dir(CONN, DB);

    // customers has no lexical postings at all — "client" must reach it (if
    // it does) purely through semantic similarity.
    let customers = table_entry(
        1,
        "People who purchase from the store",
        vec![col("email"), col("name")],
    );
    let orders = table_entry(2, "Orders placed by customers", vec![col("product")]);
    let payments = table_entry(3, "Payments received for orders", vec![col("amount")]);

    let mut shard = HashMap::new();
    shard.insert("public.customers".to_string(), customers.clone());
    shard.insert("public.orders".to_string(), orders.clone());
    shard.insert("public.payments".to_string(), payments.clone());
    store
        .write_shard_entries(&base, "objects-public-0.json", &shard)
        .unwrap();
    store.write_tokens(&base, &synonym_free_tokens()).unwrap();

    let docs = [
        ("public.customers", &customers),
        ("public.orders", &orders),
        ("public.payments", &payments),
    ];
    let mut vectors = Vec::with_capacity(docs.len());
    let mut meta = Vec::with_capacity(docs.len());
    for (ref_, entry) in docs {
        let doc = build_object_doc(ref_, entry);
        let v = embedder.embed(&doc).expect("embed object doc");
        vectors.push(v);
        meta.push(EmbeddingMetaEntry {
            ref_: ref_.to_string(),
            object_hash: entry.object_hash.clone(),
            model: embedder.model_id().to_string(),
            dim: embedder.dim() as u32,
        });
    }
    let bin = serialize_embeddings(&vectors, embedder.dim());
    store.write_embeddings(&base, &meta, &bin).unwrap();

    let mut manifest = empty_manifest();
    manifest.derived.embeddings = Some(nexql_index::EMBEDDINGS_BIN.into());
    manifest.derived.embeddings_meta = Some(nexql_index::EMBEDDINGS_META.into());
    store.write_manifest(&base, &manifest).unwrap();

    let svc = IndexQueryService::new(&store, CONN, DB);

    let lexical = svc
        .search_schema(
            "client",
            10,
            None,
            SearchOptions {
                use_semantic: false,
                embedder: None,
            },
        )
        .unwrap();
    assert!(
        lexical.is_empty(),
        "lexical-only search for a term with zero postings must return no hits, got {lexical:?}"
    );

    let semantic = svc
        .search_schema(
            "client",
            10,
            None,
            SearchOptions {
                use_semantic: true,
                embedder: Some(&embedder),
            },
        )
        .unwrap();
    assert!(
        !semantic.is_empty(),
        "semantic search should surface hits for a synonym query lexical search misses"
    );
    assert_eq!(
        semantic[0].ref_, "public.customers",
        "real MiniLM embeddings should rank 'customers' top for a 'client' query; got {semantic:?}"
    );
}
