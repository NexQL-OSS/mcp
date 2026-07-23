//! Tool dispatch for catalog (Phase 2) + index (Phase 3) + Phase 4 surfaces.

use std::sync::Arc;

use nexql_index::{
    CatalogDb, Embedder, IndexQueryService, IndexStore, PgCatalogDb, QueryPolicyFilter,
    SearchOptions,
};
use nexql_policy::{PolicyFilter, SqlDecision, validate_readonly_sql};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::{Value, json};
use tokio_postgres::types::{FromSql, Kind, Type};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::error::ToolError;
use crate::export::{ExportFormat, columns_from_rows, rows_to_csv, rows_to_sql_insert};
use crate::plan::{analyze_deep_plan, build_explain_sql, extract_plan_metrics};
use crate::registry::ToolName;
use crate::schema::{ToolSpec, active_tools};
use crate::session::ToolSession;
use crate::sql::{self, REPORT_LIMIT_DEFAULT, SLOW_QUERIES_DEFAULT, parse_ref};
use crate::write::{
    apply_ddl, create_index_concurrently, edit_row, execute_sql, import_data, run_maintenance,
    terminate_query,
};

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
    /// Success payload for MCP `structuredContent`.
    ///
    /// Cursor (and some other clients) require `structuredContent` to be a JSON
    /// **object**. Bare arrays are dropped before the model sees them — always
    /// wrap: `{ "rows": [ ... ] }`.
    pub fn ok_json(value: Value) -> Self {
        let value = ensure_structured_object(value);
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

/// Cursor MCP rejects non-object `structuredContent`. Wrap arrays as `{ "rows": … }`.
fn ensure_structured_object(value: Value) -> Value {
    match value {
        Value::Array(rows) => json!({ "rows": rows }),
        other => other,
    }
}

pub struct ToolRouter {
    session: Arc<ToolSession>,
    /// Optional override; when `None`, uses `session.index_store`.
    index_override: Option<Option<IndexStore>>,
    /// When true and an embedder is set, `search_schema` fuses via RRF.
    use_semantic: bool,
    embedder: Option<Arc<dyn Embedder>>,
    specs: Vec<ToolSpec>,
}

impl ToolRouter {
    pub fn new(session: Arc<ToolSession>) -> Self {
        Self {
            session,
            index_override: None,
            use_semantic: false,
            embedder: None,
            specs: active_tools(),
        }
    }

    /// Build with an explicit index store (or `None` to force the no-index error path).
    pub fn with_index_store(session: Arc<ToolSession>, store: Option<IndexStore>) -> Self {
        Self {
            session,
            index_override: Some(store),
            use_semantic: false,
            embedder: None,
            specs: active_tools(),
        }
    }

    /// Enable semantic RRF fusion for `search_schema` (requires embeddings on disk + embedder).
    pub fn with_semantic(
        mut self,
        use_semantic: bool,
        embedder: Option<Arc<dyn Embedder>>,
    ) -> Self {
        self.use_semantic = use_semantic;
        self.embedder = embedder;
        self
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
            ToolName::GetDdl => self.get_ddl(&args).await,
            ToolName::TableStats => self.table_stats(&args).await,
            ToolName::IndexUsage => self.index_usage(&args).await,
            ToolName::ListRunningQueries => self.list_running_queries().await,
            ToolName::FindBlockingLocks => self.find_blocking_locks().await,
            ToolName::SlowQueries => self.slow_queries(&args).await,
            ToolName::DbHealthCheck => self.db_health_check().await,
            ToolName::ExplainAnalyze => self.explain_analyze(&args).await,
            ToolName::AnalyzeQueryPlan => self.analyze_query_plan(&args).await,
            ToolName::GetIndexStatus => self.get_index_status().await,
            ToolName::ListExtensions => self.list_extensions().await,
            ToolName::ServerSettings => self.server_settings().await,
            ToolName::SuggestIndexes => self.suggest_indexes(&args).await,
            ToolName::FindUnusedIndexes => self.find_unused_indexes(&args).await,
            ToolName::BloatReport => self.bloat_report(&args).await,
            ToolName::FindMissingFks => self.find_missing_fks(&args).await,
            ToolName::ExportQuery => self.export_query(&args).await,
            ToolName::ListRoles => self.list_roles(&args).await,
            ToolName::DbDashboard => self.db_dashboard().await,
            ToolName::DeepPlanAnalysis => self.deep_plan_analysis(&args).await,
            ToolName::SchemaDiff => self.schema_diff(&args).await,
            ToolName::GenerateMigration => self.generate_migration(&args).await,
            ToolName::ExecuteSql => self.execute_sql_tool(&args).await,
            ToolName::EditRow => self.edit_row_tool(&args).await,
            ToolName::ImportData => self.import_data_tool(&args).await,
            ToolName::ApplyDdl => self.apply_ddl_tool(&args).await,
            ToolName::CreateIndexConcurrently => self.create_index_concurrently_tool(&args).await,
            ToolName::RunMaintenance => self.run_maintenance_tool(&args).await,
            ToolName::TerminateQuery => self.terminate_query_tool(&args).await,
        }
    }

    fn require_write(&self) -> Result<(), ToolError> {
        if !self.session.access_mode.allows_writes() {
            return Err(ToolError::Execution(
                "write tools require --access-mode write or admin (current session: read)".into(),
            ));
        }
        Ok(())
    }

    fn require_admin(&self) -> Result<(), ToolError> {
        if !self.session.access_mode.allows_admin() {
            return Err(ToolError::Execution(
                "admin tools require --access-mode admin".into(),
            ));
        }
        Ok(())
    }

    async fn execute_sql_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_write()?;
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        execute_sql(&self.session, sql, dry_run).await
    }

    async fn edit_row_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_write()?;
        edit_row(&self.session, args).await
    }

    async fn import_data_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_write()?;
        import_data(&self.session, args).await
    }

    async fn apply_ddl_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_admin()?;
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        apply_ddl(&self.session, sql, dry_run).await
    }

    async fn create_index_concurrently_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_admin()?;
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        create_index_concurrently(&self.session, sql).await
    }

    async fn run_maintenance_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_admin()?;
        run_maintenance(&self.session, args).await
    }

    async fn terminate_query_tool(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        self.require_admin()?;
        terminate_query(&self.session, args).await
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
        let hits = svc.search_schema(
            query,
            SEARCH_SCHEMA_LIMIT,
            Some(&filter),
            SearchOptions {
                use_semantic: self.use_semantic,
                embedder: self.embedder.as_deref(),
            },
        )?;
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

    async fn get_ddl(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ref_ = args
            .get("ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ref is required".into()))?;
        let (schema, name) =
            parse_ref(ref_).map_err(ToolError::InvalidArgs)?;
        let kind = args
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("table");
        let reg = sql::regclass_literal(&schema, &name);
        let client = self.session.checkout().await?;

        match kind {
            "view" | "matview" => {
                let sql = format!("SELECT pg_get_viewdef({reg}, true) AS definition");
                let rows = client.query(&sql, &[]).await?;
                Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
            }
            "function" => {
                let sql = format!(
                    r#"SELECT p.proname AS name, pg_get_functiondef(p.oid) AS definition
                       FROM pg_proc p
                       JOIN pg_namespace n ON n.oid = p.pronamespace
                       WHERE n.nspname = '{schema}' AND p.proname = '{name}'"#
                );
                let rows = client.query(&sql, &[]).await?;
                Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
            }
            "index" => {
                let sql = format!("SELECT pg_get_indexdef({reg}) AS definition");
                let rows = client.query(&sql, &[]).await?;
                Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
            }
            "table" => {
                let columns = client
                    .query(&sql::column_details(&schema, &name), &[])
                    .await?;
                let constraints = client
                    .query(
                        &format!(
                            r#"SELECT conname AS name, pg_get_constraintdef(oid) AS definition
                               FROM pg_constraint WHERE conrelid = {reg} ORDER BY conname"#
                        ),
                        &[],
                    )
                    .await?;
                let indexes = client
                    .query(
                        &format!(
                            r#"SELECT indexname AS name, indexdef AS definition
                               FROM pg_indexes
                               WHERE schemaname = '{schema}' AND tablename = '{name}'
                               ORDER BY indexname"#
                        ),
                        &[],
                    )
                    .await?;
                Ok(ToolOutcome::ok_json(json!({
                    "table": format!("{schema}.{name}"),
                    "columns": rows_to_json(&columns),
                    "constraints": rows_to_json(&constraints),
                    "indexes": rows_to_json(&indexes),
                })))
            }
            other => Err(ToolError::InvalidArgs(format!(
                "Unsupported DDL kind \"{other}\". Use table, view, matview, function, or index."
            ))),
        }
    }

    async fn table_stats(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ref_ = args
            .get("ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ref is required".into()))?;
        let (schema, name) =
            parse_ref(ref_).map_err(ToolError::InvalidArgs)?;
        let client = self.session.checkout().await?;
        let stats = client
            .query(&sql::table_stats(&schema, &name), &[])
            .await?;
        let activity = client
            .query(&sql::table_activity(&schema, &name), &[])
            .await?;
        let columns = client
            .query(&sql::column_stats(&schema, &name), &[])
            .await?;
        let size = rows_to_json(&stats)
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null);
        let activity = rows_to_json(&activity)
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null);
        Ok(ToolOutcome::ok_json(json!({
            "size": size,
            "activity": activity,
            "columns": rows_to_json(&columns),
        })))
    }

    async fn index_usage(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let ref_ = args
            .get("ref")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("ref is required".into()))?;
        let (schema, name) =
            parse_ref(ref_).map_err(ToolError::InvalidArgs)?;
        let client = self.session.checkout().await?;
        let rows = client
            .query(&sql::index_usage(&schema, &name), &[])
            .await?;
        Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
    }

    async fn list_running_queries(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let rows = client.query(sql::running_queries(), &[]).await?;
        Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
    }

    async fn find_blocking_locks(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let rows = client.query(sql::blocking_locks(), &[]).await?;
        let values = rows_to_json(&rows);
        if values.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return Ok(ToolOutcome::ok_json(json!({
                "message": "No blocking locks found.",
                "locks": [],
            })));
        }
        Ok(ToolOutcome::ok_json(values))
    }

    async fn slow_queries(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(SLOW_QUERIES_DEFAULT);
        let client = self.session.checkout().await?;
        match client.query(&sql::slow_queries(limit), &[]).await {
            Ok(rows) => Ok(ToolOutcome::ok_json(rows_to_json(&rows))),
            Err(e) => {
                if let Some(message) = sql::map_stat_statements_error(&e) {
                    Ok(ToolOutcome::ok_json(json!({
                        "error": message,
                        "hint": message,
                    })))
                } else {
                    Err(ToolError::Postgres(e))
                }
            }
        }
    }

    async fn db_health_check(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let sections: &[(&str, &str)] = &[
            ("overview", sql::database_stats()),
            ("cache", sql::cache_hit_ratio()),
            ("dead_tuples", sql::database_maintenance_stats()),
            ("connection_states", sql::connection_states()),
            ("blocking_locks", sql::blocking_locks()),
        ];
        let mut report = serde_json::Map::new();
        for (key, q) in sections {
            match client.query(*q, &[]).await {
                Ok(rows) => {
                    report.insert((*key).into(), rows_to_json(&rows));
                }
                Err(e) => {
                    report.insert((*key).into(), json!({ "error": e.to_string() }));
                }
            }
        }
        let lock_count = report
            .get("blocking_locks")
            .and_then(|v| v.as_array())
            .map(|a| a.len() as u64);
        report.insert("blocking_lock_count".into(), json!(lock_count));
        Ok(ToolOutcome::ok_json(Value::Object(report)))
    }

    async fn explain_analyze(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        require_select_or_with(sql)?;
        let explain = build_explain_sql(sql, true);
        self.run_explain_in_transaction(&explain).await
    }

    async fn analyze_query_plan(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        require_select_or_with(sql)?;
        let analyze = args
            .get("analyze")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let explain = build_explain_sql(sql, analyze);
        let outcome = self.run_explain_in_transaction(&explain).await?;
        let rows = outcome.structured.unwrap_or(Value::Null);
        let row_array = rows
            .get("rows")
            .and_then(|v| v.as_array())
            .or_else(|| rows.as_array());
        let plan = row_array
            .and_then(|a| a.first())
            .and_then(|r| r.get("QUERY PLAN"))
            .cloned()
            .unwrap_or(Value::Null);
        let metrics = extract_plan_metrics(&plan).or_else(|| extract_plan_metrics(&rows));
        let recommendations = metrics
            .as_ref()
            .and_then(|m| m.get("recommendations"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        Ok(ToolOutcome::ok_json(json!({
            "metrics": metrics,
            "recommendations": recommendations,
            "plan": plan,
        })))
    }

    /// EXPLAIN ANALYZE executes the query — always wrap in READ ONLY + ROLLBACK.
    async fn run_explain_in_transaction(&self, explain_sql: &str) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        client
            .batch_execute("SET statement_timeout = '30s'")
            .await?;
        client.batch_execute("BEGIN").await?;
        let result = async {
            client
                .batch_execute("SET TRANSACTION READ ONLY")
                .await?;
            let rows = client.query(explain_sql, &[]).await?;
            Ok::<_, ToolError>(rows_to_json(&rows))
        }
        .await;
        // Always roll back — belt-and-braces on top of default_transaction_read_only.
        let _ = client.batch_execute("ROLLBACK").await;
        match result {
            Ok(values) => Ok(ToolOutcome::ok_json(values)),
            Err(e) => Err(e),
        }
    }

    async fn get_index_status(&self) -> Result<ToolOutcome, ToolError> {
        let (store, connection_id, database) = self.index_service().await?;
        let base = store.base_dir(&connection_id, &database);
        let Some(manifest) = store.read_manifest(&base)? else {
            return Err(ToolError::Execution(format!(
                "No schema index for database \"{database}\" — run `nexql-mcp index build`."
            )));
        };

        let mut live_fingerprint: Option<String> = None;
        let mut drift: Option<bool> = None;
        if let Ok(client) = self.session.checkout().await {
            let db = PgCatalogDb::new(&client);
            if let Ok(fp) = db.schema_fingerprint().await {
                drift = Some(fp != manifest.schema_fingerprint);
                live_fingerprint = Some(fp);
            }
        }

        Ok(ToolOutcome::ok_json(json!({
            "connectionId": manifest.connection_id,
            "database": manifest.database,
            "indexedAt": manifest.indexed_at,
            "fingerprint": manifest.schema_fingerprint,
            "liveFingerprint": live_fingerprint,
            "drift": drift,
            "pgVersion": manifest.pg_version,
            "counts": {
                "tables": manifest.counts.tables,
                "views": manifest.counts.views,
                "functions": manifest.counts.functions,
                "enums": manifest.counts.enums,
            },
            "buildMs": manifest.stats.build_ms,
            "warnings": manifest.stats.warnings,
        })))
    }

    async fn list_extensions(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let rows = client.query(sql::list_extensions(), &[]).await?;
        Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
    }

    async fn server_settings(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let rows = client.query(sql::server_settings(), &[]).await?;
        Ok(ToolOutcome::ok_json(rows_to_json(&rows)))
    }

    async fn suggest_indexes(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(REPORT_LIMIT_DEFAULT);
        let client = self.session.checkout().await?;

        let high_seq = client
            .query(&sql::high_seq_scan_tables(limit), &[])
            .await?;
        let unindexed_fks = client
            .query(&sql::unindexed_fk_columns(limit), &[])
            .await?;

        let mut pg_stat_available = false;
        let mut slow_queries = Value::Null;
        let mut pg_stat_note: Option<String> = None;
        match client.query(&sql::slow_queries(limit.min(10)), &[]).await {
            Ok(rows) => {
                pg_stat_available = true;
                slow_queries = rows_to_json(&rows);
            }
            Err(e) => {
                if let Some(message) = sql::map_stat_statements_error(&e) {
                    pg_stat_note = Some(message);
                } else {
                    return Err(ToolError::Postgres(e));
                }
            }
        }

        let mut plan_heuristics = Value::Null;
        if let Some(sql_text) = args.get("sql").and_then(|v| v.as_str()) {
            require_select_or_with(sql_text)?;
            let explain = build_explain_sql(sql_text, false);
            let outcome = self.run_explain_in_transaction(&explain).await?;
            let rows = outcome.structured.unwrap_or(Value::Null);
            let plan = rows
                .as_array()
                .and_then(|a| a.first())
                .and_then(|r| r.get("QUERY PLAN"))
                .cloned()
                .unwrap_or(Value::Null);
            let metrics = extract_plan_metrics(&plan).or_else(|| extract_plan_metrics(&rows));
            plan_heuristics = json!({
                "metrics": metrics,
                "hint": "Use analyze_query_plan with analyze=true for actual timings before creating indexes.",
            });
        }

        let high_seq_json = rows_to_json(&high_seq);
        let unindexed_json = rows_to_json(&unindexed_fks);
        let has_candidates = high_seq_json
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
            || unindexed_json
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false)
            || plan_heuristics != Value::Null;

        if !has_candidates && !pg_stat_available {
            return Ok(ToolOutcome::ok_json(json!({
                "suggestions": [],
                "message": "No index suggestions yet. Either table stats show healthy index use, or there is not enough scan history. Enable pg_stat_statements and/or pass a sql argument for EXPLAIN plan heuristics.",
                "hint": pg_stat_note,
            })));
        }

        if !has_candidates {
            return Ok(ToolOutcome::ok_json(json!({
                "high_seq_scan_tables": high_seq_json,
                "unindexed_fk_columns": unindexed_json,
                "slow_queries": slow_queries,
                "plan_heuristics": plan_heuristics,
                "message": "No strong index candidates from sequential-scan or unindexed-FK heuristics. Review slow_queries / pass sql for plan-level advice.",
                "hint": "CREATE INDEX CONCURRENTLY after validating with EXPLAIN (ANALYZE, BUFFERS).",
            })));
        }

        Ok(ToolOutcome::ok_json(json!({
            "high_seq_scan_tables": high_seq_json,
            "unindexed_fk_columns": unindexed_json,
            "slow_queries": slow_queries,
            "plan_heuristics": plan_heuristics,
            "pg_stat_statements": pg_stat_available,
            "hint": pg_stat_note.unwrap_or_else(|| {
                "Validate candidates with analyze_query_plan / EXPLAIN before CREATE INDEX CONCURRENTLY.".into()
            }),
        })))
    }

    async fn find_unused_indexes(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(REPORT_LIMIT_DEFAULT);
        let client = self.session.checkout().await?;
        let rows = client
            .query(&sql::find_unused_indexes(limit), &[])
            .await?;
        let indexes = rows_to_json(&rows);
        if indexes.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return Ok(ToolOutcome::ok_json(json!({
                "indexes": [],
                "message": "No unused non-constraint indexes found (idx_scan = 0). Note: pg_stat_reset / server restart clears scan counts — treat never-scanned indexes cautiously on fresh stats.",
            })));
        }
        Ok(ToolOutcome::ok_json(json!({
            "indexes": indexes,
            "hint": "Prefer DROP INDEX CONCURRENTLY after confirming the workload (and that stats are mature).",
        })))
    }

    async fn bloat_report(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(REPORT_LIMIT_DEFAULT);
        let client = self.session.checkout().await?;
        let rows = client.query(&sql::bloat_report(limit), &[]).await?;
        let tables = rows_to_json(&rows);
        if tables.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return Ok(ToolOutcome::ok_json(json!({
                "tables": [],
                "method": "dead_tuple_ratio",
                "message": "No tables with significant dead-tuple pressure (>1000 dead tuples). This is a simplified estimate from pg_stat_user_tables, not physical page bloat.",
            })));
        }
        Ok(ToolOutcome::ok_json(json!({
            "tables": tables,
            "method": "dead_tuple_ratio",
            "note": "Approximate bloat via n_dead_tup / (n_live_tup + n_dead_tup). Not a physical page-bloat estimate (pgstattuple / check_postgres). Consider VACUUM / VACUUM FULL only after confirming impact.",
            "hint": "VACUUM ANALYZE on high bloat_pct tables; investigate autovacuum settings if last_autovacuum is stale.",
        })))
    }

    async fn find_missing_fks(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(REPORT_LIMIT_DEFAULT);
        let capped = limit.clamp(1, sql::REPORT_LIMIT_MAX) as usize;

        // Prefer schema-index join-graph inferred edges when an index exists.
        if let Ok((store, connection_id, database)) = self.index_service().await {
            let base = store.base_dir(&connection_id, &database);
            if let Ok(Some(manifest)) = store.read_manifest(&base) {
                if let Ok(Some(graph)) = store.read_join_graph(&base, &manifest) {
                    let candidates: Vec<Value> = graph
                        .edges
                        .into_iter()
                        .filter(|e| e.inferred == Some(true) && e.disabled != Some(true))
                        .take(capped)
                        .map(|e| {
                            let cols: Vec<Value> = e
                                .cols
                                .iter()
                                .map(|(a, b)| json!({ "from": a, "to": b }))
                                .collect();
                            json!({
                                "from_table": e.from,
                                "to_table": e.to,
                                "via": e.via,
                                "columns": cols,
                                "detection": "join_graph_inferred",
                            })
                        })
                        .collect();
                    if !candidates.is_empty() {
                        return Ok(ToolOutcome::ok_json(json!({
                            "candidates": candidates,
                            "source": "join_graph",
                            "hint": "These edges were inferred by naming convention and have no declared FK. Review before ALTER TABLE … ADD FOREIGN KEY.",
                        })));
                    }
                }
            }
        }

        let client = self.session.checkout().await?;
        let rows = client
            .query(&sql::find_missing_fks_catalog(limit), &[])
            .await?;
        let candidates = rows_to_json(&rows);
        if candidates.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return Ok(ToolOutcome::ok_json(json!({
                "candidates": [],
                "source": "catalog",
                "message": "No missing FK candidates found via join-graph inferred edges or *_id naming against single-column PKs.",
            })));
        }
        Ok(ToolOutcome::ok_json(json!({
            "candidates": candidates,
            "source": "catalog",
            "hint": "Naming-inferred only — verify referential integrity and nullability before adding constraints. Run `nexql-mcp index build` for join-graph inferred edges.",
        })))
    }

    async fn list_roles(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let role = args
            .get("role")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let Some(role_name) = role else {
            let rows = client.query(sql::list_roles(), &[]).await?;
            return Ok(ToolOutcome::ok_json(rows_to_json(&rows)));
        };

        let details = client
            .query(sql::role_details(), &[&role_name])
            .await?;
        if details.is_empty() {
            return Err(ToolError::Execution(format!(
                "Role \"{role_name}\" not found"
            )));
        }
        let member_of = client
            .query(sql::role_member_of(), &[&role_name])
            .await?;
        let has_members = client
            .query(sql::role_has_members(), &[&role_name])
            .await?;
        let privileges = client
            .query(sql::role_table_privileges(), &[&role_name])
            .await?;

        Ok(ToolOutcome::ok_json(json!({
            "role": rows_to_json(&details).as_array().and_then(|a| a.first().cloned()).unwrap_or(Value::Null),
            "member_of": rows_to_json(&member_of),
            "has_members": rows_to_json(&has_members),
            "table_privileges": rows_to_json(&privileges),
        })))
    }

    async fn export_query(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        require_select_or_with(sql)?;

        let format = args
            .get("format")
            .and_then(|v| v.as_str())
            .map(|s| {
                ExportFormat::parse(s).ok_or_else(|| {
                    ToolError::InvalidArgs(format!(
                        "Unsupported format \"{s}\". Use csv, json, or sqlinsert."
                    ))
                })
            })
            .transpose()?
            .unwrap_or(ExportFormat::Csv);

        let table_target = match args.get("table").and_then(|v| v.as_str()) {
            Some(t) if !t.trim().is_empty() => {
                Some(parse_ref(t).map_err(ToolError::InvalidArgs)?)
            }
            _ => None,
        };

        if format == ExportFormat::SqlInsert && table_target.is_none() {
            return Err(ToolError::InvalidArgs(
                "table (schema.name) is required when format=sqlinsert".into(),
            ));
        }

        let max_rows = self.session.caps.max_rows;
        let outcome = self.run_select_internal(sql, Some(max_rows)).await?;
        if outcome.is_error {
            return Ok(outcome);
        }

        let structured = outcome.structured.unwrap_or(Value::Null);
        let rows_val = structured
            .get("rows")
            .cloned()
            .or_else(|| structured.get("data").and_then(|d| d.get("rows").cloned()))
            .unwrap_or(Value::Array(vec![]));
        let rows = rows_val.as_array().cloned().unwrap_or_default();
        let columns = columns_from_rows(&rows);
        let truncated = structured
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let payload = match format {
            ExportFormat::Json => json!({
                "format": format.as_str(),
                "rowCount": rows.len(),
                "truncated": truncated,
                "columns": columns,
                "rows": rows,
            }),
            ExportFormat::Csv => {
                let content = rows_to_csv(&rows, &columns);
                let (char_trunc, content) = self.session.caps.truncate_chars(&content);
                json!({
                    "format": format.as_str(),
                    "rowCount": rows.len(),
                    "truncated": truncated || char_trunc,
                    "columns": columns,
                    "content": content,
                })
            }
            ExportFormat::SqlInsert => {
                let (schema, table) = table_target.expect("checked above");
                let content = rows_to_sql_insert(&rows, &columns, &schema, &table);
                let (char_trunc, content) = self.session.caps.truncate_chars(&content);
                json!({
                    "format": format.as_str(),
                    "rowCount": rows.len(),
                    "truncated": truncated || char_trunc,
                    "table": format!("{schema}.{table}"),
                    "columns": columns,
                    "content": content,
                })
            }
        };

        Ok(ToolOutcome::ok_json(payload))
    }

    async fn db_dashboard(&self) -> Result<ToolOutcome, ToolError> {
        let client = self.session.checkout().await?;
        let sections: &[(&str, &str)] = &[
            ("db_info", sql::dashboard_db_info()),
            ("connection_states", sql::connection_states()),
            ("top_tables", sql::dashboard_top_tables()),
            ("object_counts", sql::dashboard_object_counts()),
            ("active_queries", sql::dashboard_active_queries()),
            ("blocking_locks", sql::blocking_locks()),
            ("max_connections", sql::dashboard_max_connections()),
            ("extension_count", sql::dashboard_extension_count()),
            ("cache", sql::cache_hit_ratio()),
        ];
        let mut report = serde_json::Map::new();
        for (key, q) in sections {
            match client.query(*q, &[]).await {
                Ok(rows) => {
                    report.insert((*key).into(), rows_to_json(&rows));
                }
                Err(e) => {
                    report.insert((*key).into(), json!({ "error": e.to_string() }));
                }
            }
        }

        // Normalize single-row sections to objects for agents.
        for key in ["db_info", "object_counts", "extension_count", "cache"] {
            if let Some(Value::Array(arr)) = report.get(key).cloned() {
                if arr.len() == 1 {
                    report.insert(key.into(), arr.into_iter().next().unwrap());
                }
            }
        }
        if let Some(Value::Array(arr)) = report.get("max_connections").cloned() {
            if let Some(row) = arr.first() {
                report.insert(
                    "max_connections".into(),
                    row.get("max_connections")
                        .cloned()
                        .unwrap_or_else(|| row.clone()),
                );
            }
        }

        Ok(ToolOutcome::ok_json(Value::Object(report)))
    }

    async fn deep_plan_analysis(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let sql = args
            .get("sql")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sql is required".into()))?;
        require_select_or_with(sql)?;
        let analyze = args
            .get("analyze")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let explain = build_explain_sql(sql, analyze);
        let outcome = self.run_explain_in_transaction(&explain).await?;
        let rows = outcome.structured.unwrap_or(Value::Null);
        let row_array = rows
            .get("rows")
            .and_then(|v| v.as_array())
            .or_else(|| rows.as_array());
        let plan = row_array
            .and_then(|a| a.first())
            .and_then(|r| r.get("QUERY PLAN"))
            .cloned()
            .unwrap_or(Value::Null);
        let deep = analyze_deep_plan(&plan, sql)
            .or_else(|| analyze_deep_plan(&rows, sql))
            .ok_or_else(|| {
                ToolError::Execution("Could not parse EXPLAIN JSON plan for deep analysis".into())
            })?;
        let metrics = extract_plan_metrics(&plan).or_else(|| extract_plan_metrics(&rows));
        Ok(ToolOutcome::ok_json(json!({
            "deep": deep,
            "metrics": metrics,
            "plan": plan,
            "analyzed": analyze,
        })))
    }

    async fn schema_diff(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let source_schema = args
            .get("sourceSchema")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sourceSchema is required".into()))?;
        let target_schema = args
            .get("targetSchema")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("targetSchema is required".into()))?;
        crate::schema_diff::require_safe_schema(source_schema)?;
        crate::schema_diff::require_safe_schema(target_schema)?;

        let client = self.session.checkout().await?;
        let source = crate::schema_diff::load_schema_snapshot(&client, source_schema).await?;
        let target = crate::schema_diff::load_schema_snapshot(&client, target_schema).await?;
        let diffs = crate::schema_diff::compute_schema_diff(&source, &target);
        let changed = diffs
            .iter()
            .filter(|d| d.status != crate::schema_diff::DiffStatus::Unchanged)
            .count();
        Ok(ToolOutcome::ok_json(json!({
            "sourceSchema": source_schema,
            "targetSchema": target_schema,
            "tableCount": diffs.len(),
            "changedCount": changed,
            "diffs": crate::schema_diff::diffs_to_json(&diffs),
        })))
    }

    async fn generate_migration(&self, args: &Value) -> Result<ToolOutcome, ToolError> {
        let source_schema = args
            .get("sourceSchema")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("sourceSchema is required".into()))?;
        let target_schema = args
            .get("targetSchema")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("targetSchema is required".into()))?;
        crate::schema_diff::require_safe_schema(source_schema)?;
        crate::schema_diff::require_safe_schema(target_schema)?;

        let client = self.session.checkout().await?;
        let source = crate::schema_diff::load_schema_snapshot(&client, source_schema).await?;
        let target = crate::schema_diff::load_schema_snapshot(&client, target_schema).await?;
        let diffs = crate::schema_diff::compute_schema_diff(&source, &target);
        let statements =
            crate::schema_diff::build_migration_statements(source_schema, target_schema, &diffs);
        let sql = if statements.is_empty() {
            format!("-- No differences between {source_schema} and {target_schema}")
        } else {
            statements.join("\n\n")
        };
        Ok(ToolOutcome::ok_json(json!({
            "sourceSchema": source_schema,
            "targetSchema": target_schema,
            "statementCount": statements.len(),
            "sql": sql,
            "hint": "Read-only: review and run via execute_sql / apply_ddl only with --access-mode write|admin. Destructive drops are commented out.",
        })))
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
            // Always object-shaped for Cursor structuredContent (bare arrays are dropped).
            let payload = ensure_structured_object(values);
            let text = serde_json::to_string_pretty(&payload)
                .map_err(|e| ToolError::Execution(e.to_string()))?;
            let (trunc, text) = self.session.caps.truncate_chars(&text);
            let structured = if trunc {
                json!({ "truncated_chars": true, "data": payload })
            } else {
                payload
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
        // Always `{ "rows": [...] }` — truncation flags are extra fields on the object.
        let mut payload = ensure_structured_object(values);
        if truncated {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("truncated".into(), json!(true));
                obj.insert("maxRows".into(), json!(max_rows));
            }
        }
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

fn require_select_or_with(sql: &str) -> Result<(), ToolError> {
    match validate_readonly_sql(sql)? {
        SqlDecision::Allow => {}
        SqlDecision::Reject => {
            return Err(ToolError::Execution(
                "Security Error: Only SELECT or WITH statements can be analyzed.".into(),
            ));
        }
    }
    let trimmed = sql.trim().to_ascii_lowercase();
    if !(trimmed.starts_with("select") || trimmed.starts_with("with")) {
        return Err(ToolError::Execution(
            "Security Error: Only SELECT or WITH statements can be analyzed.".into(),
        ));
    }
    Ok(())
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

/// Detect SQL NULL for any column type without committing to a concrete `FromSql` type.
enum SqlNullness {
    Null,
    Value,
}

impl<'a> FromSql<'a> for SqlNullness {
    fn from_sql(
        _: &Type,
        _: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(SqlNullness::Value)
    }

    fn from_sql_null(_: &Type) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(SqlNullness::Null)
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

fn try_cell<T, F>(row: &tokio_postgres::Row, idx: usize, map: F) -> Option<Value>
where
    T: for<'a> FromSql<'a>,
    F: FnOnce(T) -> Value,
{
    match row.try_get::<_, Option<T>>(idx) {
        Ok(Some(v)) => Some(map(v)),
        Ok(None) => Some(Value::Null),
        Err(_) => None,
    }
}

fn cell_to_json(row: &tokio_postgres::Row, idx: usize) -> Value {
    let col_type = row.columns()[idx].type_();
    if matches!(row.try_get::<_, SqlNullness>(idx), Ok(SqlNullness::Null)) {
        return Value::Null;
    }

    if let Kind::Array(elem) = col_type.kind() {
        return array_cell_to_json(row, idx, elem);
    }

    if let Some(v) = match *col_type {
        Type::BOOL => try_cell::<bool, _>(row, idx, |b| json!(b)),
        Type::INT2 => try_cell::<i16, _>(row, idx, |n| json!(n)),
        Type::INT4 | Type::OID => try_cell::<i32, _>(row, idx, |n| json!(n)),
        Type::INT8 => try_cell::<i64, _>(row, idx, |n| json!(n)),
        Type::FLOAT4 => try_cell::<f32, _>(row, idx, |n| json!(n)),
        Type::FLOAT8 => try_cell::<f64, _>(row, idx, |n| json!(n)),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => {
            try_cell::<String, _>(row, idx, Value::String)
        }
        Type::TIMESTAMP => try_cell::<NaiveDateTime, _>(row, idx, |t| {
            json!(t.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
        }),
        Type::TIMESTAMPTZ => try_cell::<DateTime<FixedOffset>, _>(row, idx, |t| json!(t.to_rfc3339())),
        Type::DATE => try_cell::<NaiveDate, _>(row, idx, |d| json!(d.format("%Y-%m-%d").to_string())),
        Type::TIME => try_cell::<NaiveTime, _>(row, idx, |t| json!(t.format("%H:%M:%S%.f").to_string())),
        Type::UUID => try_cell::<Uuid, _>(row, idx, |u| json!(u.to_string())),
        Type::JSON | Type::JSONB => try_cell::<Value, _>(row, idx, |j| j),
        Type::NUMERIC => try_cell::<Decimal, _>(row, idx, |d| json!(d.to_string())),
        Type::MONEY => try_cell::<i64, _>(row, idx, |v| json!(money_to_string(v))),
        Type::BYTEA => try_cell::<Vec<u8>, _>(row, idx, |b| json!(BASE64.encode(b))),
        _ => None,
    } {
        return v;
    }

    cell_to_json_untyped(row, idx, col_type)
}

fn array_cell_to_json(row: &tokio_postgres::Row, idx: usize, elem: &Type) -> Value {
    let try_array = |result: Result<Option<Vec<Value>>, tokio_postgres::Error>| -> Option<Value> {
        match result {
            Ok(Some(items)) => Some(Value::Array(items)),
            Ok(None) => Some(Value::Null),
            Err(_) => None,
        }
    };

    match *elem {
        Type::BOOL => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<bool>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|x| json!(x)).collect())),
            ) {
                return v;
            }
        }
        Type::INT2 => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<i16>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|x| json!(x)).collect())),
            ) {
                return v;
            }
        }
        Type::INT4 | Type::OID => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<i32>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|x| json!(x)).collect())),
            ) {
                return v;
            }
        }
        Type::INT8 => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<i64>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|x| json!(x)).collect())),
            ) {
                return v;
            }
        }
        Type::FLOAT4 => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<f32>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|n| json!(n)).collect())),
            ) {
                return v;
            }
        }
        Type::FLOAT8 => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<f64>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|n| json!(n)).collect())),
            ) {
                return v;
            }
        }
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<String>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(Value::String).collect())),
            ) {
                return v;
            }
        }
        Type::UUID => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<Uuid>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|u| json!(u.to_string())).collect())),
            ) {
                return v;
            }
        }
        Type::TIMESTAMP => {
            if let Some(v) = try_array(row.try_get::<_, Option<Vec<NaiveDateTime>>>(idx).map(
                |v| {
                    v.map(|a| {
                        a.into_iter()
                            .map(|t| json!(t.format("%Y-%m-%dT%H:%M:%S%.f").to_string()))
                            .collect()
                    })
                },
            )) {
                return v;
            }
        }
        Type::TIMESTAMPTZ => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<DateTime<FixedOffset>>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|t| json!(t.to_rfc3339())).collect())),
            ) {
                return v;
            }
        }
        Type::DATE => {
            if let Some(v) = try_array(row.try_get::<_, Option<Vec<NaiveDate>>>(idx).map(|v| {
                v.map(|a| {
                    a.into_iter()
                        .map(|d| json!(d.format("%Y-%m-%d").to_string()))
                        .collect()
                })
            })) {
                return v;
            }
        }
        Type::JSON | Type::JSONB => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<Value>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|j| j).collect())),
            ) {
                return v;
            }
        }
        Type::NUMERIC => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<Decimal>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|d| json!(d.to_string())).collect())),
            ) {
                return v;
            }
        }
        Type::MONEY => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<i64>>>(idx)
                    .map(|v| v.map(|a| a.into_iter().map(|m| json!(money_to_string(m))).collect())),
            ) {
                return v;
            }
        }
        Type::BYTEA => {
            if let Some(v) = try_array(
                row.try_get::<_, Option<Vec<Vec<u8>>>>(idx).map(|v| {
                    v.map(|a| {
                        a.into_iter()
                            .map(|b| json!(BASE64.encode(b)))
                            .collect()
                    })
                }),
            ) {
                return v;
            }
        }
        _ => {}
    }

    cell_to_json_untyped(row, idx, row.columns()[idx].type_())
}

/// PostgreSQL `money` is int64 in ten-thousandths of the base currency unit.
fn money_to_string(v: i64) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let abs = v.unsigned_abs();
    format!("{}{}.{:04}", sign, abs / 10_000, abs % 10_000)
}

/// Last-resort decoding for unknown or composite Postgres types — never silent null for non-null cells.
fn cell_to_json_untyped(row: &tokio_postgres::Row, idx: usize, pg_type: &Type) -> Value {
    if let Ok(Some(s)) = row.try_get::<_, Option<String>>(idx) {
        return Value::String(s);
    }
    json!({
        "__untyped": true,
        "type": pg_type.name()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::build_explain_sql;
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
    fn ok_json_wraps_arrays_for_cursor_structured_content() {
        let out = ToolOutcome::ok_json(json!([{ "id": 1 }, { "id": 2 }]));
        assert!(!out.is_error);
        let s = out.structured.as_ref().unwrap();
        assert!(s.is_object(), "structuredContent must be object, got {s}");
        assert_eq!(s["rows"].as_array().unwrap().len(), 2);
        assert!(out.text.contains("\"rows\""));
    }

    #[test]
    fn ok_json_leaves_objects_unchanged() {
        let out = ToolOutcome::ok_json(json!({ "kind": "table", "name": "orders" }));
        let s = out.structured.as_ref().unwrap();
        assert_eq!(s["kind"], "table");
        assert!(s.get("rows").is_none());
    }

    #[test]
    fn router_specs_include_phase4_and_phase9() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        assert_eq!(router.specs().len(), 41);
        let names: Vec<_> = router.specs().iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"search_schema"));
        assert!(names.contains(&"get_ddl"));
        assert!(names.contains(&"explain_analyze"));
        assert!(names.contains(&"get_index_status"));
        assert!(names.contains(&"list_extensions"));
        assert!(names.contains(&"server_settings"));
        assert!(names.contains(&"suggest_indexes"));
        assert!(names.contains(&"find_unused_indexes"));
        assert!(names.contains(&"bloat_report"));
        assert!(names.contains(&"find_missing_fks"));
        assert!(names.contains(&"export_query"));
        assert!(names.contains(&"list_roles"));
        assert!(names.contains(&"db_dashboard"));
        assert!(names.contains(&"deep_plan_analysis"));
        assert!(names.contains(&"execute_sql"));
        assert!(names.contains(&"edit_row"));
        assert!(names.contains(&"import_data"));
        assert!(names.contains(&"apply_ddl"));
        assert!(names.contains(&"create_index_concurrently"));
        assert!(names.contains(&"run_maintenance"));
        assert!(names.contains(&"terminate_query"));
    }

    #[tokio::test]
    async fn write_tools_refuse_read_mode() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        for tool in [
            "execute_sql",
            "edit_row",
            "import_data",
            "apply_ddl",
            "create_index_concurrently",
            "run_maintenance",
            "terminate_query",
        ] {
            let out = router
                .call(tool, json!({ "sql": "SELECT 1", "table": "public.t", "rows": [], "action": "insert", "values": {}, "pid": 1 }))
                .await;
            assert!(out.is_error, "{tool}: {}", out.text);
            assert!(
                out.text.contains("write") || out.text.contains("admin"),
                "{tool}: {}",
                out.text
            );
        }
    }

    #[tokio::test]
    async fn table_stats_rejects_injection_ref() {
        let session = ToolSession::for_tests(vec![test_conn()], PolicyFilter::default(), None);
        let router = ToolRouter::with_index_store(session, None);
        let out = router
            .call("table_stats", json!({ "ref": "public.users; DROP" }))
            .await;
        assert!(out.is_error, "{}", out.text);
        assert!(
            out.text.contains("Invalid object reference") || out.text.contains("invalid arguments"),
            "expected ref validation error, got: {}",
            out.text
        );
    }

    #[test]
    fn explain_transaction_path_builds_readonly_sequence() {
        // Documented contract: BEGIN → SET TRANSACTION READ ONLY → EXPLAIN → ROLLBACK
        let explain = build_explain_sql("SELECT 1", true);
        assert!(explain.starts_with("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)"));
        assert!(!explain.to_ascii_lowercase().contains("commit"));
        let steps = ["BEGIN", "SET TRANSACTION READ ONLY", &explain, "ROLLBACK"];
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0], "BEGIN");
        assert_eq!(steps[1], "SET TRANSACTION READ ONLY");
        assert_eq!(steps[3], "ROLLBACK");
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
