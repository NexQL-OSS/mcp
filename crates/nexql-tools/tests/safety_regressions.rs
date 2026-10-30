// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Live Postgres checks for read-mode escapes and PII lineage.
//! Skipped when `initdb` is not on `PATH`, unless `NEXQL_REQUIRE_LIVE_PG=1`
//! (the Linux CI test job), which fails the test instead of skipping.

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
        "SELECT query_to_xml('select pg_read_file(''/etc/passwd'')', true, false, '')",
        "SELECT table_to_xml('public.users', true, false, '')",
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

    // A set-returning function in FROM must not launder a whole row.
    let lateral = router
        .call(
            "run_select",
            json!({ "sql": "SELECT kv.key, kv.value FROM users u \
                            CROSS JOIN LATERAL jsonb_each_text(to_jsonb(u)) kv" }),
        )
        .await;
    assert!(lateral.is_error, "{}", lateral.text);
    assert!(!lateral.text.contains("a@b.co"), "{}", lateral.text);
    assert!(!lateral.text.contains("hashed-secret"), "{}", lateral.text);

    // A scalar subquery is named after its inner column, not "?column?".
    for sql in [
        "SELECT (SELECT email AS contact FROM users LIMIT 1)",
        "SELECT (SELECT email FROM users LIMIT 1)",
        "SELECT CASE WHEN id > 0 THEN '' ELSE email END FROM users",
        "SELECT (email || '')::text FROM users",
        "SELECT xmlelement(name x, email) FROM users",
    ] {
        let out = router.call("run_select", json!({ "sql": sql })).await;
        assert!(!out.is_error, "{sql}: {}", out.text);
        assert!(!out.text.contains("a@b.co"), "{sql}: {}", out.text);
    }

    // Unqualified catalog views resolve through search_path, not `public`.
    let catalog = router
        .call(
            "run_select",
            json!({ "sql": "SELECT * FROM pg_tables WHERE schemaname = 'public'" }),
        )
        .await;
    assert!(!catalog.is_error, "{}", catalog.text);
}

#[tokio::test]
async fn begin_read_only_refuses_an_open_read_write_transaction() {
    let Some(pg) = common::start_temp_pg() else {
        eprintln!("skip: initdb/postgres not on PATH");
        return;
    };
    let params = params_from_url(&pg.url).unwrap();
    let client = nexql_conn::connect_once(&params).await.unwrap();

    // BEGIN inside an open transaction only warns, so this used to slip by.
    client.batch_execute("BEGIN READ WRITE").await.unwrap();
    let err = nexql_conn::begin_read_only(&client).await.unwrap_err();
    assert!(err.to_string().contains("read-write transaction"), "{err}");

    // The stray transaction was rolled back, so a fresh one works.
    nexql_conn::begin_read_only(&client).await.unwrap();
    let ro: String = client
        .query_one("SHOW transaction_read_only", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(ro, "on");
    client.batch_execute("COMMIT").await.unwrap();
}

#[tokio::test]
async fn adversarial_bypass_matrix() {
    let Some(pg) = common::start_temp_pg() else {
        eprintln!("skip: initdb/postgres not on PATH");
        return;
    };
    let params = params_from_url(&pg.url).unwrap();
    let setup = nexql_conn::connect_once(&params).await.unwrap();
    setup
        .batch_execute(
            "CREATE SCHEMA customers;
             CREATE TABLE public.users (
                id int PRIMARY KEY,
                email text NOT NULL,
                password_hash text NOT NULL,
                secretary text NOT NULL,
                tag text NOT NULL
             );
             INSERT INTO public.users VALUES (1, 'a@b.co', 'hashed-secret', 'pat', 'from-public');
             CREATE TABLE public.contacts (id int PRIMARY KEY, city text);
             INSERT INTO public.contacts VALUES (1, 'paris');
             CREATE TABLE public.orders (id int PRIMARY KEY, user_id int, total int);
             INSERT INTO public.orders VALUES (1, 1, 42);
             CREATE TABLE customers.users (
                id int PRIMARY KEY,
                email text NOT NULL,
                password_hash text NOT NULL,
                tag text NOT NULL
             );
             INSERT INTO customers.users VALUES (1, 'cust@b.co', 'cust-hash', 'from-customers');
             CREATE VIEW public.user_emails AS
                SELECT id, email, password_hash, secretary FROM public.users;
             CREATE MATERIALIZED VIEW public.user_emails_m AS
                SELECT id, email, password_hash FROM public.users;
             CREATE FUNCTION public.wrap_read(path text) RETURNS text
                LANGUAGE sql AS $$ SELECT pg_read_file(path) $$;
             CREATE FUNCTION public.wrap2(path text) RETURNS text
                LANGUAGE sql AS $$ SELECT public.wrap_read(path) $$;
             CREATE FUNCTION public.benign_secdef() RETURNS int
                LANGUAGE sql SECURITY DEFINER AS $$ SELECT 1 $$;
             ALTER ROLE spike SET search_path TO customers, public;",
        )
        .await
        .unwrap();
    drop(setup);

    let pii = vec!["customers.users.email".into(), "public.users.email".into()];
    let read = open_router(params.clone(), pii.clone(), AccessMode::Read).await;
    let write = open_router(params, pii, AccessMode::Write).await;

    let secrets = [
        "a@b.co",
        "cust@b.co",
        "hashed-secret",
        "cust-hash",
        "returning-secret",
    ];

    // search_path: unqualified `users` is customers.users, not public.users.
    let path = read
        .call(
            "run_select",
            json!({ "sql": "SELECT tag, email AS contact FROM users" }),
        )
        .await;
    assert!(!path.is_error, "search_path: {}", path.text);
    assert!(path.text.contains("from-customers"), "{}", path.text);
    assert!(!path.text.contains("from-public"), "{}", path.text);
    assert!(!path.text.contains("cust@b.co"), "{}", path.text);
    assert!(path.text.contains("<redacted>"), "{}", path.text);

    let ordinary = [
        (
            "join",
            "SELECT u.id, o.total FROM public.users u JOIN public.orders o ON o.user_id = u.id",
            "42",
        ),
        (
            "aggregate",
            "SELECT count(*) AS n, max(id) AS m FROM public.users",
            "1",
        ),
        (
            "subquery-non-pii",
            "SELECT (SELECT id FROM public.users LIMIT 1) AS i",
            "1",
        ),
        (
            "generate_series",
            "SELECT g FROM generate_series(1, 3) g",
            "1",
        ),
        (
            "lateral-generate_series",
            "SELECT u.id, x.n FROM public.users u CROSS JOIN LATERAL generate_series(1, u.id) AS x(n)",
            "1",
        ),
        ("secretary", "SELECT secretary FROM public.users", "pat"),
    ];
    for (label, sql, expect) in ordinary {
        let out = read.call("run_select", json!({ "sql": sql })).await;
        assert!(!out.is_error, "{label}: {}", out.text);
        assert!(out.text.contains(expect), "{label}: {}", out.text);
        for secret in secrets {
            assert!(
                !out.text.contains(secret),
                "{label} leaked {secret}: {}",
                out.text
            );
        }
    }

    let redacted = [
        (
            "case",
            "SELECT CASE WHEN id > 0 THEN email ELSE '' END FROM public.users",
        ),
        (
            "subquery",
            "SELECT (SELECT email FROM public.users LIMIT 1)",
        ),
        ("coalesce", "SELECT coalesce(email, 'x') FROM public.users"),
        ("array", "SELECT ARRAY[email] FROM public.users"),
        (
            "xmlelement",
            "SELECT xmlelement(name x, email) FROM public.users",
        ),
        ("format", "SELECT format('%s', email) FROM public.users"),
        (
            "string_agg",
            "SELECT string_agg(email, ',') FROM public.users",
        ),
        (
            "recursive-cte",
            "WITH RECURSIVE r(n, e) AS (\
               SELECT 1, email FROM public.users WHERE id = 1 \
               UNION ALL SELECT n + 1, e FROM r WHERE n < 2\
             ) SELECT e AS out FROM r",
        ),
        (
            "union",
            "SELECT email FROM public.users UNION ALL SELECT email FROM public.users",
        ),
        (
            "window",
            "SELECT first_value(email) OVER (ORDER BY id) FROM public.users",
        ),
        (
            "using",
            "SELECT email FROM public.users JOIN public.contacts USING (id)",
        ),
        (
            "natural-join",
            "SELECT email FROM public.users NATURAL JOIN public.contacts",
        ),
        ("view", "SELECT email AS contact FROM public.user_emails"),
        ("matview", "SELECT email FROM public.user_emails_m"),
    ];
    for (label, sql) in redacted {
        let out = read.call("run_select", json!({ "sql": sql })).await;
        assert!(!out.is_error, "{label}: {}", out.text);
        assert!(out.text.contains("<redacted>"), "{label}: {}", out.text);
        for secret in secrets {
            assert!(
                !out.text.contains(secret),
                "{label} leaked {secret}: {}",
                out.text
            );
        }
    }

    let refused = [
        "SELECT kv.key, kv.value FROM public.users u CROSS JOIN LATERAL jsonb_each_text(to_jsonb(u)) kv",
        "SELECT query_to_xml('select pg_read_file(''/etc/passwd'')', true, false, '')",
        "SELECT table_to_xml('public.users', true, false, '')",
        "SELECT cursor_to_xml('c', 10, true, false, '')",
        "BEGIN READ WRITE; SELECT email FROM public.users",
        "SELECT loread(0, 10)",
        "SELECT lowrite(0, 'x')",
        "SELECT lo_export(1, '/tmp/x')",
        "SELECT public.wrap_read('/etc/passwd')",
        "SELECT wrap2('/etc/passwd')",
        "SELECT benign_secdef()",
    ];
    for sql in refused {
        let out = read.call("run_select", json!({ "sql": sql })).await;
        assert!(out.is_error, "{sql}: {}", out.text);
        for secret in secrets {
            assert!(
                !out.text.contains(secret),
                "{sql} leaked {secret}: {}",
                out.text
            );
        }
        assert!(
            !out.text.contains("root:"),
            "{sql} leaked a file: {}",
            out.text
        );
    }

    let returning = write
        .call(
            "execute_sql",
            json!({
                "sql": "INSERT INTO public.users (id, email, password_hash, secretary, tag) \
                        VALUES (2, 'c@d.co', 'returning-secret', 'pat', 'from-public') \
                        RETURNING email, password_hash",
                "dry_run": true
            }),
        )
        .await;
    assert!(!returning.is_error, "{}", returning.text);
    assert!(returning.text.contains("<redacted>"), "{}", returning.text);
    assert!(!returning.text.contains("c@d.co"), "{}", returning.text);
    assert!(
        !returning.text.contains("returning-secret"),
        "{}",
        returning.text
    );
}

async fn open_router(
    params: nexql_conn::ConnectionParams,
    pii: Vec<String>,
    mode: AccessMode,
) -> ToolRouter {
    let info = ConnectionInfo {
        id: "default".into(),
        name: "default".into(),
        host: params.host.clone(),
        port: params.port,
        database: params.dbname.clone(),
        params,
        policy: policy_from_profile(
            Some(&ProfileConfig {
                pii_columns: pii,
                ..Default::default()
            }),
            mode,
            PolicyCaps::default().with_max_rows(20),
        ),
    };
    let session = ToolSession::from_connections(vec![info], None)
        .await
        .unwrap();
    ToolRouter::new(session)
}
