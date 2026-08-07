// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Offline-built schema index (dbindex port).
//!
//! On-disk format must stay byte-compatible with the TS extension index
//! (JSON shards + flat f32 `.bin` + manifest).
//!
//! Reference: `nexql-pro/pro/src/features/dbindex/`

pub mod builder;
pub mod catalog;
pub mod embed;
pub mod error;
pub mod joins;
pub mod lexical;
pub mod migrate;
pub mod model;
pub mod object_hash;
pub mod query;
pub mod rrf;
pub mod store;

pub use builder::{
    BuildProgress, BuildRequest, CatalogDb, MAX_OBJECTS_PER_SHARD, MAX_SHARD_BYTES, PgCatalogDb,
    build_index, format_schema_fingerprint,
};
pub use catalog::{
    COLUMNS_QUERY, CONSTRAINTS_QUERY, DOMAINS_QUERY, ENUMS_QUERY, FUNCTIONS_QUERY, INDEXES_QUERY,
    NON_SYSTEM_SCHEMAS_QUERY, RELATIONS_QUERY, RawColumnRow, RawConstraintRow, RawDomainRow,
    RawEnumRow, RawFunctionRow, RawIndexRow, RawRelationRow, RawViewRow, SCHEMA_FINGERPRINT_QUERY,
    VIEW_DEFINITIONS_QUERY, map_relkind_to_db_object_kind,
};
#[cfg(feature = "embeddings")]
pub use embed::MiniLmEmbedder;
pub use embed::{
    Embedder, HF_MODEL_ID, LOCAL_MODEL_ID, MODEL_DIM, build_object_doc, embeddings_env_local,
    is_embeddable_kind,
};
pub use error::IndexError;
pub use joins::{
    MAX_JOIN_HOPS, PathStep, find_shortest_join_path, get_join_path, unreachable_join_path_message,
};
pub use lexical::{
    TableCounts, abbreviations, builtin_synonyms, candidate_refs_from_postings,
    extract_synonyms_from_comment, score_object, stem_word, tokenize,
};
pub use migrate::{CURRENT_FORMAT_VERSION, migrate_manifest};
pub use model::{
    BuildDepth, BuildMode, CheckEntry, ColumnEntry, ColumnOverride, ColumnProfile, DbObjectKind,
    EmbeddingMetaEntry, ForeignKeyEntry, IndexCounts, IndexDerived, IndexEntry, IndexManifest,
    IndexOverrides, IndexScope, IndexStats, JoinEdge, JoinGraph, ObjectEntry, ObjectOverride,
    ObjectShard, TokenIndex, ValueHit, ValueIndex,
};
pub use object_hash::{compute_definition_hash, compute_object_hash};
pub use query::{
    IndexQueryService, QueryPolicyFilter, RankedHit, SampleValuesResult, SearchOptions,
    missing_object_message, no_samples_message, search_schema_lexical,
};
pub use rrf::{RRF_K, RRF_MISSING_RANK, cosine_similarity, fuse_rrf};
pub use store::{
    DBINDEX_DIR, EMBEDDINGS_BIN, EMBEDDINGS_META, IndexStore, JOIN_GRAPH_FILE, LOCK_FILE,
    MANIFEST_FILE, OVERRIDES_FILE, STALE_LOCK, TOKENS_FILE, VALUES_FILE, deserialize_embedding,
    safe_segment, serialize_embeddings,
};
