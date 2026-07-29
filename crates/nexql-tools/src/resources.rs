//! MCP resources backed by the on-disk schema index (dbindex).
//!
//! Port of `pro/src/mcp/McpResourceProvider.ts`. Disk-only — no DB connections.
//!
//! URIs:
//! - `nexql://{profile}/{database}/manifest`
//! - `nexql://{profile}/{database}/joingraph`
//! - `nexql://{profile}/{database}/object/{schema}/{name}`

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use nexql_index::{IndexOverrides, IndexStore, JoinEdge, JoinGraph, ObjectEntry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Matches TS `PAGE_SIZE`.
pub const PAGE_SIZE: usize = 200;

/// JSON-RPC / MCP error codes used by resources.
pub const ERR_INVALID_PARAMS: i32 = -32602;
pub const ERR_RESOURCE_NOT_FOUND: i32 = -32002;

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("{0}")]
    InvalidParams(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Internal(String),
}

impl ResourceError {
    pub fn code(&self) -> i32 {
        match self {
            Self::InvalidParams(_) => ERR_INVALID_PARAMS,
            Self::NotFound(_) => ERR_RESOURCE_NOT_FOUND,
            Self::Internal(_) => -32603,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceListResult {
    pub resources: Vec<McpResource>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContents {
    pub uri: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceReadResult {
    pub contents: Vec<ResourceContents>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplate {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorState {
    /// Index into `list_indexed_databases()` ordering.
    db: usize,
    /// Offset into the flattened resource list of that database.
    offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceKind {
    Manifest,
    JoinGraph,
    Object { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUri {
    pub connection_id: String,
    pub database: String,
    pub kind: ResourceKind,
}

/// Disk-backed MCP resource provider.
pub struct ResourceProvider {
    store: IndexStore,
}

impl ResourceProvider {
    pub fn new(store: IndexStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &IndexStore {
        &self.store
    }

    pub fn list(&self, cursor: Option<&str>) -> Result<ResourceListResult, ResourceError> {
        let state = decode_cursor(cursor)?;
        let databases = self
            .store
            .list_indexed_databases()
            .map_err(|e| ResourceError::Internal(e.to_string()))?;

        let mut resources = Vec::new();
        let mut db_index = state.0;
        let mut offset = state.1;

        while db_index < databases.len() {
            let (connection_id, database) = &databases[db_index];
            let all = self.list_for_database(connection_id, database)?;
            let take = PAGE_SIZE.saturating_sub(resources.len());
            let end = (offset + take).min(all.len());
            let page = &all[offset.min(all.len())..end];
            resources.extend_from_slice(page);

            if end < all.len() {
                return Ok(ResourceListResult {
                    resources,
                    next_cursor: Some(encode_cursor(&CursorState {
                        db: db_index,
                        offset: end,
                    })),
                });
            }
            if resources.len() >= PAGE_SIZE && db_index + 1 < databases.len() {
                return Ok(ResourceListResult {
                    resources,
                    next_cursor: Some(encode_cursor(&CursorState {
                        db: db_index + 1,
                        offset: 0,
                    })),
                });
            }
            db_index += 1;
            offset = 0;
        }

        Ok(ResourceListResult {
            resources,
            next_cursor: None,
        })
    }

    pub fn read(&self, uri: &str) -> Result<ResourceReadResult, ResourceError> {
        let parsed = parse_uri(uri)?;
        let base = self.store.base_dir(&parsed.connection_id, &parsed.database);
        let manifest = self
            .store
            .read_manifest(&base)
            .map_err(|e| ResourceError::Internal(e.to_string()))?
            .ok_or_else(|| {
                ResourceError::NotFound(format!(
                    "Resource not found: no index for {}/{}",
                    parsed.connection_id, parsed.database
                ))
            })?;

        let payload: Value = match &parsed.kind {
            ResourceKind::Manifest => serde_json::to_value(&manifest)
                .map_err(|e| ResourceError::Internal(e.to_string()))?,
            ResourceKind::JoinGraph => {
                let graph = self
                    .store
                    .read_join_graph(&base, &manifest)
                    .map_err(|e| ResourceError::Internal(e.to_string()))?
                    .ok_or_else(|| {
                        ResourceError::NotFound(format!(
                            "Resource not found: join graph missing for {}",
                            parsed.database
                        ))
                    })?;
                let overrides = self
                    .store
                    .read_overrides(&base)
                    .map_err(|e| ResourceError::Internal(e.to_string()))?;
                let merged = merge_join_graph(graph, overrides.as_ref());
                serde_json::to_value(&merged).map_err(|e| ResourceError::Internal(e.to_string()))?
            }
            ResourceKind::Object { schema, name } => {
                let entry = self
                    .store
                    .get_object_entry(&base, &manifest, schema, name)
                    .map_err(|e| ResourceError::Internal(e.to_string()))?;
                match entry {
                    Some(e) if e.excluded != Some(true) => serde_json::to_value(&e)
                        .map_err(|e| ResourceError::Internal(e.to_string()))?,
                    _ => {
                        return Err(ResourceError::NotFound(format!(
                            "Resource not found: {uri}"
                        )));
                    }
                }
            }
        };

        let text = serde_json::to_string_pretty(&payload)
            .map_err(|e| ResourceError::Internal(e.to_string()))?;
        Ok(ResourceReadResult {
            contents: vec![ResourceContents {
                uri: uri.to_owned(),
                mime_type: "application/json".into(),
                text,
            }],
        })
    }

    pub fn list_templates(&self) -> Vec<ResourceTemplate> {
        vec![ResourceTemplate {
            uri_template: "nexql://{connectionId}/{database}/object/{schema}/{name}".into(),
            name: "Database object".into(),
            description:
                "Structural card for an indexed table, view, materialized view, or function \
(columns, keys, indexes, definition). Use the search_schema tool to discover refs."
                    .into(),
            mime_type: "application/json".into(),
        }]
    }

    fn list_for_database(
        &self,
        connection_id: &str,
        database: &str,
    ) -> Result<Vec<McpResource>, ResourceError> {
        let base = self.store.base_dir(connection_id, database);
        let Some(manifest) = self
            .store
            .read_manifest(&base)
            .map_err(|e| ResourceError::Internal(e.to_string()))?
        else {
            return Ok(Vec::new());
        };
        let overrides = self
            .store
            .read_overrides(&base)
            .map_err(|e| ResourceError::Internal(e.to_string()))?;

        let prefix = format!(
            "nexql://{}/{}",
            encode_uri_component(connection_id),
            encode_uri_component(database)
        );

        let mut resources = vec![
            McpResource {
                uri: format!("{prefix}/manifest"),
                name: format!("{database} index manifest"),
                description: Some(format!(
                    "Index metadata for {database} (schemas, counts, fingerprint, build time)."
                )),
                mime_type: "application/json".into(),
            },
            McpResource {
                uri: format!("{prefix}/joingraph"),
                name: format!("{database} join graph"),
                description: Some(format!(
                    "Declared and inferred foreign-key relationships between tables in {database}."
                )),
                mime_type: "application/json".into(),
            },
        ];

        let mut objects: Vec<(String, ObjectEntry)> = Vec::new();
        for shard in &manifest.shards {
            let Some(entries) = self
                .store
                .read_shard_entries(&base, &shard.file)
                .map_err(|e| ResourceError::Internal(e.to_string()))?
            else {
                continue;
            };
            for (ref_, entry) in entries {
                if entry.excluded == Some(true) || object_excluded(overrides.as_ref(), &ref_) {
                    continue;
                }
                objects.push((ref_, entry));
            }
        }
        // Stable order for cursor pagination (HashMap iteration is not).
        objects.sort_by(|a, b| a.0.cmp(&b.0));

        for (ref_, entry) in objects {
            let (schema, name) = split_ref(&ref_);
            let description = match &entry.comment {
                Some(c) if !c.is_empty() => format!("{} — {c}", entry.kind.as_str()),
                _ => entry.kind.as_str().to_owned(),
            };
            resources.push(McpResource {
                uri: format!(
                    "{prefix}/object/{}/{}",
                    encode_uri_component(&schema),
                    encode_uri_component(&name)
                ),
                name: ref_,
                description: Some(description),
                mime_type: "application/json".into(),
            });
        }

        Ok(resources)
    }
}

fn object_excluded(overrides: Option<&IndexOverrides>, ref_: &str) -> bool {
    overrides
        .and_then(|o| o.objects.as_ref())
        .and_then(|objs| objs.get(ref_))
        .and_then(|obj| obj.excluded)
        == Some(true)
}

fn split_ref(ref_: &str) -> (String, String) {
    match ref_.split_once('.') {
        Some((schema, name)) => (schema.to_owned(), name.to_owned()),
        None => ("public".to_owned(), ref_.to_owned()),
    }
}

/// Merge join-graph overrides (matches TS `IndexStore.readJoinGraph`).
fn merge_join_graph(mut graph: JoinGraph, overrides: Option<&IndexOverrides>) -> JoinGraph {
    let Some(joins) = overrides.and_then(|o| o.joins.as_ref()) else {
        return graph;
    };
    if joins.is_empty() {
        return graph;
    }

    let mut override_map: HashMap<String, JoinEdge> = HashMap::new();
    for edge in joins {
        let key = format!("{}->{}:{}", edge.from, edge.to, edge.via);
        override_map.insert(key, edge.clone());
    }

    let mut merged = Vec::new();
    for base in graph.edges.drain(..) {
        let key = format!("{}->{}:{}", base.from, base.to, base.via);
        if let Some(over) = override_map.remove(&key) {
            if over.disabled != Some(true) {
                merged.push(over);
            }
        } else {
            merged.push(base);
        }
    }
    for edge in override_map.into_values() {
        if edge.disabled != Some(true) {
            merged.push(edge);
        }
    }
    graph.edges = merged;
    graph
}

/// Encode pagination cursor `{db, offset}` as base64 JSON.
pub fn encode_cursor_state(db: usize, offset: usize) -> String {
    encode_cursor_inner(&CursorState { db, offset })
}

/// Decode pagination cursor; missing/empty → `(0, 0)`.
pub fn decode_cursor(cursor: Option<&str>) -> Result<(usize, usize), ResourceError> {
    let state = decode_cursor_inner(cursor)?;
    Ok((state.db, state.offset))
}

fn encode_cursor_inner(state: &CursorState) -> String {
    let json = serde_json::to_string(state).expect("cursor serialize");
    B64.encode(json.as_bytes())
}

fn decode_cursor_inner(cursor: Option<&str>) -> Result<CursorState, ResourceError> {
    let Some(cursor) = cursor.filter(|c| !c.is_empty()) else {
        return Ok(CursorState { db: 0, offset: 0 });
    };
    let bytes = B64
        .decode(cursor.as_bytes())
        .map_err(|_| ResourceError::InvalidParams("Invalid cursor".into()))?;
    let state: CursorState = serde_json::from_slice(&bytes)
        .map_err(|_| ResourceError::InvalidParams("Invalid cursor".into()))?;
    Ok(state)
}

fn encode_cursor(state: &CursorState) -> String {
    encode_cursor_inner(state)
}

/// Parse a `nexql://…` resource URI.
pub fn parse_uri(uri: &str) -> Result<ParsedUri, ResourceError> {
    let rest = uri
        .strip_prefix("nexql://")
        .ok_or_else(|| ResourceError::NotFound(format!("Resource not found: {uri}")))?;
    let mut parts = rest.splitn(3, '/');
    let connection_id = parts.next().filter(|s| !s.is_empty());
    let database = parts.next().filter(|s| !s.is_empty());
    let tail = parts.next().filter(|s| !s.is_empty());
    let (Some(conn_raw), Some(db_raw), Some(tail)) = (connection_id, database, tail) else {
        return Err(ResourceError::NotFound(format!(
            "Resource not found: {uri}"
        )));
    };

    let connection_id = decode_uri_component(conn_raw)
        .ok_or_else(|| ResourceError::NotFound(format!("Resource not found: {uri}")))?;
    let database = decode_uri_component(db_raw)
        .ok_or_else(|| ResourceError::NotFound(format!("Resource not found: {uri}")))?;

    if tail == "manifest" {
        return Ok(ParsedUri {
            connection_id,
            database,
            kind: ResourceKind::Manifest,
        });
    }
    if tail == "joingraph" {
        return Ok(ParsedUri {
            connection_id,
            database,
            kind: ResourceKind::JoinGraph,
        });
    }

    let mut obj_parts = tail.splitn(3, '/');
    let kind = obj_parts.next();
    let schema_raw = obj_parts.next();
    let name_raw = obj_parts.next();
    if kind != Some("object") || schema_raw.is_none() || name_raw.is_none() {
        return Err(ResourceError::NotFound(format!(
            "Resource not found: {uri}"
        )));
    }
    let schema = decode_uri_component(schema_raw.unwrap())
        .ok_or_else(|| ResourceError::NotFound(format!("Resource not found: {uri}")))?;
    let name = decode_uri_component(name_raw.unwrap())
        .ok_or_else(|| ResourceError::NotFound(format!("Resource not found: {uri}")))?;
    if schema.is_empty() || name.is_empty() || name.contains('/') {
        return Err(ResourceError::NotFound(format!(
            "Resource not found: {uri}"
        )));
    }

    Ok(ParsedUri {
        connection_id,
        database,
        kind: ResourceKind::Object { schema, name },
    })
}

/// `encodeURIComponent`-compatible encoding for URI path segments.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn decode_uri_component(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hi = from_hex(bytes[i + 1])?;
                let lo = from_hex(bytes[i + 2])?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexql_index::{
        BuildDepth, BuildMode, ColumnEntry, DbObjectKind, IndexCounts, IndexDerived, IndexManifest,
        IndexScope, IndexStats, ObjectShard,
    };
    use tempfile::TempDir;

    #[test]
    fn cursor_round_trip() {
        let encoded = encode_cursor_state(2, 150);
        let (db, offset) = decode_cursor(Some(&encoded)).unwrap();
        assert_eq!((db, offset), (2, 150));
    }

    #[test]
    fn cursor_default_when_absent() {
        assert_eq!(decode_cursor(None).unwrap(), (0, 0));
        assert_eq!(decode_cursor(Some("")).unwrap(), (0, 0));
    }

    #[test]
    fn invalid_cursor_is_invalid_params() {
        let err = decode_cursor(Some("!!!not-base64-json!!!")).unwrap_err();
        assert_eq!(err.code(), ERR_INVALID_PARAMS);
        assert!(err.to_string().contains("Invalid cursor"));
    }

    #[test]
    fn parse_manifest_joingraph_object_uris() {
        let m = parse_uri("nexql://prod/appdb/manifest").unwrap();
        assert_eq!(m.connection_id, "prod");
        assert_eq!(m.database, "appdb");
        assert_eq!(m.kind, ResourceKind::Manifest);

        let j = parse_uri("nexql://prod/appdb/joingraph").unwrap();
        assert_eq!(j.kind, ResourceKind::JoinGraph);

        let o = parse_uri("nexql://prod/appdb/object/public/users").unwrap();
        assert_eq!(
            o.kind,
            ResourceKind::Object {
                schema: "public".into(),
                name: "users".into()
            }
        );
    }

    #[test]
    fn parse_uri_decodes_components() {
        let o = parse_uri("nexql://my%20conn/db%2F1/object/public/my%20table").unwrap();
        assert_eq!(o.connection_id, "my conn");
        assert_eq!(o.database, "db/1");
        assert_eq!(
            o.kind,
            ResourceKind::Object {
                schema: "public".into(),
                name: "my table".into()
            }
        );
    }

    #[test]
    fn unknown_uri_is_not_found() {
        let err = parse_uri("nexql://a/b/c").unwrap_err();
        assert_eq!(err.code(), ERR_RESOURCE_NOT_FOUND);
        let err = parse_uri("http://x").unwrap_err();
        assert_eq!(err.code(), ERR_RESOURCE_NOT_FOUND);
    }

    #[test]
    fn empty_index_lists_nothing() {
        let tmp = TempDir::new().unwrap();
        let provider = ResourceProvider::new(IndexStore::new(tmp.path()));
        let result = provider.list(None).unwrap();
        assert!(result.resources.is_empty());
        assert!(result.next_cursor.is_none());
    }

    #[test]
    fn list_and_read_manifest_from_fixture() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let base = store.base_dir("conn1", "db1");
        let manifest = IndexManifest {
            format_version: 1,
            connection_id: "conn1".into(),
            database: "db1".into(),
            indexed_at: "2026-01-01T00:00:00Z".into(),
            build_mode: BuildMode::Guided,
            build_depth: BuildDepth::Structure,
            schema_fingerprint: "fp".into(),
            pg_version: "16".into(),
            environment: "development".into(),
            scope: IndexScope {
                included_schemas: vec!["public".into()],
                excluded_objects: vec![],
                pii_excluded_columns: vec![],
            },
            counts: IndexCounts {
                tables: 1,
                views: 0,
                functions: 0,
                enums: 0,
            },
            shards: vec![ObjectShard {
                file: "objects_public.json".into(),
                schema: "public".into(),
                objects: 1,
                bytes: 10,
                hash: "h".into(),
            }],
            derived: IndexDerived {
                tokens: "tokens.json".into(),
                join_graph: "joingraph.json".into(),
                values: None,
                embeddings: None,
                embeddings_meta: None,
            },
            stats: IndexStats {
                build_ms: 1,
                queries_run: 1,
                warnings: vec![],
            },
        };
        store.write_manifest(&base, &manifest).unwrap();
        let mut entries = HashMap::new();
        entries.insert(
            "public.users".into(),
            ObjectEntry {
                kind: DbObjectKind::Table,
                oid: 1,
                object_hash: "x".into(),
                comment: Some("users table".into()),
                row_estimate: 10.0,
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
            },
        );
        store
            .write_shard_entries(&base, "objects_public.json", &entries)
            .unwrap();

        let provider = ResourceProvider::new(store);
        let listed = provider.list(None).unwrap();
        assert_eq!(listed.resources.len(), 3);
        assert!(
            listed
                .resources
                .iter()
                .any(|r| r.uri.ends_with("/manifest"))
        );
        assert!(
            listed
                .resources
                .iter()
                .any(|r| r.uri.ends_with("/object/public/users"))
        );

        let read = provider
            .read("nexql://conn1/db1/object/public/users")
            .unwrap();
        assert_eq!(read.contents.len(), 1);
        assert!(read.contents[0].text.contains("users table"));
    }
}
