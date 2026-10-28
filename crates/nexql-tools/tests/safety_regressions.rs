// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Live Postgres checks for read-mode escapes and PII lineage.
//! Skipped when `initdb` is not on `PATH` (the unit-test CI job).

mod common;

use nexql_conn::{ProfileConfig, params_from_url};
use nexql_policy::{AccessMode, PolicyCaps};
use nexql_tools::{ConnectionInfo, ToolRouter, ToolSession, policy_from_profile};
use serde_json::json;

#[tokio::test]
async fn read_select_blocks_escapes_redacts_lineage_and_uses_readonly_txn() {
    let Some(pg) = common::start_temp_pg() else {
        eprintln!("skip: initdb/postgres not on PATH");
        return;
    };
    let params = params_from_url(&pg.url).unwrap();
    let setup = nexql_conn::connect_once(&params).await.unwrap();
    setup
        .batch_execute(
            "CREATE TABLE public.users (
                id int PRIMARY KEY,
                email text NOT NULL,
                password_hash text NOT NULL
             );
             INSERT INTO public.users (id, email, password_hash)
             VALUES (1, 'a@b.co', 'hashed-secret');",
        )
        .await
        .unwrap();
    drop(setup);

    let profile = ProfileConfig {
        pii_columns: vec!["public.users.email".into(), "public.users.phone".into()],
        ..Default::default()
    };
    let info = ConnectionInfo {
        id: "default".into(),
        name: "default".into(),
        host: params.host.clone(),
        port: params.port,
        database: params.dbname.clone(),
        params,
        policy: policy_from_profile(
            Some(&profile),
            AccessMode::Read,
            PolicyCaps::default().with_max_rows(20),
        ),
    };
    let session = ToolSession::from_connections(vec![info], None)
        .await
        .unwrap();
    let router = ToolRouter::new(session);

    for sql in [
        "SELECT pg_read_file('/etc/passwd')",
        "SELECT pg_catalog.pg_read_file('/etc/passwd')",
        "SELECT (SELECT pg_read_file('/etc/passwd'))",
        "WITH f AS (SELECT pg_read_file('/etc/passwd') AS body) SELECT body FROM f",
        "SELECT set_config('default_transaction_read_only', 'off', false)",
    ] {
        let out = router.call("run_select", json!({ "sql": sql })).await;
        assert!(out.is_error, "{sql}: {}", out.text);
        assert!(
            out.text.contains("blocked") || out.text.contains("allowed only"),
            "{sql}: {}",
            out.text
        );
    }

    let txn = router
        .call(
            "run_select",
            json!({ "sql": "SELECT current_setting('transaction_read_only') AS ro" }),
        )
        .await;
    assert!(!txn.is_error, "{}", txn.text);
    assert!(
        txn.text.contains("\"on\""),
        "read-mode query must run inside a read-only transaction: {}",
        txn.text
    );

    let alias = router
        .call(
            "run_select",
            json!({ "sql": "SELECT email AS contact FROM users" }),
        )
        .await;
    assert!(!alias.is_error, "{}", alias.text);
    assert!(alias.text.contains("<redacted>"), "{}", alias.text);
    assert!(!alias.text.contains("a@b.co"), "{}", alias.text);

    let lower = router
        .call(
            "run_select",
            json!({ "sql": "SELECT lower(email) FROM users" }),
        )
        .await;
    assert!(!lower.is_error, "{}", lower.text);
    assert!(lower.text.contains("<redacted>"), "{}", lower.text);
    assert!(!lower.text.contains("a@b.co"), "{}", lower.text);

    let whole = router
        .call(
            "run_select",
            json!({ "sql": "SELECT row_to_json(u) FROM users u" }),
        )
        .await;
    assert!(whole.is_error, "{}", whole.text);
    assert!(
        whole.text.contains("password_hash") || whole.text.contains("email"),
        "{}",
        whole.text
    );
    assert!(!whole.text.contains("hashed-secret"), "{}", whole.text);
}
