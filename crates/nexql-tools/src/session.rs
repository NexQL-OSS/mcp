//! Session state: resolved profiles + active pool + optional schema index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use deadpool_postgres::{Object, Pool};
use nexql_conn::{
    ConnectionParams, PoolOptions, ProfileConfig, ResolvedConnection, checkout_guarded, create_pool,
};
use nexql_index::IndexStore;
use nexql_policy::{AccessMode, PolicyCaps, PolicyFilter};
use tokio::sync::RwLock as AsyncRwLock;

use crate::error::ToolError;

/// Index root: `NEXQL_MCP_INDEX_DIR`, else `~/.local/share/nexql-mcp` (same as CLI).
pub fn default_index_root() -> PathBuf {
    if let Ok(p) = std::env::var("NEXQL_MCP_INDEX_DIR") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/nexql-mcp")
}

/// Determine index root with workspace override support.
/// Priority: explicit env var → project config index_dir → global default.
pub fn resolve_index_root(
    workspace_root: Option<&Path>,
    project_config: Option<&nexql_conn::ProjectConfigFile>,
) -> PathBuf {
    if let Ok(p) = std::env::var("NEXQL_MCP_INDEX_DIR") {
        return PathBuf::from(p);
    }
    if let (Some(root), Some(cfg)) = (workspace_root, project_config) {
        if let Some(ref index_dir) = cfg.index_dir {
            return root.join(".nexql").join(index_dir);
        }
    }
    default_index_root()
}

#[derive(Debug, Clone)]
pub struct ConnectionPolicy {
    pub access_mode: AccessMode,
    pub caps: PolicyCaps,
    pub filter: PolicyFilter,
    pub environment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub id: String,
    pub name: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub params: ConnectionParams,
    pub policy: ConnectionPolicy,
}

#[derive(Debug, Clone)]
struct ActivePolicy {
    access_mode: AccessMode,
    caps: PolicyCaps,
    filter: PolicyFilter,
    pool_opts: PoolOptions,
}

pub struct ToolSession {
    pub connections: Vec<ConnectionInfo>,
    policy: RwLock<ActivePolicy>,
    /// Schema index root; `None` disables Phase 3 tools with an actionable error.
    pub index_store: Option<IndexStore>,
    inner: AsyncRwLock<SessionInner>,
}

fn filter_from_profile(profile: &ProfileConfig) -> PolicyFilter {
    PolicyFilter {
        allow_schemas: profile.schemas.clone(),
        deny_schemas: profile.deny_schemas.clone(),
        deny_tables: profile.deny_tables.clone(),
        pii_columns: profile.pii_columns.clone(),
    }
}

pub fn policy_from_profile(
    profile: Option<&ProfileConfig>,
    default_mode: AccessMode,
    default_caps: PolicyCaps,
) -> ConnectionPolicy {
    let access_mode = profile
        .and_then(|p| p.access_mode.as_deref())
        .and_then(|m| m.parse::<AccessMode>().ok())
        .unwrap_or(default_mode);
    let mut caps = default_caps;
    if let Some(n) = profile.and_then(|p| p.max_rows) {
        caps = caps.with_max_rows(n);
    }
    let filter = profile.map(filter_from_profile).unwrap_or_default();
    ConnectionPolicy {
        access_mode,
        caps,
        filter,
        environment: None,
    }
}

struct SessionInner {
    active_id: String,
    database: String,
    pools: HashMap<String, Pool>,
}

fn pool_key(connection_id: &str, database: &str) -> String {
    format!("{connection_id}\0{database}")
}

fn params_for_database(base: &ConnectionParams, database: &str) -> ConnectionParams {
    let mut params = base.clone();
    params.dbname = Some(database.to_string());
    // `to_url()` prefers `url` over `dbname`; drop stale URL path when host fields exist.
    if params.host.is_some() {
        params.url = None;
    }
    params
}

fn active_policy_from(policy: &ConnectionPolicy) -> ActivePolicy {
    ActivePolicy {
        access_mode: policy.access_mode,
        caps: policy.caps.clone(),
        filter: policy.filter.clone(),
        pool_opts: PoolOptions {
            read_only: !policy.access_mode.allows_writes(),
            ..Default::default()
        },
    }
}

impl ToolSession {
    pub fn access_mode(&self) -> AccessMode {
        self.policy
            .read()
            .map(|p| p.access_mode)
            .unwrap_or(AccessMode::Read)
    }

    pub fn caps(&self) -> PolicyCaps {
        self.policy
            .read()
            .map(|p| p.caps.clone())
            .unwrap_or_default()
    }

    pub fn filter(&self) -> PolicyFilter {
        self.policy
            .read()
            .map(|p| p.filter.clone())
            .unwrap_or_default()
    }

    pub fn pool_opts(&self) -> PoolOptions {
        self.policy
            .read()
            .map(|p| p.pool_opts.clone())
            .unwrap_or_default()
    }

    fn apply_policy(&self, policy: &ConnectionPolicy) {
        if let Ok(mut active) = self.policy.write() {
            *active = active_policy_from(policy);
        }
    }

    pub async fn from_resolved(
        resolved: ResolvedConnection,
        access_mode: AccessMode,
        caps: PolicyCaps,
    ) -> Result<Arc<Self>, ToolError> {
        let id = resolved
            .profile_name
            .clone()
            .unwrap_or_else(|| "default".into());
        let conn_policy = policy_from_profile(resolved.profile.as_ref(), access_mode, caps);
        let info = ConnectionInfo {
            id: id.clone(),
            name: id.clone(),
            host: resolved.params.host.clone(),
            port: resolved.params.port,
            database: resolved.params.dbname.clone(),
            params: resolved.params,
            policy: conn_policy,
        };
        Self::from_connections(vec![info], Some(id)).await
    }

    pub async fn from_connections(
        connections: Vec<ConnectionInfo>,
        active_id: Option<String>,
    ) -> Result<Arc<Self>, ToolError> {
        if connections.is_empty() {
            return Err(ToolError::Execution("no connections configured".into()));
        }
        let active = active_id.unwrap_or_else(|| connections[0].id.clone());
        let active_conn = connections
            .iter()
            .find(|c| c.id == active)
            .unwrap_or(&connections[0]);
        let active_policy = active_policy_from(&active_conn.policy);
        let pool_opts = active_policy.pool_opts.clone();
        let mut pools = HashMap::new();
        for c in &connections {
            let database = c.database.as_deref().unwrap_or("postgres");
            let pool = create_pool(&c.params, &pool_opts).await?;
            pools.insert(pool_key(&c.id, database), pool);
        }
        let database = active_conn
            .database
            .clone()
            .unwrap_or_else(|| "postgres".into());
        Ok(Arc::new(Self {
            connections,
            policy: RwLock::new(active_policy),
            index_store: Some(IndexStore::new(default_index_root())),
            inner: AsyncRwLock::new(SessionInner {
                active_id: active,
                database,
                pools,
            }),
        }))
    }

    pub async fn from_connections_with_filter(
        connections: Vec<ConnectionInfo>,
        access_mode: AccessMode,
        caps: PolicyCaps,
        active_id: Option<String>,
        filter: PolicyFilter,
    ) -> Result<Arc<Self>, ToolError> {
        let connections = connections
            .into_iter()
            .map(|mut c| {
                c.policy.access_mode = access_mode;
                c.policy.caps = caps.clone();
                c.policy.filter = filter.clone();
                c
            })
            .collect();
        Self::from_connections(connections, active_id).await
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
        let target_db = database
            .or_else(|| conn.database.clone())
            .unwrap_or_else(|| "postgres".into());
        self.apply_policy(&conn.policy);
        let pool_opts = self.pool_opts();
        let key = pool_key(connection_id, &target_db);
        let mut g = self.inner.write().await;
        if let std::collections::hash_map::Entry::Vacant(e) = g.pools.entry(key) {
            let params = params_for_database(&conn.params, &target_db);
            let pool = create_pool(&params, &pool_opts).await?;
            e.insert(pool);
        }
        g.active_id = connection_id.to_string();
        g.database = target_db;
        Ok(())
    }

    pub async fn checkout(&self) -> Result<Object, ToolError> {
        let g = self.inner.read().await;
        let key = pool_key(&g.active_id, &g.database);
        let pool = g
            .pools
            .get(&key)
            .ok_or_else(|| ToolError::Execution("no active pool".into()))?;
        let pool_opts = self.pool_opts();
        Ok(checkout_guarded(pool, &pool_opts).await?)
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
        let policy = ConnectionPolicy {
            access_mode: AccessMode::Read,
            caps: PolicyCaps::default(),
            filter,
            environment: None,
        };
        Arc::new(Self {
            connections,
            policy: RwLock::new(active_policy_from(&policy)),
            index_store,
            inner: AsyncRwLock::new(SessionInner {
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
