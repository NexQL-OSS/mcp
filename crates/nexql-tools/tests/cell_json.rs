//! Integration: `cell_to_json` / `rows_to_json` against typed Postgres columns.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use nexql_conn::params_from_url;
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

async fn router_for(url: &str) -> ToolRouter {
    let params = params_from_url(url).unwrap();
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
    ToolRouter::new(session)
}

fn row_value<'a>(out: &'a nexql_tools::ToolOutcome, column: &str) -> &'a serde_json::Value {
    let structured = out.structured.as_ref().expect("structured content");
    let rows = structured
        .get("rows")
        .and_then(|v| v.as_array())
        .expect("rows array");
    rows[0]
        .get(column)
        .unwrap_or_else(|| panic!("column {column}"))
}

#[tokio::test]
async fn cell_to_json_handles_postgres_types() {
    let Some(pg) = start_temp_pg() else {
        eprintln!("skip: initdb/postgres unavailable");
        return;
    };
    let router = router_for(&pg.url).await;

    let out = router
        .call(
            "run_select",
            json!({
                "sql": "SELECT \
                    now()::timestamptz AS ts_tz, \
                    now()::timestamp AS ts, \
                    current_date AS d, \
                    gen_random_uuid() AS uid, \
                    '{}'::jsonb AS jb, \
                    1.5::numeric AS num, \
                    ARRAY[1, 2] AS arr, \
                    true AS flag"
            }),
        )
        .await;
    assert!(!out.is_error, "{}", out.text);

    assert!(
        !row_value(&out, "ts_tz").is_null(),
        "timestamptz should not be null: {}",
        out.text
    );
    assert!(
        row_value(&out, "ts_tz").is_string(),
        "timestamptz should be ISO string: {}",
        out.text
    );

    assert!(!row_value(&out, "ts").is_null(), "timestamp: {}", out.text);
    assert!(!row_value(&out, "d").is_null(), "date: {}", out.text);
    assert!(!row_value(&out, "uid").is_null(), "uuid: {}", out.text);
    assert!(
        row_value(&out, "uid").is_string(),
        "uuid should be string: {}",
        out.text
    );

    assert!(!row_value(&out, "jb").is_null(), "jsonb: {}", out.text);
    assert!(
        row_value(&out, "jb").is_object(),
        "jsonb should be object: {}",
        out.text
    );

    assert!(!row_value(&out, "num").is_null(), "numeric: {}", out.text);
    assert!(
        row_value(&out, "num").is_string(),
        "numeric should be text: {}",
        out.text
    );

    let arr = row_value(&out, "arr").as_array().expect("int array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0], json!(1));
    assert_eq!(arr[1], json!(2));

    assert_eq!(row_value(&out, "flag"), &json!(true));
}

#[tokio::test]
async fn explain_json_plan_is_not_empty() {
    let Some(pg) = start_temp_pg() else {
        eprintln!("skip: initdb/postgres unavailable");
        return;
    };
    let router = router_for(&pg.url).await;

    let out = router
        .call(
            "run_select",
            json!({ "sql": "EXPLAIN (FORMAT JSON) SELECT 1 AS n" }),
        )
        .await;
    assert!(!out.is_error, "{}", out.text);

    let plan = row_value(&out, "QUERY PLAN");
    assert!(
        !plan.is_null(),
        "EXPLAIN JSON plan must not be null: {}",
        out.text
    );
    assert!(
        plan.is_array() || plan.is_object(),
        "EXPLAIN JSON plan should be structured JSON: {}",
        out.text
    );
}
