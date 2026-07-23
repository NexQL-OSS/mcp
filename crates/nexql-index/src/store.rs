//! On-disk index store — port of `pro/src/features/dbindex/IndexStore.ts`.
//!
//! Layout under `{root}/dbindex/{safe_segment(connectionId)}/{safe_segment(database)}/`:
//! - `manifest.json`
//! - `objects-{schema}-{n}.json` shards
//! - `tokens.json`, `joingraph.json`
//! - optional `embeddings.bin` (Float32 LE row-major) + `embeddings-meta.json`
//! - `.lock` (stale after 10 minutes)

use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::error::IndexError;
use crate::migrate::migrate_manifest;
use crate::model::{
    EmbeddingMetaEntry, ForeignKeyEntry, IndexManifest, IndexOverrides, JoinGraph, ObjectEntry,
    TokenIndex, ValueIndex,
};

/// Filename constants matching the TS builder/store.
pub const DBINDEX_DIR: &str = "dbindex";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const LOCK_FILE: &str = ".lock";
pub const TOKENS_FILE: &str = "tokens.json";
pub const JOIN_GRAPH_FILE: &str = "joingraph.json";
pub const EMBEDDINGS_BIN: &str = "embeddings.bin";
pub const EMBEDDINGS_META: &str = "embeddings-meta.json";
pub const OVERRIDES_FILE: &str = "overrides.json";
pub const VALUES_FILE: &str = "values.json";

/// Abandoned build locks older than this are overwritten (matches TS).
pub const STALE_LOCK: Duration = Duration::from_secs(10 * 60);

/// Sanitize a path segment — matches TS `safeSegment`.
pub fn safe_segment(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => c,
            _ => '_',
        })
        .collect()
}

/// Serialize embedding vectors as contiguous Float32 LE, row-major.
/// Offset of element `(i, j)` is `(i * dim + j) * 4` — matches TS `serializeEmbeddings`.
pub fn serialize_embeddings(vectors: &[Vec<f32>], dim: usize) -> Vec<u8> {
    let mut buf = vec![0u8; vectors.len() * dim * 4];
    for (i, vec) in vectors.iter().enumerate() {
        for j in 0..dim {
            let v = vec.get(j).copied().unwrap_or(0.0);
            let offset = (i * dim + j) * 4;
            buf[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    buf
}

/// Read one embedding row from a Float32 LE row-major buffer.
pub fn deserialize_embedding(buffer: &[u8], index: usize, dim: usize) -> Result<Vec<f32>, IndexError> {
    let offset = index * dim * 4;
    let end = offset + dim * 4;
    if end > buffer.len() {
        return Err(IndexError::InvalidManifest(format!(
            "embedding row {index} (dim={dim}) exceeds buffer length {}",
            buffer.len()
        )));
    }
    let mut out = Vec::with_capacity(dim);
    for j in 0..dim {
        let start = offset + j * 4;
        let bytes: [u8; 4] = buffer[start..start + 4]
            .try_into()
            .expect("4-byte slice");
        out.push(f32::from_le_bytes(bytes));
    }
    Ok(out)
}

/// Disk-backed schema index store rooted at a global storage directory.
#[derive(Debug, Clone)]
pub struct IndexStore {
    root: PathBuf,
}

impl IndexStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `{root}/dbindex/{safe(connectionId)}/{safe(database)}`
    pub fn base_dir(&self, connection_id: &str, database: &str) -> PathBuf {
        self.root
            .join(DBINDEX_DIR)
            .join(safe_segment(connection_id))
            .join(safe_segment(database))
    }

    /// Enumerate indexes by reading each `manifest.json` (dir names are lossy).
    pub fn list_indexed_databases(&self) -> Result<Vec<(String, String)>, IndexError> {
        let mut results = Vec::new();
        let index_root = self.root.join(DBINDEX_DIR);
        let conn_dirs = match fs::read_dir(&index_root) {
            Ok(d) => d,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(results),
            Err(e) => return Err(e.into()),
        };

        for conn_ent in conn_dirs {
            let conn_ent = conn_ent?;
            if !conn_ent.file_type()?.is_dir() {
                continue;
            }
            let db_dirs = match fs::read_dir(conn_ent.path()) {
                Ok(d) => d,
                Err(_) => continue,
            };
            for db_ent in db_dirs {
                let db_ent = db_ent?;
                if !db_ent.file_type()?.is_dir() {
                    continue;
                }
                if let Some(manifest) = self.read_manifest(&db_ent.path())? {
                    results.push((manifest.connection_id, manifest.database));
                }
            }
        }
        Ok(results)
    }

    /// Acquire a build lock. Returns `false` if a fresh lock exists.
    /// Stale locks (> [`STALE_LOCK`]) are overwritten.
    pub fn acquire_lock(&self, base_dir: &Path) -> Result<bool, IndexError> {
        fs::create_dir_all(base_dir)?;
        let lock_path = base_dir.join(LOCK_FILE);
        match fs::metadata(&lock_path) {
            Ok(meta) => {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                let age = SystemTime::now()
                    .duration_since(mtime)
                    .unwrap_or(Duration::ZERO);
                if age > STALE_LOCK {
                    self.write_lock_file(&lock_path)?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                self.write_lock_file(&lock_path)?;
                Ok(true)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn release_lock(&self, base_dir: &Path) -> Result<(), IndexError> {
        let lock_path = base_dir.join(LOCK_FILE);
        match fs::remove_file(&lock_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn read_manifest(&self, base_dir: &Path) -> Result<Option<IndexManifest>, IndexError> {
        let path = base_dir.join(MANIFEST_FILE);
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        Ok(Some(migrate_manifest(&raw)?))
    }

    pub fn write_manifest(&self, base_dir: &Path, manifest: &IndexManifest) -> Result<(), IndexError> {
        let bytes = serde_json::to_vec(manifest)?;
        self.write_atomic(&base_dir.join(MANIFEST_FILE), &bytes)
    }

    pub fn read_shard_entries(
        &self,
        base_dir: &Path,
        shard_file: &str,
    ) -> Result<Option<HashMap<String, ObjectEntry>>, IndexError> {
        self.read_json_opt(&base_dir.join(shard_file))
    }

    pub fn write_shard_entries(
        &self,
        base_dir: &Path,
        shard_file: &str,
        entries: &HashMap<String, ObjectEntry>,
    ) -> Result<(), IndexError> {
        let bytes = serde_json::to_vec(entries)?;
        self.write_atomic(&base_dir.join(shard_file), &bytes)
    }

    pub fn read_tokens(
        &self,
        base_dir: &Path,
        manifest: &IndexManifest,
    ) -> Result<Option<TokenIndex>, IndexError> {
        self.read_json_opt(&base_dir.join(&manifest.derived.tokens))
    }

    pub fn write_tokens(&self, base_dir: &Path, tokens: &TokenIndex) -> Result<(), IndexError> {
        let bytes = serde_json::to_vec(tokens)?;
        self.write_atomic(&base_dir.join(TOKENS_FILE), &bytes)
    }

    pub fn read_join_graph(
        &self,
        base_dir: &Path,
        manifest: &IndexManifest,
    ) -> Result<Option<JoinGraph>, IndexError> {
        self.read_json_opt(&base_dir.join(&manifest.derived.join_graph))
    }

    pub fn write_join_graph(&self, base_dir: &Path, graph: &JoinGraph) -> Result<(), IndexError> {
        let bytes = serde_json::to_vec(graph)?;
        self.write_atomic(&base_dir.join(JOIN_GRAPH_FILE), &bytes)
    }

    pub fn read_overrides(&self, base_dir: &Path) -> Result<Option<IndexOverrides>, IndexError> {
        self.read_json_opt(&base_dir.join(OVERRIDES_FILE))
    }

    pub fn write_overrides(
        &self,
        base_dir: &Path,
        overrides: &IndexOverrides,
    ) -> Result<(), IndexError> {
        let bytes = serde_json::to_vec_pretty(overrides)?;
        self.write_atomic(&base_dir.join(OVERRIDES_FILE), &bytes)
    }

    /// Read the value inverted index (`values.json`). Returns `None` when the
    /// manifest has no `derived.values` path or the file is missing.
    pub fn read_values(
        &self,
        base_dir: &Path,
        manifest: &IndexManifest,
    ) -> Result<Option<ValueIndex>, IndexError> {
        let Some(name) = &manifest.derived.values else {
            return Ok(None);
        };
        self.read_json_opt(&base_dir.join(name))
    }

    pub fn write_values(&self, base_dir: &Path, values: &ValueIndex) -> Result<(), IndexError> {
        let bytes = serde_json::to_vec(values)?;
        self.write_atomic(&base_dir.join(VALUES_FILE), &bytes)
    }

    /// Lazily load one object entry from its schema shard, applying overrides.
    ///
    /// Key is `schema.object_name`. Returns `None` when the shard or entry is
    /// missing (matches TS `getObjectEntry`).
    pub fn get_object_entry(
        &self,
        base_dir: &Path,
        manifest: &IndexManifest,
        schema: &str,
        object_name: &str,
    ) -> Result<Option<ObjectEntry>, IndexError> {
        let ref_ = format!("{schema}.{object_name}");
        let Some(shard_info) = manifest.shards.iter().find(|s| s.schema == schema) else {
            return Ok(None);
        };
        let Some(entries) = self.read_shard_entries(base_dir, &shard_info.file)? else {
            return Ok(None);
        };
        let Some(entry) = entries.get(&ref_) else {
            return Ok(None);
        };

        let Some(overrides) = self.read_overrides(base_dir)? else {
            return Ok(Some(entry.clone()));
        };

        Ok(Some(apply_overrides(entry.clone(), &ref_, &overrides)))
    }

    /// Read embeddings meta + raw Float32 LE bytes. Returns `None` when manifest
    /// has no embeddings paths or files are missing.
    pub fn read_embeddings(
        &self,
        base_dir: &Path,
        manifest: &IndexManifest,
    ) -> Result<Option<(Vec<EmbeddingMetaEntry>, Vec<u8>)>, IndexError> {
        let (Some(bin_name), Some(meta_name)) =
            (&manifest.derived.embeddings, &manifest.derived.embeddings_meta)
        else {
            return Ok(None);
        };
        let meta: Vec<EmbeddingMetaEntry> = match self.read_json_opt(&base_dir.join(meta_name))? {
            Some(m) => m,
            None => return Ok(None),
        };
        let bin = match fs::read(base_dir.join(bin_name)) {
            Ok(b) => b,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        Ok(Some((meta, bin)))
    }

    pub fn write_embeddings(
        &self,
        base_dir: &Path,
        meta: &[EmbeddingMetaEntry],
        bin: &[u8],
    ) -> Result<(), IndexError> {
        let meta_bytes = serde_json::to_vec(meta)?;
        self.write_atomic(&base_dir.join(EMBEDDINGS_META), &meta_bytes)?;
        self.write_atomic(&base_dir.join(EMBEDDINGS_BIN), bin)
    }

    /// Atomic write: write `{path}.tmp` then rename over `path` (matches TS).
    pub fn write_atomic(&self, path: &Path, content: &[u8]) -> Result<(), IndexError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(content)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn clear_index(&self, connection_id: &str, database: &str) -> Result<(), IndexError> {
        let base = self.base_dir(connection_id, database);
        match fs::remove_dir_all(&base) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete files in `base_dir` not referenced by the manifest (keeps `.lock`).
    pub fn run_garbage_collection(
        &self,
        base_dir: &Path,
        manifest: &IndexManifest,
    ) -> Result<(), IndexError> {
        let mut active: Vec<&str> = vec![
            MANIFEST_FILE,
            &manifest.derived.tokens,
            &manifest.derived.join_graph,
        ];
        if let Some(ref e) = manifest.derived.values {
            active.push(e);
        }
        if let Some(ref e) = manifest.derived.embeddings {
            active.push(e);
        }
        if let Some(ref e) = manifest.derived.embeddings_meta {
            active.push(e);
        }
        for shard in &manifest.shards {
            active.push(&shard.file);
        }

        let entries = match fs::read_dir(base_dir) {
            Ok(d) => d,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        for ent in entries {
            let ent = ent?;
            if !ent.file_type()?.is_file() {
                continue;
            }
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if name == LOCK_FILE || active.iter().any(|a| *a == name) {
                continue;
            }
            let _ = fs::remove_file(ent.path());
        }
        Ok(())
    }

    fn write_lock_file(&self, lock_path: &Path) -> Result<(), IndexError> {
        let pid = std::process::id();
        let timestamp_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let body = serde_json::json!({ "pid": pid, "timestamp": timestamp_ms });
        fs::write(lock_path, body.to_string())?;
        Ok(())
    }

    fn read_json_opt<T: serde::de::DeserializeOwned>(
        &self,
        path: &Path,
    ) -> Result<Option<T>, IndexError> {
        let data = match fs::read(path) {
            Ok(d) => d,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        Ok(Some(serde_json::from_slice(&data)?))
    }
}

/// Apply `overrides.json` overlay onto a cloned [`ObjectEntry`] (TS `getObjectEntry`).
fn apply_overrides(mut entry: ObjectEntry, ref_: &str, overrides: &IndexOverrides) -> ObjectEntry {
    if let Some(objects) = &overrides.objects
        && let Some(obj) = objects.get(ref_)
    {
        if let Some(comment) = &obj.comment {
            entry.comment = comment.clone();
        }
        if let Some(excluded) = obj.excluded {
            entry.excluded = Some(excluded);
        }
        if let Some(col_overrides) = &obj.columns {
            for col in &mut entry.columns {
                if let Some(co) = col_overrides.get(&col.name) {
                    if let Some(comment) = &co.comment {
                        col.comment = comment.clone();
                    }
                    if let Some(pii) = co.pii {
                        col.pii = Some(pii);
                    }
                }
            }
        }
    }

    if let Some(joins) = &overrides.joins {
        let fks = entry.foreign_keys.get_or_insert_with(Vec::new);
        for edge in joins {
            if edge.disabled == Some(true) {
                fks.retain(|fk| {
                    let col0 = edge.cols.first().map(|(a, _)| a.as_str());
                    !(fk.columns.first().map(String::as_str) == col0 && fk.ref_table == edge.to)
                });
                continue;
            }
            if edge.from != ref_ {
                continue;
            }
            let new_fk = ForeignKeyEntry {
                columns: edge.cols.iter().map(|(a, _)| a.clone()).collect(),
                ref_table: edge.to.clone(),
                ref_columns: edge.cols.iter().map(|(_, b)| b.clone()).collect(),
                name: edge.via.clone(),
                on_delete: None,
                inferred: edge.inferred,
            };
            if let Some(idx) = fks.iter().position(|fk| {
                fk.ref_table == edge.to
                    && fk.columns.first() == edge.cols.first().map(|(a, _)| a)
            }) {
                fks[idx] = new_fk;
            } else {
                fks.push(new_fk);
            }
        }
    }

    entry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        BuildDepth, BuildMode, ColumnEntry, DbObjectKind, IndexCounts, IndexDerived, IndexScope,
        IndexStats, JoinEdge, ObjectShard,
    };
    use tempfile::TempDir;

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
                pii_excluded_columns: vec![],
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
                tokens: TOKENS_FILE.into(),
                join_graph: JOIN_GRAPH_FILE.into(),
                values: None,
                embeddings: Some(EMBEDDINGS_BIN.into()),
                embeddings_meta: Some(EMBEDDINGS_META.into()),
            },
            stats: IndexStats {
                build_ms: 42,
                queries_run: 9,
                warnings: vec![],
            },
        }
    }

    fn sample_object() -> ObjectEntry {
        ObjectEntry {
            kind: DbObjectKind::Table,
            oid: 16384,
            object_hash: "deadbeef".into(),
            comment: Some("users".into()),
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
        }
    }

    #[test]
    fn safe_segment_replaces_unsafe_chars() {
        assert_eq!(safe_segment("a/b:c"), "a_b_c");
        assert_eq!(safe_segment("ok-name_1"), "ok-name_1");
    }

    #[test]
    fn base_dir_layout_matches_ts() {
        let store = IndexStore::new("/tmp/storage");
        let dir = store.base_dir("conn/1", "my db");
        assert_eq!(
            dir,
            PathBuf::from("/tmp/storage/dbindex/conn_1/my_db")
        );
    }

    #[test]
    fn manifest_shard_joingraph_round_trip() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let base = store.base_dir("conn-1", "appdb");

        let manifest = sample_manifest();
        store.write_manifest(&base, &manifest).unwrap();

        let mut shard = HashMap::new();
        shard.insert("public.users".into(), sample_object());
        store
            .write_shard_entries(&base, "objects-public-0.json", &shard)
            .unwrap();

        let graph = JoinGraph {
            edges: vec![JoinEdge {
                from: "public.users".into(),
                to: "public.orgs".into(),
                via: "users_org_id_fkey".into(),
                cols: vec![("org_id".into(), "id".into())],
                inferred: None,
                disabled: None,
            }],
        };
        store.write_join_graph(&base, &graph).unwrap();

        let got_manifest = store.read_manifest(&base).unwrap().expect("manifest");
        assert_eq!(got_manifest, manifest);

        let got_shard = store
            .read_shard_entries(&base, "objects-public-0.json")
            .unwrap()
            .expect("shard");
        assert_eq!(got_shard["public.users"].oid, 16384);
        assert_eq!(got_shard["public.users"].object_hash, "deadbeef");

        let got_graph = store
            .read_join_graph(&base, &got_manifest)
            .unwrap()
            .expect("joingraph");
        assert_eq!(got_graph, graph);

        assert!(base.join(MANIFEST_FILE).is_file());
        assert!(base.join("objects-public-0.json").is_file());
        assert!(base.join(JOIN_GRAPH_FILE).is_file());
    }

    #[test]
    fn embeddings_bin_float32_le_row_major_hex() {
        // One vector [1.0, 2.0]: LE bytes 00 00 80 3f | 00 00 00 40
        let vectors = vec![vec![1.0_f32, 2.0_f32]];
        let bin = serialize_embeddings(&vectors, 2);
        assert_eq!(
            bin,
            [
                0x00, 0x00, 0x80, 0x3f, // 1.0
                0x00, 0x00, 0x00, 0x40, // 2.0
            ]
        );

        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let base = store.base_dir("c", "d");
        let manifest = sample_manifest();
        store.write_manifest(&base, &manifest).unwrap();

        let meta = vec![EmbeddingMetaEntry {
            ref_: "public.users".into(),
            object_hash: "deadbeef".into(),
            model: "minilm".into(),
            dim: 2,
        }];
        store.write_embeddings(&base, &meta, &bin).unwrap();

        let (got_meta, got_bin) = store
            .read_embeddings(&base, &manifest)
            .unwrap()
            .expect("embeddings");
        assert_eq!(got_meta, meta);
        assert_eq!(got_bin, bin);
        assert_eq!(
            deserialize_embedding(&got_bin, 0, 2).unwrap(),
            vec![1.0, 2.0]
        );
    }

    #[test]
    fn lock_acquire_release_and_stale() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let base = store.base_dir("c", "d");

        assert!(store.acquire_lock(&base).unwrap());
        assert!(!store.acquire_lock(&base).unwrap());

        store.release_lock(&base).unwrap();
        assert!(store.acquire_lock(&base).unwrap());

        // Force stale mtime
        let lock = base.join(LOCK_FILE);
        let old = SystemTime::now() - STALE_LOCK - Duration::from_secs(1);
        filetime_set_mtime(&lock, old);
        assert!(store.acquire_lock(&base).unwrap());
    }

    /// Set mtime without pulling in the `filetime` crate.
    fn filetime_set_mtime(path: &Path, mtime: SystemTime) {
        let f = fs::File::options().write(true).open(path).unwrap();
        f.set_modified(mtime).unwrap();
    }

    #[test]
    fn write_atomic_leaves_no_tmp() {
        let tmp = TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let path = tmp.path().join("out.json");
        store.write_atomic(&path, b"{\"ok\":true}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"ok\":true}");
        assert!(!PathBuf::from(format!("{}.tmp", path.display())).exists());
    }
}
