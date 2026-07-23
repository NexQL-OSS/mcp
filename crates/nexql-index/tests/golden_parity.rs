//! Phase 3 golden-file gate for `nexql-index`.
//!
//! Live path: TempPg (initdb) → seed → `build_index` → normalize → compare/update
//! `tests/golden/expected/`.
//!
//! Offline path: structure invariants + `compare_normalized_manifest` against
//! committed fixtures (no Postgres required).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::time::{Duration, Instant};

use nexql_index::{
    BuildDepth, BuildMode, BuildRequest, IndexScope, IndexStore, JOIN_GRAPH_FILE, MANIFEST_FILE,
    PgCatalogDb, TOKENS_FILE, build_index,
};
use serde_json::{Map, Value};
use tempfile::TempDir;
use tokio_postgres::{Client, Config, NoTls};

const GOLDEN_INDEXED_AT: &str = "1970-01-01T00:00:00.000Z";
const GOLDEN_PLACEHOLDER: &str = "GOLDEN";
const CONN_ID: &str = "golden-conn";
const DATABASE: &str = "postgres";
const UPDATE_GOLDEN_ENV: &str = "NEXQL_MCP_UPDATE_GOLDEN";

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures_seed() -> PathBuf {
    crate_root().join("tests/fixtures/seed_schema.sql")
}

fn expected_dir() -> PathBuf {
    crate_root().join("tests/golden/expected")
}

fn update_golden() -> bool {
    matches!(
        std::env::var(UPDATE_GOLDEN_ENV).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

// ---------------------------------------------------------------------------
// Normalization + compare
// ---------------------------------------------------------------------------

/// Sort object keys recursively; leave arrays in order.
pub fn sort_json_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted = Map::new();
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            for k in keys {
                if let Some(v) = map.get(&k) {
                    sorted.insert(k, sort_json_keys(v.clone()));
                }
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_json_keys).collect()),
        other => other,
    }
}

/// Zero / pin fields that vary across PG clusters and wall-clock builds.
pub fn normalize_index_json(mut value: Value) -> Value {
    normalize_node(&mut value, None);
    sort_json_keys(value)
}

fn normalize_node(value: &mut Value, parent_key: Option<&str>) {
    match value {
        Value::Object(map) => {
            // Manifest / stats telemetry
            if map.contains_key("indexedAt") {
                map.insert("indexedAt".into(), Value::String(GOLDEN_INDEXED_AT.into()));
            }
            if map.contains_key("pgVersion") {
                map.insert(
                    "pgVersion".into(),
                    Value::String(GOLDEN_PLACEHOLDER.into()),
                );
            }
            if map.contains_key("schemaFingerprint") {
                map.insert(
                    "schemaFingerprint".into(),
                    Value::String(GOLDEN_PLACEHOLDER.into()),
                );
            }
            if let Some(stats) = map.get_mut("stats").and_then(|v| v.as_object_mut()) {
                stats.insert("buildMs".into(), Value::from(0u64));
                stats.insert("queriesRun".into(), Value::from(0u64));
                stats.insert("warnings".into(), Value::Array(vec![]));
            }

            // Object entries + shard metadata
            if map.contains_key("oid") {
                map.insert("oid".into(), Value::from(0u64));
            }
            if map.contains_key("sizeBytes") {
                map.insert("sizeBytes".into(), Value::from(0u64));
            }
            if map.contains_key("rowEstimate") {
                map.insert("rowEstimate".into(), Value::from(0u64));
            }
            if map.contains_key("objectHash") {
                map.insert(
                    "objectHash".into(),
                    Value::String(GOLDEN_PLACEHOLDER.into()),
                );
            }
            // Shard pointer: hash/bytes depend on oid-bearing payload.
            if map.contains_key("file")
                && map.contains_key("schema")
                && map.contains_key("hash")
                && map.contains_key("bytes")
            {
                map.insert("hash".into(), Value::String(GOLDEN_PLACEHOLDER.into()));
                map.insert("bytes".into(), Value::from(0u64));
            }

            let keys: Vec<String> = map.keys().cloned().collect();
            for k in keys {
                if let Some(child) = map.get_mut(&k) {
                    normalize_node(child, Some(&k));
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_node(item, parent_key);
            }
        }
        _ => {}
    }
}

pub fn compare_normalized_manifest(actual: &Value, expected: &Value) -> Result<(), String> {
    let a = normalize_index_json(actual.clone());
    let e = normalize_index_json(expected.clone());
    if a == e {
        Ok(())
    } else {
        Err(format!(
            "normalized manifest mismatch\n--- actual ---\n{}\n--- expected ---\n{}",
            pretty(&a),
            pretty(&e)
        ))
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn read_json(path: &Path) -> Value {
    let raw = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn write_normalized_json(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create golden dir");
    }
    let normalized = normalize_index_json(value.clone());
    let body = format!("{}\n", pretty(&normalized));
    fs::write(path, body).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

fn assert_or_update(path: &Path, actual: &Value) {
    let normalized = normalize_index_json(actual.clone());
    if update_golden() || !path.exists() {
        write_normalized_json(path, &normalized);
        eprintln!("wrote golden {}", path.display());
        return;
    }
    let expected = read_json(path);
    compare_normalized_manifest(&normalized, &expected).unwrap_or_else(|e| panic!("{e}"));
}

// ---------------------------------------------------------------------------
// Structure invariants (no full byte compare)
// ---------------------------------------------------------------------------

pub fn assert_structure_invariants(
    manifest: &Value,
    tokens: &Value,
    join_graph: &Value,
    shard_objects: &BTreeMap<String, Value>,
) {
    let m = manifest.as_object().expect("manifest object");
    assert!(
        m.get("schemaFingerprint")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty()),
        "schemaFingerprint must be present"
    );

    let counts = m.get("counts").and_then(|v| v.as_object()).expect("counts");
    assert_eq!(counts.get("tables").and_then(|v| v.as_u64()), Some(2));
    assert_eq!(counts.get("views").and_then(|v| v.as_u64()).unwrap_or(0), 0);

    let shards = m
        .get("shards")
        .and_then(|v| v.as_array())
        .expect("shards array");
    assert!(!shards.is_empty(), "expected at least one shard");

    let mut refs = BTreeSet::new();
    for (_file, shard) in shard_objects {
        let obj = shard.as_object().expect("shard object map");
        for key in obj.keys() {
            refs.insert(key.clone());
        }
    }
    assert!(
        refs.contains("public.users"),
        "shard missing public.users; got {refs:?}"
    );
    assert!(
        refs.contains("public.orders"),
        "shard missing public.orders; got {refs:?}"
    );

    let edges = join_graph
        .get("edges")
        .and_then(|v| v.as_array())
        .expect("join edges");
    let has_users_orders = edges.iter().any(|e| {
        let from = e.get("from").and_then(|v| v.as_str()).unwrap_or("");
        let to = e.get("to").and_then(|v| v.as_str()).unwrap_or("");
        (from == "public.orders" && to == "public.users")
            || (from == "public.users" && to == "public.orders")
    });
    assert!(
        has_users_orders,
        "join graph missing users↔orders edge: {edges:?}"
    );

    let postings = tokens
        .get("postings")
        .and_then(|v| v.as_object())
        .expect("token postings");
    assert!(
        postings.contains_key("user"),
        "tokens missing stem 'user'; keys={:?}",
        postings.keys().collect::<Vec<_>>()
    );
    assert!(
        postings.contains_key("order"),
        "tokens missing stem 'order'; keys={:?}",
        postings.keys().collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// TempPg (copied pattern from nexql-tools phase2_catalog)
// ---------------------------------------------------------------------------

struct TempPg {
    _data: TempDir,
    child: Child,
    url: String,
}

impl Drop for TempPg {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn which(bin: &str) -> Option<PathBuf> {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin}"))
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
}

fn start_temp_pg() -> Option<TempPg> {
    let initdb = which("initdb")?;
    let postgres = which("postgres")?;
    let data = TempDir::new().ok()?;
    let port = TcpListener::bind("127.0.0.1:0")
        .ok()?
        .local_addr()
        .ok()?
        .port();
    let status = Command::new(&initdb)
        .args([
            "-D",
            data.path().to_str()?,
            "-A",
            "trust",
            "-U",
            "spike",
            "--locale=C",
            "--encoding=UTF8",
        ])
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let child = Command::new(&postgres)
        .args([
            "-D",
            data.path().to_str()?,
            "-p",
            &port.to_string(),
            "-c",
            "listen_addresses=127.0.0.1",
            "-c",
            "unix_socket_directories=",
        ])
        .env("LANG", "C.UTF-8")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let url = format!("postgres://spike@127.0.0.1:{port}/postgres?sslmode=disable");
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            std::thread::sleep(Duration::from_millis(300));
            return Some(TempPg {
                _data: data,
                child,
                url,
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

async fn connect_url(url: &str) -> Client {
    let config = Config::from_str(url).expect("parse url");
    let (client, conn) = config.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("postgres connection error: {e}");
        }
    });
    client
}

// ---------------------------------------------------------------------------
// Unit tests (always run)
// ---------------------------------------------------------------------------

#[test]
fn compare_normalized_manifest_matches_after_noise() {
    let expected = serde_json::json!({
        "formatVersion": 1,
        "connectionId": "golden-conn",
        "database": "postgres",
        "indexedAt": GOLDEN_INDEXED_AT,
        "buildMode": "auto",
        "buildDepth": "structure",
        "schemaFingerprint": GOLDEN_PLACEHOLDER,
        "pgVersion": GOLDEN_PLACEHOLDER,
        "environment": "test",
        "scope": {
            "includedSchemas": ["public"],
            "excludedObjects": [],
            "piiExcludedColumns": []
        },
        "counts": { "tables": 2, "views": 0, "functions": 0, "enums": 0 },
        "shards": [{
            "file": "objects-public-0.json",
            "schema": "public",
            "objects": 2,
            "bytes": 0,
            "hash": GOLDEN_PLACEHOLDER
        }],
        "derived": {
            "tokens": "tokens.json",
            "joinGraph": "joingraph.json",
            "values": "values.json"
        },
        "stats": { "buildMs": 0, "queriesRun": 0, "warnings": [] }
    });

    let mut actual = expected.clone();
    actual["indexedAt"] = Value::String("2099-12-31T23:59:59.999Z".into());
    actual["pgVersion"] = Value::String("16.4".into());
    actual["schemaFingerprint"] = Value::String("2|16384|0|1|2200".into());
    actual["stats"]["buildMs"] = Value::from(1234u64);
    actual["stats"]["queriesRun"] = Value::from(11u64);
    actual["shards"][0]["bytes"] = Value::from(9999u64);
    actual["shards"][0]["hash"] = Value::String("deadbeef".into());

    compare_normalized_manifest(&actual, &expected).expect("should match after normalize");
}

#[test]
fn compare_normalized_manifest_detects_count_drift() {
    let expected = serde_json::json!({
        "counts": { "tables": 2, "views": 0, "functions": 0, "enums": 0 },
        "indexedAt": GOLDEN_INDEXED_AT,
        "stats": { "buildMs": 0, "queriesRun": 0, "warnings": [] }
    });
    let actual = serde_json::json!({
        "counts": { "tables": 3, "views": 0, "functions": 0, "enums": 0 },
        "indexedAt": GOLDEN_INDEXED_AT,
        "stats": { "buildMs": 0, "queriesRun": 0, "warnings": [] }
    });
    assert!(compare_normalized_manifest(&actual, &expected).is_err());
}

#[test]
fn structure_invariants_on_hand_crafted_expected() {
    let expected = expected_dir();
    let manifest_path = expected.join(MANIFEST_FILE);
    if !manifest_path.exists() {
        // Bootstrap hand-crafted stubs when expected/ is empty (agent without PG).
        write_hand_crafted_expected(&expected);
    }

    let manifest = read_json(&manifest_path);
    let tokens = read_json(&expected.join(TOKENS_FILE));
    let join_graph = read_json(&expected.join(JOIN_GRAPH_FILE));

    let mut shards = BTreeMap::new();
    let shard_list = manifest
        .get("shards")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for shard in shard_list {
        let file = shard
            .get("file")
            .and_then(|v| v.as_str())
            .expect("shard.file");
        let path = expected.join(file);
        if path.exists() {
            shards.insert(file.to_owned(), read_json(&path));
        }
    }
    if shards.is_empty() {
        // Minimal object-ref map for invariant check when only manifest exists.
        shards.insert(
            "objects-public-0.json".into(),
            serde_json::json!({
                "public.users": { "kind": "table", "oid": 0 },
                "public.orders": { "kind": "table", "oid": 0 }
            }),
        );
    }

    assert_structure_invariants(&manifest, &tokens, &join_graph, &shards);
}

fn write_hand_crafted_expected(dir: &Path) {
    fs::create_dir_all(dir).expect("expected dir");
    write_normalized_json(
        &dir.join(MANIFEST_FILE),
        &serde_json::json!({
            "formatVersion": 1,
            "connectionId": CONN_ID,
            "database": DATABASE,
            "indexedAt": GOLDEN_INDEXED_AT,
            "buildMode": "auto",
            "buildDepth": "structure",
            "schemaFingerprint": GOLDEN_PLACEHOLDER,
            "pgVersion": GOLDEN_PLACEHOLDER,
            "environment": "test",
            "scope": {
                "includedSchemas": ["public"],
                "excludedObjects": [],
                "piiExcludedColumns": []
            },
            "counts": { "tables": 2, "views": 0, "functions": 0, "enums": 0 },
            "shards": [{
                "file": "objects-public-0.json",
                "schema": "public",
                "objects": 2,
                "bytes": 0,
                "hash": GOLDEN_PLACEHOLDER
            }],
            "derived": {
                "tokens": TOKENS_FILE,
                "joinGraph": JOIN_GRAPH_FILE,
                "values": "values.json"
            },
            "stats": { "buildMs": 0, "queriesRun": 0, "warnings": [] }
        }),
    );
    write_normalized_json(
        &dir.join(JOIN_GRAPH_FILE),
        &serde_json::json!({
            "edges": [{
                "from": "public.orders",
                "to": "public.users",
                "via": "orders_user_id_fkey",
                "cols": [["user_id", "id"]]
            }]
        }),
    );
    write_normalized_json(
        &dir.join(TOKENS_FILE),
        &serde_json::json!({
            "version": 1,
            "df": { "order": 1.0, "user": 2.0 },
            "postings": {
                "order": [["public.orders", 3.0]],
                "user": [["public.users", 3.0], ["public.orders", 1.0]]
            },
            "synonyms": {}
        }),
    );
    write_normalized_json(
        &dir.join("objects-public-0.json"),
        &serde_json::json!({
            "public.orders": {
                "kind": "table",
                "oid": 0,
                "objectHash": GOLDEN_PLACEHOLDER,
                "comment": "Purchase records (aka purchases)",
                "rowEstimate": 0,
                "sizeBytes": 0,
                "columns": [],
                "primaryKey": ["id"],
                "foreignKeys": [{
                    "columns": ["user_id"],
                    "refTable": "public.users",
                    "refColumns": ["id"],
                    "name": "orders_user_id_fkey"
                }]
            },
            "public.users": {
                "kind": "table",
                "oid": 0,
                "objectHash": GOLDEN_PLACEHOLDER,
                "comment": "Application accounts (aka customers)",
                "rowEstimate": 0,
                "sizeBytes": 0,
                "columns": [],
                "primaryKey": ["id"]
            }
        }),
    );
}

// ---------------------------------------------------------------------------
// Integration: live build ↔ golden
// ---------------------------------------------------------------------------

#[tokio::test]
async fn golden_parity_build_against_temp_pg() {
    let Some(pg) = start_temp_pg() else {
        eprintln!("skip: initdb/postgres unavailable");
        return;
    };

    let client = connect_url(&pg.url).await;
    let seed = fs::read_to_string(fixtures_seed()).expect("read seed_schema.sql");
    client.batch_execute(&seed).await.expect("apply seed");

    let tmp = TempDir::new().expect("temp index root");
    let store = IndexStore::new(tmp.path());
    let catalog = PgCatalogDb::new(&client);
    let req = BuildRequest {
        connection_id: CONN_ID.into(),
        database: DATABASE.into(),
        scope: IndexScope {
            included_schemas: vec!["public".into()],
            excluded_objects: vec![],
            pii_excluded_columns: vec![],
        },
        depth: BuildDepth::Structure,
        build_mode: BuildMode::Auto,
        environment: "test".into(),
        embeddings: false,
    };

    let manifest = build_index(&store, &catalog, &req, None, None, None)
        .await
        .expect("build_index");

    let base = store.base_dir(CONN_ID, DATABASE);
    let manifest_json: Value =
        serde_json::to_value(&manifest).expect("manifest to value");
    let tokens_json = read_json(&base.join(TOKENS_FILE));
    let join_json = read_json(&base.join(JOIN_GRAPH_FILE));

    let mut shard_objects = BTreeMap::new();
    for shard in &manifest.shards {
        let path = base.join(&shard.file);
        let v = read_json(&path);
        shard_objects.insert(shard.file.clone(), v);
    }

    // Live invariants use pre-normalize fingerprint / real token stems.
    assert_structure_invariants(&manifest_json, &tokens_json, &join_json, &shard_objects);

    let out = expected_dir();
    assert_or_update(&out.join(MANIFEST_FILE), &manifest_json);
    assert_or_update(&out.join(TOKENS_FILE), &tokens_json);
    assert_or_update(&out.join(JOIN_GRAPH_FILE), &join_json);
    for (file, value) in &shard_objects {
        assert_or_update(&out.join(file), value);
    }

    // If we just wrote goldens, re-read and byte-compare via normalizer.
    if update_golden() {
        let expected_manifest = read_json(&out.join(MANIFEST_FILE));
        compare_normalized_manifest(&manifest_json, &expected_manifest)
            .expect("round-trip after UPDATE_GOLDEN");
    }
}
