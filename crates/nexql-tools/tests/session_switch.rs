// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Integration: `switch_connection` must pool by (connection_id, database).

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use std::sync::Arc;

use nexql_conn::params_from_url;
use nexql_policy::{AccessMode, PolicyCaps};
use nexql_tools::{ConnectionInfo, ToolSession};
use tempfile::TempDir;

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
    let started = start_temp_pg_inner();
    if started.is_none() && std::env::var("NEXQL_REQUIRE_LIVE_PG").ok().as_deref() == Some("1") {
        panic!("NEXQL_REQUIRE_LIVE_PG=1 but initdb/postgres could not start a temporary server");
    }
    started
}

fn start_temp_pg_inner() -> Option<TempPg> {
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
        // An open port is not enough: the server rejects connections while it
        // is still starting up. Postgres writes `ready` as the 8th line of
        // postmaster.pid once it accepts connections.
        let ready = std::fs::read_to_string(data.path().join("postmaster.pid")).is_ok_and(|pid| {
            pid.lines()
                .nth(7)
                .is_some_and(|line| line.trim() == "ready")
        });
        if ready {
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

async fn session_for(url: &str) -> Arc<ToolSession> {
    let params = params_from_url(url).unwrap();
    let info = ConnectionInfo {
        id: "default".into(),
        name: "default".into(),
        host: params.host.clone(),
        port: params.port,
        database: params.dbname.clone(),
        params,
        policy: nexql_tools::session::policy_from_profile(
            None,
            AccessMode::Read,
            PolicyCaps::default(),
        ),
    };
    ToolSession::from_connections(vec![info], None)
        .await
        .unwrap()
}

#[tokio::test]
async fn switch_connection_uses_target_database() {
    let Some(pg) = start_temp_pg() else {
        eprintln!("skip: initdb/postgres unavailable");
        return;
    };

    {
        let params = params_from_url(&pg.url).unwrap();
        let bootstrap = ToolSession::from_connections(
            vec![ConnectionInfo {
                id: "default".into(),
                name: "default".into(),
                host: params.host.clone(),
                port: params.port,
                database: params.dbname.clone(),
                params,
                policy: nexql_tools::session::policy_from_profile(
                    None,
                    AccessMode::Write,
                    PolicyCaps::default(),
                ),
            }],
            None,
        )
        .await
        .unwrap();
        let client = bootstrap.checkout().await.unwrap();
        client
            .batch_execute("CREATE DATABASE nexql_switch_other")
            .await
            .unwrap();
    }

    let session = session_for(&pg.url).await;

    let client = session.checkout().await.unwrap();
    let row = client
        .query_one("SELECT current_database()", &[])
        .await
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "postgres");

    session
        .switch("default", Some("nexql_switch_other".into()))
        .await
        .unwrap();

    let client = session.checkout().await.unwrap();
    let row = client
        .query_one("SELECT current_database()", &[])
        .await
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "nexql_switch_other");

    session
        .switch("default", Some("postgres".into()))
        .await
        .unwrap();

    let client = session.checkout().await.unwrap();
    let row = client
        .query_one("SELECT current_database()", &[])
        .await
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "postgres");
}

#[tokio::test]
async fn search_all_databases_indexes_other_databases_on_first_use() {
    let Some(pg) = start_temp_pg() else {
        eprintln!("skip: initdb/postgres unavailable");
        return;
    };
    {
        let (client, conn) = tokio_postgres::connect(&pg.url, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(conn);
        for db in ["nexql_analytics", "nexql_empty"] {
            client
                .batch_execute(&format!("CREATE DATABASE {db}"))
                .await
                .unwrap();
        }
        let other = pg.url.replacen("/postgres", "/nexql_analytics", 1);
        let (client, conn) = tokio_postgres::connect(&other, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(conn);
        client
            .batch_execute(
                "CREATE SCHEMA dw; \
                 CREATE TABLE dw.fact_web_traffic (day date, visits int); \
                 INSERT INTO dw.fact_web_traffic VALUES ('2026-10-01', 7)",
            )
            .await
            .unwrap();
    }

    let session = session_for(&pg.url).await;
    let index_dir = TempDir::new().unwrap();
    let router = nexql_tools::ToolRouter::with_index_store(
        session,
        Some(nexql_index::IndexStore::new(index_dir.path())),
    );

    let out = router
        .call(
            "search_all_databases",
            serde_json::json!({ "query": "web traffic" }),
        )
        .await;
    assert!(!out.is_error, "{}", out.text);
    let body = serde_json::to_string(&out.structured).unwrap();
    assert!(body.contains("dw.fact_web_traffic"), "{body}");
    assert!(body.contains("indexed_now"), "{body}");
    assert!(body.contains("no tables or views"), "{body}");

    // `database` alone targets the active connection.
    let out = router
        .call(
            "run_select",
            serde_json::json!({
                "sql": "SELECT sum(visits) AS v FROM dw.fact_web_traffic",
                "database": "nexql_analytics",
            }),
        )
        .await;
    assert!(!out.is_error, "{}", out.text);
    assert!(out.text.contains('7'), "{}", out.text);

    let out = router.call("list_databases", serde_json::json!({})).await;
    assert!(!out.is_error, "{}", out.text);
    assert!(out.text.contains("nexql_analytics"), "{}", out.text);
}

#[tokio::test]
async fn orient_lists_enum_like_columns_from_planner_stats() {
    let Some(pg) = start_temp_pg() else {
        eprintln!("skip: initdb/postgres unavailable");
        return;
    };
    {
        let (client, conn) = tokio_postgres::connect(&pg.url, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(conn);
        client
            .batch_execute(
                "CREATE TABLE public.tickets (id int PRIMARY KEY, status text, \
                     contact_email text, notes text, created_by text); \
                 INSERT INTO public.tickets \
                     SELECT g, (ARRAY['OPEN','CLOSED','PENDING'])[1 + g % 3], \
                            (ARRAY['a@example.com','b@example.com'])[1 + g % 2], \
                            (ARRAY['Called back','Left voicemail'])[1 + g % 2], \
                            (ARRAY['ana','raj'])[1 + g % 2] \
                     FROM generate_series(1, 300) AS g; \
                 ANALYZE public.tickets;",
            )
            .await
            .unwrap();
    }

    let session = session_for(&pg.url).await;
    let index_dir = TempDir::new().unwrap();
    let router = nexql_tools::ToolRouter::with_index_store(
        session,
        Some(nexql_index::IndexStore::new(index_dir.path())),
    );
    let out = router.call("orient", serde_json::json!({})).await;
    assert!(!out.is_error, "{}", out.text);
    let enums = &out.structured.unwrap()["enums"];
    let mut status: Vec<&str> = enums["public.tickets.status"]
        .as_array()
        .unwrap_or_else(|| panic!("status missing: {enums}"))
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    status.sort();
    assert_eq!(status, ["CLOSED", "OPEN", "PENDING"]);
    // Masked, free-text and actor columns are never listed.
    for column in ["contact_email", "notes", "created_by"] {
        assert!(
            enums.get(format!("public.tickets.{column}")).is_none(),
            "{column} listed: {enums}"
        );
    }
}
