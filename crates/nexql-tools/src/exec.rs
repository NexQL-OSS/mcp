//! Tool dispatch for catalog (Phase 2) + index (Phase 3) surfaces.

use std::sync::Arc;

use nexql_index::{IndexQueryService, IndexStore, QueryPolicyFilter};
use nexql_policy::{PolicyFilter, SqlDecision, validate_readonly_sql};
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::registry::ToolName;
use crate::schema::{ToolSpec, active_tools};
use crate::session::ToolSession;

/// Default hit cap for `search_schema` (matches TS ToolExecutor).
const SEARCH_SCHEMA_LIMIT: usize = 10;

const NO_INDEX_HINT: &str =
    "No schema index configured — set NEXQL_MCP_INDEX_DIR or run `nexql-mcp index build`.";

#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub text: String,
    pub structured: Option<Value>,
    pub is_error: bool,
}

impl ToolOutcome {
    pub fn ok_json(value: Value) -> Self {
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        Self {
            text,
            structured: Some(value),
            is_error: false,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        let message = msg.into();
        Self {
            text: message.clone(),
            structured: Some(json!({ "error": message })),
            is_error: true,
        }
    }
}

pub struct ToolRouter {
    session: Arc<ToolSession>,
    /// Optional override; when `None`, uses `session.index_store`.
    index_override: Option<Option<IndexStore>>,
    specs: Vec<ToolSpec>,
}

impl ToolRouter {
    pub fn new(session: Arc<ToolSession>) -> Self {
        Self {
            session,
            index_override: None,
            specs: active_tools(),
        }
    }

    /// Build with an explicit index store (or `None` to force the no-index error path).
    pub fn with_index_store(session: Arc<ToolSession>, store: Option<IndexStore>) -> Self {
        Self {
            session,
            index_override: Some(store),
            specs: active_tools(),
        }
    }

    pub fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn index_store(&self) -> Option<&IndexStore> {
        match &self.index_override {
            Some(inner) => inner.as_ref(),
            None => self.session.index_store.as_ref(),
        }
    }

    fn query_filter(&self) -> QueryPolicyFilter {
        policy_to_query_filter(&self.session.filter)
    }

    pub async fn call(&self, name: &str, args: Value) -> ToolOutcome {
        match self.call_inner(name, args).await {
            Ok(out) => out,
            Err(e) => ToolOutcome::err(e.to_string()),
        }
    }

    async fn call_inner(&self, name: &str, args: Value) -> Result<ToolOutcome, ToolError> {
        let tool = ToolName::parse(name).ok_or_else(|| ToolError::Unknown(name.to_string()))?;
        match tool {
            ToolName::ListConnections => Ok(self.list_connections()),
            ToolName::ListDatabases => self.list_databases(&args).await,
            ToolName::ListSchemas => self.list_schemas().await,
            ToolName::ListObjects => self.list_objects(&args).await,
            ToolName::GetCurrentContext => self.get_current_context().await,
            ToolName::SwitchConnection => self.switch_connection(&args).await,
            ToolName::RunSelect => self.run_select(&args).await,
            ToolName::ExplainQuery => self.explain_query(&args).await,
            ToolName::SearchSchema => self.search_schema(&args).await,
            ToolName::DescribeObject => self.describe_object(&args).await,
            ToolName::GetJoinPath => self.get_join_path(&args).await,
            ToolName::SampleValues => self.sample_values(&args).await,
            _ => Err(ToolError::Unknown(format!(
                "{name} is not available until a later phase"
            ))),
        }
    }

    async fn index_service(&self) -> Result<(&IndexStore, String, String), ToolError> {
        let store = self
            .index_store()
            .ok_or_else(|| ToolError::Execution(NO_INDEX_HINT.into()))?;
        let (connection_id, database) = self.session.active_context().await;
        let base = store.base_dir(&connection_id, &database);
        if store.read_manifest(&base)?.is_none() {
            return Err(ToolError::Execution(format!(
                "No schema index for database \"{database}\" — run `nexql-mcp index build`."
            )));
        }
        Ok((store, connection_id, database))
    }

    async fn search_schema(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if query.is_empty() {
            return Ok(ToolOutcome::ok_json(json!([])));
        }
        let (store, connection_id, database) = self.index_service().await?;
        let svc = IndexQueryService::new(store, &connection_id, &database);
        let filter = self.query_filter();
        let hits = svc.search_schema(query, SEARCH_SCHEMA_LIMIT, Some(&filter))?;
        let rows: Vec<Value> = hits
            .into_iter()
            .map(|h| {
                json!({
                    "ref": h.ref_,
                    "score": h.score,
                    "kind": h.kind,
                })
            })
            .collect();
        Ok(ToolOutcome::ok_json(json!(rows)))
    }

    async fn describe_object(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ref_ = args
            .get("ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ref is required".into()))?;
        let (store, connection_id, database) = self.index_service().await?;
        let svc = IndexQueryService::new(store, &connection_id, &database);
        let filter = self.query_filter();
        let entry = svc.describe_object(ref_, Some(&filter))?;
        let value = serde_json::to_value(entry).map_err(|e| ToolError::Execution(e.to_string()))?;
        Ok(ToolOutcome::ok_json(value))
    }

    async fn get_join_path(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let a = args
            .get("a")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("a is required".into()))?;
        let b = args
            .get("b")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("b is required".into()))?;
        let (store, connection_id, database) = self.index_service().await?;
        let svc = IndexQueryService::new(store, &connection_id, &database);
        let path = svc.get_join_path(a, b)?;
        let value = serde_json::to_value(path).map_err(|e| ToolError::Execution(e.to_string()))?;
        Ok(ToolOutcome::ok_json(value))
    }

    async fn sample_values(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ref_ = args
            .get("ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ref is required".into()))?;
        let col = args
            .get("col")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("col is required".into()))?;
        let (store, connection_id, database) = self.index_service().await?;
        let svc = IndexQueryService::new(store, &connection_id, &database);
        let filter = self.query_filter();
        // Index-only this phase — live DB sampling stays Phase 4+.
        let result = svc.sample_values(ref_, col, Some(&filter), None)?;
        let mut payload = json!({ "values": result.values });
        if let Some(message) = result.message {
            payload["message"] = json!(message);
        }
        Ok(ToolOutcome::ok_json(payload))
    }

    fn list_connections(&self) -> ToolOutcome {
        let rows: Vec<Value> = self
            .session
            .connections
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "name": c.name,
                    "host": c.host,
                    "port": c.port,
                    "database": c.database,
                })
            })
            .collect();
        ToolOutcome::ok_json(json!(rows))
    }

    async fn list_databases(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let connection_id = args
            .get("connectionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("connectionId is required".into()))?;
        let conn = self
            .session
            .connections
            .iter()
            .find(|c| c.id == connection_id)
            .ok_or_else(|| {
                ToolError::Execution(format!(
                    "Connection not found for ID: {connection_id} — call list_connections"
                ))
            })?;
        // Connect using that profile's params (may differ from active).
        let client = {
            // Temporarily use active checkout if same id; else one-shot.
            if self.session.active_context().await.0 == connection_id {
                self.session.checkout().await?
            } else {
                let pool = nexql_conn::create_pool(&conn.params, &self.session.pool_opts).await?;
                nexql_conn::checkout_guarded(&pool, &self.session.pool_opts).await?
            }
        };
        let rows = client
            .query(
                "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
                &[],
            )
            .await?;
        let names: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
        Ok(ToolOutcome::ok_json(json!(names)))
    }

    async fn list_schemas(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let rows = client
            .query(
                r#"
                SELECT nspname AS schema_name
                FROM pg_namespace
                WHERE nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
                  AND nspname NOT LIKE 'pg_%'
                ORDER BY nspname
                "#,
                &[],
            )
            .await?;
        let out: Vec<Value> = rows
            .iter()
            .filter(|r| {
                let name: String = r.get(0);
                self.session.filter.allows_schema(&name)
            })
            .map(|r| json!({ "schema_name": r.get::<_, String>(0) }))
            .collect();
        Ok(ToolOutcome::ok_json(json!(out)))
    }

    async fn list_objects(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let schema = args
            .get("schema")
            .and_then(|v| v.as_str())
            .unwrap_or("public");
        if !schema
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ToolError::InvalidArgs(
                "Invalid or missing schema name format".into(),
            ));
        }
        if !self.session.filter.allows_schema(schema) {
            return Ok(ToolOutcome::ok_json(json!([])));
        }
        let kind = args.get("kind").and_then(|v| v.as_str());
        let mut queries = Vec::new();
        let push_rel = |queries: &mut Vec<String>, relkinds: &[&str], label: &str| {
            let kinds = relkinds
                .iter()
                .map(|k| format!("'{k}'"))
                .collect::<Vec<_>>()
                .join(",");
            queries.push(format!(
                r#"
                SELECT n.nspname AS schema, c.relname AS name, '{label}' AS kind,
                       d.description AS comment
                FROM pg_class c
                JOIN pg_namespace n ON n.oid = c.relnamespace
                LEFT JOIN pg_description d ON d.objoid = c.oid AND d.objsubid = 0
                WHERE n.nspname = $1 AND c.relkind IN ({kinds})
                "#
            ));
        };
        if kind.is_none() || kind == Some("table") {
            push_rel(&mut queries, &["r", "f", "p"], "table");
        }
        if kind.is_none() || kind == Some("view") {
            push_rel(&mut queries, &["v"], "view");
        }
        if kind.is_none() || kind == Some("matview") {
            push_rel(&mut queries, &["m"], "matview");
        }
        if queries.is_empty() {
            return Ok(ToolOutcome::ok_json(json!([])));
        }
        let sql = queries.join("\nUNION ALL\n") + "\nORDER BY kind, name";
        let client = self.session.checkout().await?;
        let rows = client.query(&sql, &[&schema]).await?;
        let out: Vec<Value> = rows
            .iter()
            .filter(|r| {
                let s: String = r.get("schema");
                let name: String = r.get("name");
                self.session.filter.allows_table(&s, &name)
            })
            .map(|r| {
                json!({
                    "schema": r.get::<_, String>("schema"),
                    "name": r.get::<_, String>("name"),
                    "kind": r.get::<_, String>("kind"),
                    "comment": r.get::<_, Option<String>>("comment"),
                })
            })
            .collect();
        Ok(ToolOutcome::ok_json(json!(out)))
    }

    async fn get_current_context(&self) -> Result<ToolOutcome, ToolError> {
        let (connection_id, database) = self.session.active_context().await;
        let conn = self
            .session
            .connections
            .iter()
            .find(|c| c.id == connection_id);
        Ok(ToolOutcome::ok_json(json!({
            "connectionId": connection_id,
            "connectionName": conn.map(|c| c.name.as_str()).unwrap_or("Unknown"),
            "database": database,
            "host": conn.and_then(|c| c.host.clone()),
            "port": conn.and_then(|c| c.port),
            "access_mode": match self.session.access_mode {
                nexql_policy::AccessMode::Read => "read",
                nexql_policy::AccessMode::Write => "write",
                nexql_policy::AccessMode::Admin => "admin",
            },
        })))
    }

    async fn switch_connection(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let connection_id = args
            .get("connectionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("connectionId is required".into()))?;
        let database = args
            .get("database")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        self.session.switch(connection_id, database).await?;
        self.get_current_context().await
    }

    async fn run_select(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        match validate_readonly_sql(sql)? {
            SqlDecision::Allow => {}
            SqlDecision::Reject => {
                return Err(ToolError::Execution(
                    "Security Error: Only read-only SELECT, WITH, or EXPLAIN statements are permitted."
                        .into(),
                ));
            }
        }
        let trimmed = sql.trim().to_ascii_lowercase();
        if trimmed.starts_with("explain") {
            return self.run_select_internal(sql, None).await;
        }
        let max_rows = self.session.caps.max_rows;
        self.run_select_internal(sql, Some(max_rows)).await
    }

    async fn explain_query(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        match validate_readonly_sql(sql)? {
            SqlDecision::Allow => {}
            SqlDecision::Reject => {
                return Err(ToolError::Execution(
                    "Security Error: Only SELECT, WITH, or EXPLAIN statements can be analyzed."
                        .into(),
                ));
            }
        }
        let clean = if sql.trim().to_ascii_lowercase().starts_with("explain") {
            sql.to_string()
        } else {
            format!("EXPLAIN {sql}")
        };
        // Re-validate EXPLAIN wrapper
        if validate_readonly_sql(&clean)? == SqlDecision::Reject {
            return Err(ToolError::Execution(
                "Security Error: EXPLAIN target is not read-only.".into(),
            ));
        }
        self.run_select_internal(&clean, None).await
    }

    async fn run_select_internal(
        &self,
        sql: &str,
        max_rows: Option<u32>,
    ) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let Some(max_rows) = max_rows else {
            let rows = client.query(sql, &[]).await?;
            let values = rows_to_json(&rows);
            let text = serde_json::to_string_pretty(&values)
                .map_err(|e| ToolError::Execution(e.to_string()))?;
            let (trunc, text) = self.session.caps.truncate_chars(&text);
            let structured = if trunc {
                json!({ "truncated_chars": true, "rows": values })
            } else {
                values
            };
            return Ok(ToolOutcome {
                text: text.to_string(),
                structured: Some(structured),
                is_error: false,
            });
        };

        let cleaned = sql.trim().trim_end_matches(';').trim();
        let wrapped = format!(
            "SELECT * FROM ({cleaned}) AS nexql_limited LIMIT {}",
            max_rows + 1
        );
        let rows = match client.query(&wrapped, &[]).await {
            Ok(r) => r,
            Err(_) => client.query(sql, &[]).await?,
        };
        let truncated = rows.len() as u32 > max_rows;
        let keep = if truncated {
            &rows[..max_rows as usize]
        } else {
            &rows[..]
        };
        let values = rows_to_json(keep);
        let payload = if truncated {
            json!({ "rows": values, "truncated": true, "maxRows": max_rows })
        } else {
            values
        };
        let text = serde_json::to_string_pretty(&payload)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let (char_trunc, text) = self.session.caps.truncate_chars(&text);
        let structured = if char_trunc {
            json!({ "truncated_chars": true, "data": payload })
        } else {
            payload
        };
        Ok(ToolOutcome {
            text: text.to_string(),
            structured: Some(structured),
            is_error: false,
        })
    }
}

fn policy_to_query_filter(filter: &PolicyFilter) -> QueryPolicyFilter {
    QueryPolicyFilter {
        allow_schemas: filter.allow_schemas.clone(),
        deny_schemas: filter.deny_schemas.clone(),
        deny_tables: filter.deny_tables.clone(),
        pii_columns: filter.pii_columns.clone(),
    }
}

fn rows_to_json(rows: &[tokio_postgres::Row]) -> Value {
    let arr: Vec<Value> = rows
        .iter()
        .map(|row| {
            let mut map = serde_json::Map::new();
            for (i, col) in row.columns().iter().enumerate() {
                map.insert(col.name().to_string(), cell_to_json(row, i));
            }
            Value::Object(map)
        })
        .collect();
    Value::Array(arr)
}

fn cell_to_json(row: &tokio_postgres::Row, idx: usize) -> Value {
    if let Ok(v) = row.try_get::<_, Option<String>>(idx) {
        return match v {
            Some(s) => Value::String(s),
            None => Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<_, Option<i32>>(idx) {
        return match v {
            Some(n) => json!(n),
            None => Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<_, Option<i64>>(idx) {
        return match v {
            Some(n) => json!(n),
            None => Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<_, Option<bool>>(idx) {
        return match v {
            Some(b) => json!(b),
            None => Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<_, Option<f64>>(idx) {
        return match v {
            Some(n) => json!(n),
            None => Value::Null,
        };
    }
    Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexql_policy::PolicyFilter;
    use serde_json::json;

    use crate::session::{ConnectionInfo, ToolSession};

    fn test_conn() -> ConnectionInfo {
        ConnectionInfo {
            id: "conn-1".into(),
            name: "conn-1".into(),
            host: Some("127.0.0.1".into()),
            port: Some(5432),
            database: Some("appdb".into()),
            params: Default::default(),
        }
    }

    #[test]
    fn policy_maps_one_to_one() {
        let f = PolicyFilter {
            allow_schemas: vec!["public".into()],
            deny_schemas: vec!["pgboss".into()],
            deny_tables: vec!["auth.*".into()],
            pii_columns: vec!["public.users.ssn".into()],
        };
        let q = policy_to_query_filter(&f);
        assert_eq!(q.allow_schemas, f.allow_schemas);
        assert_eq!(q.deny_schemas, f.deny_schemas);
        assert_eq!(q.deny_tables, f.deny_tables);
        assert_eq!(q.pii_columns, f.pii_columns);
    }

    #[test]
    fn router_specs_include_phase3() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        assert_eq!(router.specs().len(), 12);
        let names: Vec<_> = router.specs().iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"search_schema"));
        assert!(names.contains(&"describe_object"));
        assert!(names.contains(&"get_join_path"));
        assert!(names.contains(&"sample_values"));
    }

    #[tokio::test]
    async fn missing_index_returns_actionable_error() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        let out = router
            .call("search_schema", json!({ "query": "users" }))
            .await;
        assert!(out.is_error, "{}", out.text);
        assert!(
            out.text.contains("nexql-mcp index build"),
            "expected actionable hint, got: {}",
            out.text
        );
    }

    #[tokio::test]
    async fn empty_index_dir_returns_build_hint() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = IndexStore::new(tmp.path());
        let session = ToolSession::for_tests(
            vec![test_conn()],
            PolicyFilter::default(),
            Some(IndexStore::new(tmp.path())),
        );
        let router = ToolRouter::with_index_store(session, Some(store));
        let out = router
            .call("describe_object", json!({ "ref": "public.users" }))
            .await;
        assert!(out.is_error, "{}", out.text);
        assert!(
            out.text.contains("nexql-mcp index build"),
            "expected build hint, got: {}",
            out.text
        );
    }
}
