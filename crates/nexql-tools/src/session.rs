//! Session state: resolved profiles + active pool + optional schema index.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use deadpool_postgres::{Object, Pool};
use nexql_conn::{
    ConnectionParams, PoolOptions, ProfileConfig, ResolvedConnection, checkout_guarded,
    create_pool,
};
use nexql_index::IndexStore;
use nexql_policy::{AccessMode, PolicyCaps, PolicyFilter};
use tokio::sync::RwLock;

use crate::error::ToolError;

/// Index root: `NEXQL_MCP_INDEX_DIR`, else `~/.local/share/nexql-mcp` (same as CLI).
pub fn default_index_root() -> PathBuf {
    if let Ok(p) = std::env::var("NEXQL_MCP_INDEX_DIR") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/nexql-mcp")
}

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub id: String,
    pub name: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub params: ConnectionParams,
}

pub struct ToolSession {
    pub connections: Vec<ConnectionInfo>,
    pub access_mode: AccessMode,
    pub caps: PolicyCaps,
    pub filter: PolicyFilter,
    pub pool_opts: PoolOptions,
    /// Schema index root; `None` disables Phase 3 tools with an actionable error.
    pub index_store: Option<IndexStore>,
    inner: RwLock<SessionInner>,
}

fn filter_from_profile(profile: &ProfileConfig) -> PolicyFilter {
    PolicyFilter {
        allow_schemas: profile.schemas.clone(),
        deny_schemas: profile.deny_schemas.clone(),
        deny_tables: profile.deny_tables.clone(),
        pii_columns: profile.pii_columns.clone(),
    }
}

struct SessionInner {
    active_id: String,
    database: String,
    pools: HashMap<String, Pool>,
}

impl ToolSession {
    pub async fn from_resolved(
        resolved: ResolvedConnection,
        access_mode: AccessMode,
        caps: PolicyCaps,
    ) -> Result<Arc<Self>, ToolError> {
        let id = resolved
            .profile_name
            .clone()
            .unwrap_or_else(|| "default".into());
        let filter = resolved
            .profile
            .as_ref()
            .map(filter_from_profile)
            .unwrap_or_default();
        let info = ConnectionInfo {
            id: id.clone(),
            name: id.clone(),
            host: resolved.params.host.clone(),
            port: resolved.params.port,
            database: resolved.params.dbname.clone(),
            params: resolved.params,
        };
        Self::from_connections_with_filter(vec![info], access_mode, caps, Some(id), filter).await
    }

    pub async fn from_connections(
        connections: Vec<ConnectionInfo>,
        access_mode: AccessMode,
        caps: PolicyCaps,
        active_id: Option<String>,
    ) -> Result<Arc<Self>, ToolError> {
        Self::from_connections_with_filter(
            connections,
            access_mode,
            caps,
            active_id,
            PolicyFilter::default(),
        )
        .await
    }

    pub async fn from_connections_with_filter(
        connections: Vec<ConnectionInfo>,
        access_mode: AccessMode,
        caps: PolicyCaps,
        active_id: Option<String>,
        filter: PolicyFilter,
    ) -> Result<Arc<Self>, ToolError> {
        if connections.is_empty() {
            return Err(ToolError::Execution("no connections configured".into()));
        }
        let pool_opts = PoolOptions {
            read_only: !access_mode.allows_writes(),
            ..Default::default()
        };
        let active = active_id.unwrap_or_else(|| connections[0].id.clone());
        let mut pools = HashMap::new();
        for c in &connections {
            let pool = create_pool(&c.params, &pool_opts).await?;
            pools.insert(c.id.clone(), pool);
        }
        let database = connections
            .iter()
            .find(|c| c.id == active)
            .and_then(|c| c.database.clone())
            .unwrap_or_else(|| "postgres".into());
        Ok(Arc::new(Self {
            connections,
            access_mode,
            caps,
            filter,
            pool_opts,
            index_store: Some(IndexStore::new(default_index_root())),
            inner: RwLock::new(SessionInner {
                active_id: active,
                database,
                pools,
            }),
        }))
    }

    pub async fn active_context(&self) -> (String, String) {
        let g = self.inner.read().await;
        (g.active_id.clone(), g.database.clone())
    }

    pub async fn switch(
        &self,
        connection_id: &str,
        database: Option<String>,
    ) -> Result<(), ToolError> {
        let conn = self
            .connections
            .iter()
            .find(|c| c.id == connection_id)
            .ok_or_else(|| {
                ToolError::Execution(format!(
                    "Connection not found for ID: {connection_id} — call list_connections"
                ))
            })?;
        let mut g = self.inner.write().await;
        if !g.pools.contains_key(connection_id) {
            let pool = create_pool(&conn.params, &self.pool_opts).await?;
            g.pools.insert(connection_id.to_string(), pool);
        }
        g.active_id = connection_id.to_string();
        g.database = database
            .or_else(|| conn.database.clone())
            .unwrap_or_else(|| "postgres".into());
        Ok(())
    }

    pub async fn checkout(&self) -> Result<Object, ToolError> {
        let g = self.inner.read().await;
        let pool = g
            .pools
            .get(&g.active_id)
            .ok_or_else(|| ToolError::Execution("no active pool".into()))?;
        Ok(checkout_guarded(pool, &self.pool_opts).await?)
    }

    /// Test helper: session with no live pools (index tools only).
    #[cfg(test)]
    pub fn for_tests(
        connections: Vec<ConnectionInfo>,
        filter: PolicyFilter,
        index_store: Option<IndexStore>,
    ) -> Arc<Self> {
        assert!(!connections.is_empty());
        let active = connections[0].id.clone();
        let database = connections[0]
            .database
            .clone()
            .unwrap_or_else(|| "postgres".into());
        Arc::new(Self {
            connections,
            access_mode: AccessMode::Read,
            caps: PolicyCaps::default(),
            filter,
            pool_opts: PoolOptions::default(),
            index_store,
            inner: RwLock::new(SessionInner {
                active_id: active,
                database,
                pools: HashMap::new(),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_maps_profile_fields() {
        let profile = ProfileConfig {
            schemas: vec!["public".into()],
            deny_schemas: vec!["pgboss".into()],
            deny_tables: vec!["auth.*".into()],
            pii_columns: vec!["public.users.ssn".into()],
            ..Default::default()
        };
        let f = filter_from_profile(&profile);
        assert_eq!(f.allow_schemas, vec!["public"]);
        assert_eq!(f.deny_schemas, vec!["pgboss"]);
        assert_eq!(f.deny_tables, vec!["auth.*"]);
        assert_eq!(f.pii_columns, vec!["public.users.ssn"]);
        assert!(f.allows_schema("public"));
        assert!(!f.allows_schema("pgboss"));
        assert!(!f.allows_table("auth", "sessions"));
    }
}
