// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Wire-compatible index model types.
//!
//! Field names must match the on-disk JSON produced by
//! `pro/src/features/dbindex/types.ts` (camelCase keys).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Build trigger mode — matches TS `BuildMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildMode {
    Auto,
    Guided,
}

/// How much catalog detail to capture — matches TS `BuildDepth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildDepth {
    Structure,
    Stats,
    Profiles,
}

/// Object kind discriminator — matches TS `DbObjectKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DbObjectKind {
    Table,
    View,
    Matview,
    Function,
    Enum,
    Domain,
    Sequence,
}

/// Index build scope — matches TS `IndexScope`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexScope {
    pub included_schemas: Vec<String>,
    pub excluded_objects: Vec<String>,
    /// Formatted as `schema.table.column`.
    pub pii_excluded_columns: Vec<String>,
}

/// Per-schema object shard pointer — matches TS `ObjectShard`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectShard {
    pub file: String,
    pub schema: String,
    pub objects: u64,
    pub bytes: u64,
    pub hash: String,
}

/// Object-count breakdown on the manifest — matches `IndexManifest.counts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexCounts {
    pub tables: u64,
    pub views: u64,
    pub functions: u64,
    pub enums: u64,
}

/// Paths to derived artifacts — matches `IndexManifest.derived`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexDerived {
    pub tokens: String,
    pub join_graph: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embeddings: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embeddings_meta: Option<String>,
}

/// Build telemetry — matches `IndexManifest.stats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub build_ms: u64,
    pub queries_run: u64,
    pub warnings: Vec<String>,
}

/// Root on-disk manifest (`manifest.json`) — matches TS `IndexManifest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexManifest {
    pub format_version: u32,
    pub connection_id: String,
    pub database: String,
    pub indexed_at: String,
    pub build_mode: BuildMode,
    pub build_depth: BuildDepth,
    pub schema_fingerprint: String,
    pub pg_version: String,
    pub environment: String,
    pub scope: IndexScope,
    pub counts: IndexCounts,
    pub shards: Vec<ObjectShard>,
    pub derived: IndexDerived,
    pub stats: IndexStats,
}

/// Column stats from `pg_stats` / value profiler — matches TS `ColumnProfile`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnProfile {
    pub n_distinct: f64,
    pub null_frac: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_values: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Option<String>>,
}

/// Column metadata on an object entry — matches TS `ColumnEntry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub not_null: bool,
    #[serde(rename = "default")]
    pub default_value: Option<String>,
    pub comment: Option<String>,
    pub ordinal: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_pk: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ColumnProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pii: Option<bool>,
}

/// Foreign-key edge on a table — matches TS `ForeignKeyEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyEntry {
    pub columns: Vec<String>,
    /// `schema.table`
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_delete: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred: Option<bool>,
}

/// Index definition on a table — matches TS `IndexEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial: Option<Option<String>>,
}

/// Check constraint — matches TS `CheckEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckEntry {
    pub name: String,
    pub expr: String,
}

/// One catalog object in a shard file — matches TS `ObjectEntry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectEntry {
    pub kind: DbObjectKind,
    pub oid: u32,
    pub object_hash: String,
    pub comment: Option<String>,
    pub row_estimate: f64,
    pub size_bytes: u64,
    pub columns: Vec<ColumnEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_key: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreign_keys: Option<Vec<ForeignKeyEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexes: Option<Vec<IndexEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<Vec<CheckEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excluded: Option<bool>,

    // views / matviews
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,

    // functions
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volatility: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Option<String>>,

    // enums / domains
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraint: Option<String>,
}

/// TF-IDF token postings file — matches TS `TokenIndex`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenIndex {
    pub version: u32,
    /// Document frequency of each token (for IDF).
    pub df: HashMap<String, f64>,
    /// Token → `[objectRef, weight]` postings.
    pub postings: HashMap<String, Vec<(String, f64)>>,
    pub synonyms: HashMap<String, Vec<String>>,
}

/// Single join-graph edge — matches TS `JoinEdge`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinEdge {
    pub from: String,
    pub to: String,
    pub via: String,
    pub cols: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

/// Join graph artifact — matches TS `JoinGraph`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinGraph {
    pub edges: Vec<JoinEdge>,
}

/// Embedding metadata row — matches TS `EmbeddingMetaEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingMetaEntry {
    /// Object ref (`schema.table`); wire key is `ref`.
    #[serde(rename = "ref")]
    pub ref_: String,
    pub object_hash: String,
    pub model: String,
    pub dim: u32,
}

/// User overrides overlay — matches TS `IndexOverrides`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IndexOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joins: Option<Vec<JoinEdge>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synonyms: Option<HashMap<String, Vec<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<HashMap<String, ObjectOverride>>,
}

/// Per-object override — matches nested `IndexOverrides.objects` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ObjectOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excluded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<HashMap<String, ColumnOverride>>,
}

/// Per-column override — matches nested column override in `IndexOverrides`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ColumnOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pii: Option<bool>,
}

/// One hit in the value inverted index (`values.json`) — matches TS `readValues` entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueHit {
    /// Object ref (`schema.table`); wire key is `ref`.
    #[serde(rename = "ref")]
    pub ref_: String,
    pub col: String,
}

/// Token → value postings — matches TS `IndexStore.readValues` return type.
pub type ValueIndex = HashMap<String, Vec<ValueHit>>;

impl DbObjectKind {
    /// Lowercase wire / tool kind string (`"table"`, `"view"`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::View => "view",
            Self::Matview => "matview",
            Self::Function => "function",
            Self::Enum => "enum",
            Self::Domain => "domain",
            Self::Sequence => "sequence",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> IndexManifest {
        IndexManifest {
            format_version: 1,
            connection_id: "conn-1".into(),
            database: "appdb".into(),
            indexed_at: "2026-07-22T12:00:00.000Z".into(),
            build_mode: BuildMode::Auto,
            build_depth: BuildDepth::Structure,
            schema_fingerprint: "10|100|1000|2|50".into(),
            pg_version: "16.2".into(),
            environment: "development".into(),
            scope: IndexScope {
                included_schemas: vec!["public".into()],
                excluded_objects: vec![],
                pii_excluded_columns: vec!["public.users.ssn".into()],
            },
            counts: IndexCounts {
                tables: 1,
                views: 0,
                functions: 0,
                enums: 0,
            },
            shards: vec![ObjectShard {
                file: "objects-public-0.json".into(),
                schema: "public".into(),
                objects: 1,
                bytes: 256,
                hash: "abc123".into(),
            }],
            derived: IndexDerived {
                tokens: "tokens.json".into(),
                join_graph: "joinGraph.json".into(),
                values: None,
                embeddings: None,
                embeddings_meta: None,
            },
            stats: IndexStats {
                build_ms: 42,
                queries_run: 9,
                warnings: vec![],
            },
        }
    }

    fn sample_object_entry() -> ObjectEntry {
        ObjectEntry {
            kind: DbObjectKind::Table,
            oid: 16384,
            object_hash: "deadbeef".into(),
            comment: Some("users table".into()),
            row_estimate: 100.0,
            size_bytes: 8192,
            columns: vec![ColumnEntry {
                name: "id".into(),
                type_name: "integer".into(),
                not_null: true,
                default_value: None,
                comment: None,
                ordinal: 1,
                is_pk: Some(true),
                profile: None,
                pii: None,
            }],
            primary_key: Some(vec!["id".into()]),
            foreign_keys: Some(vec![ForeignKeyEntry {
                columns: vec!["org_id".into()],
                ref_table: "public.orgs".into(),
                ref_columns: vec!["id".into()],
                name: "users_org_id_fkey".into(),
                on_delete: Some("CASCADE".into()),
                inferred: None,
            }]),
            indexes: Some(vec![IndexEntry {
                name: "users_pkey".into(),
                columns: vec!["id".into()],
                unique: true,
                method: "btree".into(),
                partial: None,
            }]),
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

    #[test]
    fn index_manifest_serde_round_trip() {
        let original = sample_manifest();
        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: IndexManifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, original);

        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["formatVersion"], 1);
        assert_eq!(value["connectionId"], "conn-1");
        assert_eq!(value["buildMode"], "auto");
        assert_eq!(value["buildDepth"], "structure");
        assert_eq!(value["schemaFingerprint"], "10|100|1000|2|50");
        assert_eq!(value["scope"]["includedSchemas"][0], "public");
        assert_eq!(value["scope"]["piiExcludedColumns"][0], "public.users.ssn");
        assert_eq!(value["counts"]["tables"], 1);
        assert_eq!(value["shards"][0]["file"], "objects-public-0.json");
        assert_eq!(value["derived"]["joinGraph"], "joinGraph.json");
        assert!(value["derived"].get("embeddings").is_none());
        assert_eq!(value["stats"]["buildMs"], 42);
    }

    #[test]
    fn object_entry_serde_round_trip() {
        let original = sample_object_entry();
        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: ObjectEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, original);

        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["kind"], "table");
        assert_eq!(value["objectHash"], "deadbeef");
        assert_eq!(value["rowEstimate"], 100.0);
        assert_eq!(value["sizeBytes"], 8192);
        assert_eq!(value["columns"][0]["type"], "integer");
        assert_eq!(value["columns"][0]["notNull"], true);
        assert!(value["columns"][0]["default"].is_null());
        assert_eq!(value["columns"][0]["isPk"], true);
        assert_eq!(value["primaryKey"][0], "id");
        assert_eq!(value["foreignKeys"][0]["refTable"], "public.orgs");
        assert_eq!(value["foreignKeys"][0]["onDelete"], "CASCADE");
        assert_eq!(value["indexes"][0]["unique"], true);
    }

    #[test]
    fn token_index_postings_wire_as_tuples() {
        let mut postings = HashMap::new();
        postings.insert(
            "user".into(),
            vec![
                ("public.users".into(), 1.5),
                ("public.accounts".into(), 0.5),
            ],
        );
        let idx = TokenIndex {
            version: 1,
            df: HashMap::from([("user".into(), 2.0)]),
            postings,
            synonyms: HashMap::new(),
        };
        let json = serde_json::to_string(&idx).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["postings"]["user"][0],
            serde_json::json!(["public.users", 1.5])
        );
        let parsed: TokenIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, idx);
    }

    #[test]
    fn embedding_meta_ref_key_is_ref() {
        let entry = EmbeddingMetaEntry {
            ref_: "public.users".into(),
            object_hash: "abc".into(),
            model: "minilm".into(),
            dim: 384,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["ref"], "public.users");
        assert_eq!(json["objectHash"], "abc");
        let parsed: EmbeddingMetaEntry = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, entry);
    }
}
