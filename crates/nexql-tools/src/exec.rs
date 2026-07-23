//! Tool dispatch for the Phase 2 catalog surface.

use std::sync::Arc;

use nexql_policy::{SqlDecision, validate_readonly_sql};
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::registry::ToolName;
use crate::schema::{ToolSpec, phase2_catalog_tools};
use crate::session::ToolSession;

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
    specs: Vec<ToolSpec>,
}

impl ToolRouter {
    pub fn new(session: Arc<ToolSession>) -> Self {
        Self {
            session,
            specs: phase2_catalog_tools(),
        }
    }

    pub fn specs(&self) -> &[ToolSpec] {
        &self.specs
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
            _ => Err(ToolError::Unknown(format!(
                "{name} is not available until a later phase"
            ))),
        }
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
