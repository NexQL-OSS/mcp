//! Postgres connect + seed helpers for the Phase 0 spike.

use std::str::FromStr;

use anyhow::{Context, Result, bail};
use rustls::ClientConfig;
use tokio_postgres::{Client, Config, NoTls};

use crate::catalog::{COLUMNS_QUERY, CONSTRAINTS_QUERY, RELATIONS_QUERY};

/// Connect using a libpq URL. Uses rustls when sslmode=require (Neon-style).
pub async fn connect_url(url: &str) -> Result<Client> {
    let config = Config::from_str(url).context("parse connection URL")?;
    let force_tls = matches!(
        config.get_ssl_mode(),
        tokio_postgres::config::SslMode::Require
    );

    if force_tls {
        connect_rustls(config).await
    } else {
        let (client, conn) = config.connect(NoTls).await.context("connect NoTls")?;
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                eprintln!("postgres connection error: {e}");
            }
        });
        Ok(client)
    }
}

/// Resolve URL from arg / `NEXQL_MCP_SPIKE_DATABASE_URL` / `DATABASE_URL`.
pub async fn connect(url: Option<&str>) -> Result<Client> {
    let resolved = url
        .map(str::to_owned)
        .or_else(|| std::env::var("NEXQL_MCP_SPIKE_DATABASE_URL").ok())
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .context(
            "no database URL — set NEXQL_MCP_SPIKE_DATABASE_URL or DATABASE_URL, \
             or pass an explicit URL",
        )?;
    connect_url(&resolved).await
}

async fn connect_rustls(config: Config) -> Result<Client> {
    let mut root_store = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        let _ = root_store.add(cert);
    }
    if !native.errors.is_empty() {
        eprintln!(
            "warning: some native certs failed to load: {:?}",
            native.errors
        );
    }

    let tls_config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let tls = tokio_postgres_rustls::MakeRustlsConnect::new(tls_config);
    let (client, conn) = config.connect(tls).await.context("connect rustls")?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("postgres connection error: {e}");
        }
    });
    Ok(client)
}

/// Seed `public.users` / `public.orders` with an FK — Phase 0 catalog smoke fixture.
pub async fn seed_users_orders(client: &Client) -> Result<()> {
    client
        .batch_execute(
            r#"
            DROP TABLE IF EXISTS public.orders;
            DROP TABLE IF EXISTS public.users;
            CREATE TABLE public.users (
              id   serial PRIMARY KEY,
              email text NOT NULL,
              name  text
            );
            CREATE TABLE public.orders (
              id      serial PRIMARY KEY,
              user_id integer NOT NULL REFERENCES public.users(id),
              amount  numeric(12,2) NOT NULL
            );
            INSERT INTO public.users (email, name) VALUES ('a@example.com', 'Ada');
            INSERT INTO public.orders (user_id, amount) VALUES (1, 42.50);
            "#,
        )
        .await
        .context("seed users/orders")?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct CatalogSample {
    pub relation_count: usize,
    pub column_count: usize,
    pub fk_count: usize,
}

/// Run the three catalog queries and assert non-empty results on a seeded DB.
pub async fn sample_catalog(client: &Client) -> Result<CatalogSample> {
    let schemas: Vec<&str> = vec!["public"];
    let relations = client
        .query(RELATIONS_QUERY, &[&schemas])
        .await
        .context("RELATIONS_QUERY")?;
    if relations.is_empty() {
        bail!("RELATIONS_QUERY returned 0 rows");
    }

    let oids: Vec<u32> = relations
        .iter()
        .map(|r| {
            let oid: i32 = r.get("oid");
            u32::try_from(oid).expect("oid fits u32")
        })
        .collect();
    let columns = client
        .query(COLUMNS_QUERY, &[&oids])
        .await
        .context("COLUMNS_QUERY")?;
    if columns.is_empty() {
        bail!("COLUMNS_QUERY returned 0 rows");
    }

    let constraints = client
        .query(CONSTRAINTS_QUERY, &[&oids])
        .await
        .context("CONSTRAINTS_QUERY")?;

    let fk_count = constraints
        .iter()
        .filter(|r| r.get::<_, i8>("type") == i8::try_from(b'f').expect("ascii"))
        .count();
    if fk_count == 0 {
        bail!("CONSTRAINTS_QUERY returned no foreign keys");
    }

    Ok(CatalogSample {
        relation_count: relations.len(),
        column_count: columns.len(),
        fk_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn select_version_when_url_set() {
        let Ok(url) = std::env::var("NEXQL_MCP_SPIKE_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
        else {
            eprintln!("skip: no DATABASE_URL / NEXQL_MCP_SPIKE_DATABASE_URL");
            return;
        };
        let client = connect_url(&url).await.expect("connect");
        let row = client
            .query_one("SELECT version()", &[])
            .await
            .expect("version()");
        let version: String = row.get(0);
        assert!(version.contains("PostgreSQL"), "got: {version}");
    }
}
