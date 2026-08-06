//! Integration: Phase 2 catalog tools against a throwaway Postgres.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nexql_conn::{connect_once, params_from_url};
use nexql_policy::{AccessMode, PolicyCaps};
use nexql_tools::{ConnectionInfo, ToolRouter, ToolSession};
use serde_json::json;
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

async fn router_for(url: &str) -> Arc<ToolRouter> {
    let params = params_from_url(url).unwrap();

    // Seed via a one-shot connection — the tool session below is read-only
    // (`AccessMode::Read` sets `default_transaction_read_only = ON` on every
    // pooled checkout), so DDL must land before that guard is in effect.
    connect_once(&params)
        .await
        .unwrap()
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS public.users (id serial PRIMARY KEY, email text);
             CREATE TABLE IF NOT EXISTS public.orders (
               id serial PRIMARY KEY,
               user_id int REFERENCES public.users(id)
             );",
        )
        .await
        .unwrap();

    let info = ConnectionInfo {
        id: "default".into(),
        name: "default".into(),
        host: params.host.clone(),
        port: params.port,
        database: params.dbname.clone(),
        params,
    };
    let session = ToolSession::from_connections(
        vec![info],
        AccessMode::Read,
        PolicyCaps::default().with_max_rows(10),
        None,
    )
    .await
    .unwrap();
    Arc::new(ToolRouter::new(session))
}

#[tokio::test]
async fn phase2_catalog_tools_smoke() {
    let Some(pg) = start_temp_pg() else {
        eprintln!("skip: initdb/postgres unavailable");
        return;
    };
    let router = router_for(&pg.url).await;
    assert_eq!(router.specs().len(), 45);

    let ctx = router.call("get_current_context", json!({})).await;
    assert!(!ctx.is_error, "{}", ctx.text);

    let schemas = router.call("list_schemas", json!({})).await;
    assert!(!schemas.is_error, "{}", schemas.text);
    assert!(schemas.text.contains("public"));

    let objects = router
        .call("list_objects", json!({"schema": "public", "kind": "table"}))
        .await;
    assert!(!objects.is_error, "{}", objects.text);
    assert!(objects.text.contains("users"));

    let select = router
        .call("run_select", json!({"sql": "SELECT 1 AS n"}))
        .await;
    assert!(!select.is_error, "{}", select.text);

    let dml = router
        .call("run_select", json!({"sql": "DELETE FROM public.users"}))
        .await;
    assert!(dml.is_error, "DML must fail: {}", dml.text);

    let explain = router
        .call("explain_query", json!({"sql": "SELECT 1"}))
        .await;
    assert!(!explain.is_error, "{}", explain.text);

    let stacked = router
        .call(
            "run_select",
            json!({"sql": "SELECT 1; DROP TABLE public.users"}),
        )
        .await;
    assert!(stacked.is_error, "stacked must fail: {}", stacked.text);
}
