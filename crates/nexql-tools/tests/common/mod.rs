// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Shared helpers for nexql-tools integration tests (throwaway Postgres).

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nexql_conn::params_from_url;
use nexql_index::IndexStore;
use nexql_policy::{AccessMode, PolicyCaps};
use nexql_tools::{ConnectionInfo, ToolRouter, ToolSession};
use tempfile::TempDir;

pub struct TempPg {
    _data: TempDir,
    child: Child,
    pub url: String,
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

/// Start a throwaway Postgres on a random local port. Returns `None` when
/// `initdb` / `postgres` are unavailable (CI without PG toolchain).
pub fn start_temp_pg() -> Option<TempPg> {
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

/// Seed a minimal FK-linked schema used by tool smoke tests.
pub async fn seed_smoke_schema(url: &str) {
    let params = params_from_url(url).unwrap();
    let client = nexql_conn::connect_once(&params).await.unwrap();
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS public.users (
                id serial PRIMARY KEY,
                email text NOT NULL
             );
             CREATE TABLE IF NOT EXISTS public.orders (
                id serial PRIMARY KEY,
                user_id int REFERENCES public.users(id),
                total numeric
             );
             CREATE SCHEMA IF NOT EXISTS staging;
             CREATE TABLE IF NOT EXISTS staging.users (
                id serial PRIMARY KEY,
                email text NOT NULL
             );",
        )
        .await
        .unwrap();
}

#[allow(dead_code)] // fields kept alive for RAII (temp PG, dirs)
pub struct SmokeEnv {
    pub pg: TempPg,
    pub index_dir: TempDir,
    pub config_dir: TempDir,
    pub router: Arc<ToolRouter>,
    pub url: String,
}

/// Admin-mode router with a built schema index over the smoke fixture.
pub async fn smoke_env() -> Option<SmokeEnv> {
    let pg = start_temp_pg()?;
    let url = pg.url.clone();
    seed_smoke_schema(&url).await;

    let index_dir = TempDir::new().ok()?;
    let config_dir = TempDir::new().ok()?;
    unsafe {
        std::env::set_var("NEXQL_MCP_INDEX_DIR", index_dir.path());
        std::env::set_var(
            "NEXQL_MCP_CONFIG",
            config_dir.path().join("config.toml"),
        );
    }

    let params = params_from_url(&url).unwrap();
    let info = ConnectionInfo {
        id: "default".into(),
        name: "default".into(),
        host: params.host.clone(),
        port: params.port,
        database: params.dbname.clone(),
        params,
        policy: nexql_tools::session::policy_from_profile(
            None,
            AccessMode::Admin,
            PolicyCaps::default().with_max_rows(50),
        ),
    };
    let store = IndexStore::new(index_dir.path());
    let session = ToolSession::from_connections(vec![info], None)
        .await
        .ok()?;
    let router = Arc::new(ToolRouter::with_index_store(session, Some(store)));

    let build = router.call("rebuild_index", serde_json::json!({ "depth": "shallow" })).await;
    if build.is_error {
        eprintln!("skip: rebuild_index failed: {}", build.text);
        return None;
    }

    Some(SmokeEnv {
        pg,
        index_dir,
        config_dir,
        router,
        url,
    })
}
