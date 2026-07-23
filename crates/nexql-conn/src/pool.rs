//! Connection pool with read-only + statement_timeout session guards.

use std::str::FromStr;
use std::time::Duration;

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};
use rustls::ClientConfig;
use tokio_postgres::{Client, Config, NoTls};

use crate::error::ConnError;
use crate::resolve::ConnectionParams;

#[derive(Debug, Clone)]
pub struct PoolOptions {
    pub max_connections: usize,
    pub statement_timeout: Duration,
    pub read_only: bool,
}

impl Default for PoolOptions {
    fn default() -> Self {
        Self {
            max_connections: 5,
            statement_timeout: Duration::from_secs(30),
            read_only: true,
        }
    }
}

/// Create a deadpool (NoTls). For `sslmode=require`, use [`connect_once`].
pub async fn create_pool(params: &ConnectionParams, opts: &PoolOptions) -> Result<Pool, ConnError> {
    let url = params.to_url()?;
    let pg_config = Config::from_str(&url).map_err(|e| ConnError::Pool(e.to_string()))?;
    if matches!(
        pg_config.get_ssl_mode(),
        tokio_postgres::config::SslMode::Require
    ) {
        return Err(ConnError::Pool(
            "pooled connections do not support sslmode=require yet — use connect_once".into(),
        ));
    }
    let mgr_config = ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    };
    let manager = Manager::from_config(pg_config, NoTls, mgr_config);
    Pool::builder(manager)
        .max_size(opts.max_connections)
        .runtime(Runtime::Tokio1)
        .build()
        .map_err(|e| ConnError::Pool(e.to_string()))
}

/// Apply session guards on a checked-out client. Fail closed.
pub async fn apply_session_guards(client: &Client, opts: &PoolOptions) -> Result<(), ConnError> {
    if opts.read_only {
        client
            .batch_execute("SET default_transaction_read_only = ON")
            .await?;
    }
    let timeout_ms = opts.statement_timeout.as_millis();
    client
        .batch_execute(&format!("SET statement_timeout = '{timeout_ms}ms'"))
        .await?;
    Ok(())
}

/// Convenience: checkout + apply guards.
pub async fn checkout_guarded(
    pool: &Pool,
    opts: &PoolOptions,
) -> Result<deadpool_postgres::Object, ConnError> {
    let client = pool
        .get()
        .await
        .map_err(|e| ConnError::Pool(e.to_string()))?;
    apply_session_guards(&client, opts).await?;
    Ok(client)
}

/// One-shot connect (supports rustls when `sslmode=require`).
pub async fn connect_once(params: &ConnectionParams) -> Result<Client, ConnError> {
    let url = params.to_url()?;
    let config = Config::from_str(&url).map_err(|e| ConnError::Pool(e.to_string()))?;
    let force_tls = matches!(
        config.get_ssl_mode(),
        tokio_postgres::config::SslMode::Require
    );
    if force_tls {
        let mut root_store = rustls::RootCertStore::empty();
        let native = rustls_native_certs::load_native_certs();
        for cert in native.certs {
            let _ = root_store.add(cert);
        }
        let tls_config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        let tls = tokio_postgres_rustls::MakeRustlsConnect::new(tls_config);
        let (client, conn) = config.connect(tls).await?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(client)
    } else {
        let (client, conn) = config.connect(NoTls).await?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::params_from_url;
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};
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

    #[tokio::test]
    async fn session_guards_set_read_only() {
        let Some(pg) = start_temp_pg() else {
            eprintln!("skip: initdb/postgres unavailable");
            return;
        };
        let params = params_from_url(&pg.url).unwrap();
        let opts = PoolOptions {
            max_connections: 2,
            ..Default::default()
        };
        let pool = create_pool(&params, &opts).await.expect("pool");
        let client = checkout_guarded(&pool, &opts).await.expect("checkout");
        let row = client
            .query_one("SHOW default_transaction_read_only", &[])
            .await
            .unwrap();
        let v: String = row.get(0);
        assert_eq!(v, "on");
        let row = client
            .query_one("SHOW statement_timeout", &[])
            .await
            .unwrap();
        let t: String = row.get(0);
        assert!(
            t.contains("30") || t == "30s" || t == "30000ms",
            "timeout={t}"
        );
    }

    #[tokio::test]
    async fn pool_respects_max_connections() {
        let Some(pg) = start_temp_pg() else {
            eprintln!("skip: initdb/postgres unavailable");
            return;
        };
        let params = params_from_url(&pg.url).unwrap();
        let opts = PoolOptions {
            max_connections: 2,
            ..Default::default()
        };
        let pool = create_pool(&params, &opts).await.unwrap();
        let _a = checkout_guarded(&pool, &opts).await.unwrap();
        let _b = checkout_guarded(&pool, &opts).await.unwrap();
        assert_eq!(pool.status().max_size, 2);
        assert_eq!(pool.status().size, 2);
    }
}
