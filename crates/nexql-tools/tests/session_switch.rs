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
