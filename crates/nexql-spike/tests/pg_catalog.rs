//! Integration tests requiring a live Postgres.
//!
//! Spins up a throwaway cluster via `initdb` when no URL is set (needs `initdb`/`postgres`
//! on PATH). Set `NEXQL_MCP_SPIKE_DATABASE_URL` to reuse an existing server instead.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use nexql_spike::pg::{connect_url, sample_catalog, seed_users_orders};
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

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
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
    let port = free_port();

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
        .env("LC_ALL", "C.UTF-8")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let url = format!("postgres://spike@127.0.0.1:{port}/postgres?sslmode=disable");
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut ready = false;
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            // give postmaster a beat after accept
            std::thread::sleep(Duration::from_millis(400));
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !ready {
        return None;
    }

    Some(TempPg {
        _data: data,
        child,
        url,
    })
}

async fn resolve_url() -> Option<(String, Option<TempPg>)> {
    if let Ok(url) =
        std::env::var("NEXQL_MCP_SPIKE_DATABASE_URL").or_else(|_| std::env::var("DATABASE_URL"))
    {
        return Some((url, None));
    }
    let pg = start_temp_pg()?;
    let url = pg.url.clone();
    Some((url, Some(pg)))
}

#[tokio::test]
async fn connect_and_version() {
    let Some((url, _guard)) = resolve_url().await else {
        eprintln!("skip: no DATABASE_URL and initdb/postgres unavailable");
        return;
    };
    let client = connect_url(&url).await.expect("connect");
    let row = client
        .query_one("SELECT version()", &[])
        .await
        .expect("version");
    let version: String = row.get(0);
    assert!(version.contains("PostgreSQL"), "{version}");
}

#[tokio::test]
async fn catalog_queries_on_seeded_schema() {
    let Some((url, _guard)) = resolve_url().await else {
        eprintln!("skip: no DATABASE_URL and initdb/postgres unavailable");
        return;
    };
    let client = connect_url(&url).await.expect("connect");
    seed_users_orders(&client).await.expect("seed");
    let sample = sample_catalog(&client).await.expect("catalog");
    assert!(sample.relation_count >= 2, "{sample:?}");
    assert!(sample.column_count >= 1, "{sample:?}");
    assert!(sample.fk_count >= 1, "{sample:?}");
}
