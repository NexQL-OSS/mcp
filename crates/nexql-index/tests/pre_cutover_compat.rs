//! Phase 3 / Phase 7 gate: Rust `IndexStore` + `IndexQueryService` must read a
//! pre-cutover on-disk index laid out exactly as the VS Code extension writes
//! under `{globalStorage}/dbindex/{conn}/{db}/`.
//!
//! Fixture source: `tests/golden/pre_cutover/` (formatVersion=1, same schema as
//! TS `IndexBuilder`). Cross-check against `tests/golden/ts/` (committed twin of
//! `expected/` for structural parity).

use std::fs;
use std::path::{Path, PathBuf};

use nexql_index::{
    IndexQueryService, IndexStore, JOIN_GRAPH_FILE, MANIFEST_FILE, SearchOptions, TOKENS_FILE,
};
use serde_json::Value;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn pre_cutover_root() -> PathBuf {
    crate_root().join("tests/golden/pre_cutover")
}

fn ts_golden_dir() -> PathBuf {
    crate_root().join("tests/golden/ts")
}

fn expected_dir() -> PathBuf {
    crate_root().join("tests/golden/expected")
}

fn read_json(path: &Path) -> Value {
    let raw = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

#[test]
fn pre_cutover_fixture_layout_matches_extension_dbindex() {
    let base = pre_cutover_root().join("dbindex/golden-conn/postgres");
    assert!(
        base.join(MANIFEST_FILE).is_file(),
        "missing {}",
        base.join(MANIFEST_FILE).display()
    );
    assert!(base.join(TOKENS_FILE).is_file());
    assert!(base.join(JOIN_GRAPH_FILE).is_file());
    assert!(base.join("objects-public-0.json").is_file());
}

#[test]
fn rust_index_store_reads_pre_cutover_fixture() {
    let store = IndexStore::new(pre_cutover_root());
    let listed = store
        .list_indexed_databases()
        .expect("list_indexed_databases");
    assert!(
        listed
            .iter()
            .any(|(c, d)| c == "golden-conn" && d == "postgres"),
        "expected golden-conn/postgres in {listed:?}"
    );

    let base = store.base_dir("golden-conn", "postgres");
    let manifest = store
        .read_manifest(&base)
        .expect("read_manifest")
        .expect("manifest present");
    assert_eq!(manifest.format_version, 1);
    assert_eq!(manifest.connection_id, "golden-conn");
    assert_eq!(manifest.database, "postgres");
    assert_eq!(manifest.counts.tables, 2);

    let users = store
        .get_object_entry(&base, &manifest, "public", "users")
        .expect("get users")
        .expect("public.users");
    assert_eq!(users.kind.as_str(), "table");

    let tokens = store
        .read_tokens(&base, &manifest)
        .expect("tokens")
        .expect("tokens present");
    assert!(tokens.postings.contains_key("user"));

    let graph = store
        .read_join_graph(&base, &manifest)
        .expect("joingraph")
        .expect("joingraph present");
    assert!(
        graph.edges.iter().any(|e| {
            (e.from == "public.orders" && e.to == "public.users")
                || (e.from == "public.users" && e.to == "public.orders")
        }),
        "missing users↔orders edge: {:?}",
        graph.edges
    );
}

#[test]
fn rust_query_service_searches_pre_cutover_fixture() {
    let store = IndexStore::new(pre_cutover_root());
    let qs = IndexQueryService::new(&store, "golden-conn", "postgres");
    let hits = qs
        .search_schema("user", 10, None, SearchOptions::default())
        .expect("search_schema");
    assert!(
        hits.iter().any(|h| h.ref_ == "public.users"),
        "expected public.users in hits: {hits:?}"
    );

    let entry = qs.describe_object("public.users", None).expect("describe");
    assert_eq!(entry.kind.as_str(), "table");
}

#[test]
fn ts_golden_matches_expected_byte_for_byte() {
    // `ts/` is the committed stand-in for TS IndexBuilder output (same formatVersion=1
    // artifacts as `expected/`). When a live TS harness lands, regenerate `ts/` from
    // IndexBuilder against seed_schema.sql and keep this gate.
    for name in [MANIFEST_FILE, TOKENS_FILE, JOIN_GRAPH_FILE, "objects-public-0.json"] {
        let ts_path = ts_golden_dir().join(name);
        let exp_path = expected_dir().join(name);
        assert!(ts_path.is_file(), "missing {}", ts_path.display());
        assert!(exp_path.is_file(), "missing {}", exp_path.display());
        let ts = read_json(&ts_path);
        let exp = read_json(&exp_path);
        assert_eq!(
            ts, exp,
            "ts/{name} diverged from expected/{name} — regenerate both from the same seed"
        );
    }
}

#[test]
fn pre_cutover_manifest_matches_ts_golden() {
    let pre = read_json(
        &pre_cutover_root().join("dbindex/golden-conn/postgres").join(MANIFEST_FILE),
    );
    let ts = read_json(&ts_golden_dir().join(MANIFEST_FILE));
    assert_eq!(
        pre, ts,
        "pre_cutover manifest drifted from ts/ golden — run scripts/sync_pre_cutover_fixture.sh"
    );
}
