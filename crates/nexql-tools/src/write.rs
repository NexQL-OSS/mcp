// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Write/admin MCP tool executors (Phase 9).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use deadpool_postgres::Object;
use nexql_policy::{
    AccessMode, ObjectRef, SqlDecision, enforce_read_table_policy, select_table_refs,
    validate_readonly_sql, validate_write_sql,
};
use rust_decimal::Decimal;
use serde_json::{Map, Value, json};
use tokio_postgres::SimpleQueryMessage;
use tokio_postgres::types::{Json, ToSql};
use uuid::Uuid;

use crate::cell_json::{redact_pii_in_rows, rows_to_json_vec};
use crate::error::ToolError;
use crate::exec::ToolOutcome;
use crate::session::ToolSession;
use crate::sql::{is_safe_ident, parse_ref, quote_ident, quote_ref};

const IMPORT_BATCH_SIZE: usize = 100;
const MUTATION_DIFF_ROW_CAP: i64 = 100;

/// Run validated SQL inside an explicit transaction; roll back on error or `dry_run`.
pub async fn execute_sql(
    session: &Arc<ToolSession>,
    sql: &str,
    dry_run: bool,
    include_diff: bool,
) -> Result<ToolOutcome, ToolError> {
    let mode = session.access_mode();
    match validate_write_sql(mode, sql)? {
        SqlDecision::Allow => {}
        SqlDecision::Reject => {
            return Err(ToolError::Execution(format!(
                "Security Error: SQL is not permitted in {:?} mode.",
                mode
            )));
        }
    }
    if matches!(validate_readonly_sql(sql)?, SqlDecision::Allow) {
        enforce_read_table_policy(&session.filter(), sql)?;
    }

    let client = session.checkout().await?;
    client.batch_execute("BEGIN").await?;
    let outcome = async {
        let (rows, command_tag) = run_simple_query(&client, sql).await?;
        Ok::<_, ToolError>((rows, command_tag))
    }
    .await;

    let rolled_back = dry_run || outcome.is_err();
    if rolled_back {
        let _ = client.batch_execute("ROLLBACK").await;
    } else {
        let _ = client.batch_execute("COMMIT").await;
    }

    match outcome {
        Ok((rows, rows_affected)) => {
            let rows = redact_row_results(session, Some(sql), None, None, rows);
            let mut payload = json!({
                "dry_run": dry_run,
                "rolled_back": rolled_back,
                "rows_affected": rows_affected,
                "rows": rows,
            });
            if include_diff && !rows.is_empty() {
                payload["after"] = json!(rows.clone());
            }
            Ok(ToolOutcome::ok_json(payload))
        }
        Err(e) => Err(append_constraint_hint(e)),
    }
}

fn append_constraint_hint(err: ToolError) -> ToolError {
    if let ToolError::Execution(ref msg) = err
        && let Some(name) = extract_constraint_name(msg)
    {
        return ToolError::Execution(format!("{msg} (constraint: {name})"));
    }
    err
}

fn extract_constraint_name(message: &str) -> Option<String> {
    message
        .split("constraint \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .map(str::to_owned)
}

/// Structured insert/update/delete by primary key (parameterized).
pub async fn edit_row(session: &Arc<ToolSession>, args: &Value) -> Result<ToolOutcome, ToolError> {
    let table_ref = args
        .get("table")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidArgs("table is required (schema.name)".into()))?;
    let action = args.get("action").and_then(|v| v.as_str()).ok_or_else(|| {
        ToolError::InvalidArgs("action is required (insert|update|delete)".into())
    })?;
    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let include_diff = args
        .get("include_diff")
        .and_then(|v| v.as_bool())
        .unwrap_or(dry_run);

    let (schema, table) = parse_ref(table_ref).map_err(ToolError::InvalidArgs)?;
    if !session.filter().allows_table(&schema, &table) {
        return Err(ToolError::Execution(format!(
            "Table \"{schema}.{table}\" is denied by policy filter."
        )));
    }

    let client = session.checkout().await?;
    client.batch_execute("BEGIN").await?;

    let result = async {
        match action.to_ascii_lowercase().as_str() {
            "insert" => {
                edit_row_insert(session, &client, &schema, &table, args, include_diff).await
            }
            "update" => {
                edit_row_update(session, &client, &schema, &table, args, include_diff).await
            }
            "delete" => {
                edit_row_delete(session, &client, &schema, &table, args, include_diff).await
            }
            other => Err(ToolError::InvalidArgs(format!(
                "Unsupported action \"{other}\". Use insert, update, or delete."
            ))),
        }
    }
    .await;

    let rolled_back = dry_run || result.is_err();
    if rolled_back {
        let _ = client.batch_execute("ROLLBACK").await;
    } else {
        let _ = client.batch_execute("COMMIT").await;
        let (connection_id, database) = session.active_context().await;
        session.mark_index_stale(&connection_id, &database);
    }

    match result {
        Ok(mut outcome) => {
            if let Some(obj) = outcome.structured.as_mut().and_then(|v| v.as_object_mut()) {
                obj.insert("dry_run".into(), json!(dry_run));
                obj.insert("rolled_back".into(), json!(rolled_back));
            }
            Ok(outcome)
        }
        Err(e) => Err(append_constraint_hint(e)),
    }
}

async fn edit_row_insert(
    session: &Arc<ToolSession>,
    client: &Object,
    schema: &str,
    table: &str,
    args: &Value,
    include_diff: bool,
) -> Result<ToolOutcome, ToolError> {
    let values = args
        .get("values")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ToolError::InvalidArgs("values object is required for insert".into()))?;
    if values.is_empty() {
        return Err(ToolError::InvalidArgs(
            "values must contain at least one column".into(),
        ));
    }
    let column_types = load_column_types(client, schema, table).await?;
    let mut columns = Vec::new();
    let mut params: Vec<Box<dyn ToSql + Sync + Send>> = Vec::new();
    for (col, val) in values {
        validate_column_name(col)?;
        columns.push(quote_ident(col));
        params.push(json_to_sql_param(
            column_types.get(col).map(String::as_str),
            val,
        )?);
    }
    let placeholders: Vec<String> = (1..=params.len()).map(|i| format!("${i}")).collect();
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({}) RETURNING *",
        quote_ref(schema, table),
        columns.join(", "),
        placeholders.join(", ")
    );
    let param_refs: Vec<&(dyn ToSql + Sync)> = params
        .iter()
        .map(|p| p.as_ref() as &(dyn ToSql + Sync))
        .collect();
    let rows = client.query(&sql, &param_refs[..]).await?;
    let rows = redact_row_results(session, None, Some(schema), Some(table), simple_rows_to_json(&rows));
    let mut payload = json!({
        "action": "insert",
        "table": format!("{schema}.{table}"),
        "rows_affected": rows.len(),
        "rows": rows,
    });
    if include_diff {
        payload["after"] = json!(rows);
    }
    Ok(ToolOutcome::ok_json(payload))
}

async fn edit_row_update(
    session: &Arc<ToolSession>,
    client: &Object,
    schema: &str,
    table: &str,
    args: &Value,
    include_diff: bool,
) -> Result<ToolOutcome, ToolError> {
    let pk = args
        .get("pk")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ToolError::InvalidArgs("pk object is required for update".into()))?;
    if pk.is_empty() {
        return Err(ToolError::InvalidArgs(
            "pk must contain at least one primary-key column".into(),
        ));
    }
    let values = args
        .get("values")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ToolError::InvalidArgs("values object is required for update".into()))?;
    if values.is_empty() {
        return Err(ToolError::InvalidArgs(
            "values must contain at least one column to update".into(),
        ));
    }

    let column_types = load_column_types(client, schema, table).await?;
    let (where_sql, pk_params) = pk_where_clause(pk, &column_types)?;
    if include_diff {
        ensure_pk_row_cap(client, schema, table, &where_sql, &pk_params).await?;
    }
    let before = if include_diff {
        snapshot_rows(client, schema, table, &where_sql, &pk_params).await?
    } else {
        Vec::new()
    };

    let mut set_cols = Vec::new();
    let mut params: Vec<Box<dyn ToSql + Sync + Send>> = Vec::new();
    for (col, val) in values {
        validate_column_name(col)?;
        let idx = params.len() + 1;
        set_cols.push(format!("{} = ${idx}", quote_ident(col)));
        params.push(json_to_sql_param(
            column_types.get(col).map(String::as_str),
            val,
        )?);
    }
    let pk_offset = params.len();
    let mut where_cols = Vec::new();
    for (col, val) in pk {
        validate_column_name(col)?;
        let idx = params.len() + 1;
        where_cols.push(format!("{} = ${idx}", quote_ident(col)));
        params.push(json_to_sql_param(
            column_types.get(col).map(String::as_str),
            val,
        )?);
    }
    let _ = pk_offset;
    let sql = format!(
        "UPDATE {} SET {} WHERE {} RETURNING *",
        quote_ref(schema, table),
        set_cols.join(", "),
        where_cols.join(" AND ")
    );
    let param_refs: Vec<&(dyn ToSql + Sync)> = params
        .iter()
        .map(|p| p.as_ref() as &(dyn ToSql + Sync))
        .collect();
    let rows = client.query(&sql, &param_refs[..]).await?;
    let after =
        redact_row_results(session, None, Some(schema), Some(table), simple_rows_to_json(&rows));
    let mut payload = json!({
        "action": "update",
        "table": format!("{schema}.{table}"),
        "rows_affected": after.len(),
        "rows": after,
    });
    if include_diff {
        payload["before"] = json!(before);
        payload["after"] = json!(after);
        payload["diff"] = json!(compute_row_diff(&before, &after));
    }
    Ok(ToolOutcome::ok_json(payload))
}

async fn edit_row_delete(
    session: &Arc<ToolSession>,
    client: &Object,
    schema: &str,
    table: &str,
    args: &Value,
    include_diff: bool,
) -> Result<ToolOutcome, ToolError> {
    let pk = args
        .get("pk")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ToolError::InvalidArgs("pk object is required for delete".into()))?;
    if pk.is_empty() {
        return Err(ToolError::InvalidArgs(
            "pk must contain at least one primary-key column".into(),
        ));
    }
    let column_types = load_column_types(client, schema, table).await?;
    let (where_sql, params) = pk_where_clause(pk, &column_types)?;
    if include_diff {
        ensure_pk_row_cap(client, schema, table, &where_sql, &params).await?;
    }
    let before = if include_diff {
        snapshot_rows(client, schema, table, &where_sql, &params).await?
    } else {
        Vec::new()
    };
    let sql = format!(
        "DELETE FROM {} WHERE {} RETURNING *",
        quote_ref(schema, table),
        where_sql
    );
    let param_refs: Vec<&(dyn ToSql + Sync)> = params
        .iter()
        .map(|p| p.as_ref() as &(dyn ToSql + Sync))
        .collect();
    let rows = client.query(&sql, &param_refs[..]).await?;
    let after =
        redact_row_results(session, None, Some(schema), Some(table), simple_rows_to_json(&rows));
    let mut payload = json!({
        "action": "delete",
        "table": format!("{schema}.{table}"),
        "rows_affected": after.len(),
        "rows": after,
    });
    if include_diff {
        payload["before"] = json!(before);
        payload["after"] = json!(after);
        payload["diff"] = json!(compute_row_diff(&before, &after));
    }
    Ok(ToolOutcome::ok_json(payload))
}

/// Batched INSERT from a JSON rows array.
pub async fn import_data(
    session: &Arc<ToolSession>,
    args: &Value,
) -> Result<ToolOutcome, ToolError> {
    let table_ref = args
        .get("table")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidArgs("table is required (schema.name)".into()))?;
    let rows_val = args
        .get("rows")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::InvalidArgs("rows array is required".into()))?;
    if rows_val.is_empty() {
        return Ok(ToolOutcome::ok_json(json!({
            "table": table_ref,
            "rows_imported": 0,
            "batches": 0,
        })));
    }

    let (schema, table) = parse_ref(table_ref).map_err(ToolError::InvalidArgs)?;
    if !session.filter().allows_table(&schema, &table) {
        return Err(ToolError::Execution(format!(
            "Table \"{schema}.{table}\" is denied by policy filter."
        )));
    }

    let columns: Vec<String> = if let Some(cols) = args.get("columns").and_then(|v| v.as_array()) {
        cols.iter()
            .map(|c| {
                let s = c
                    .as_str()
                    .ok_or_else(|| ToolError::InvalidArgs("columns must be strings".into()))?;
                validate_column_name(s)?;
                Ok(s.to_string())
            })
            .collect::<Result<Vec<_>, ToolError>>()?
    } else {
        let first = rows_val[0]
            .as_object()
            .ok_or_else(|| ToolError::InvalidArgs("each row must be a JSON object".into()))?;
        let mut cols: Vec<String> = first.keys().cloned().collect();
        cols.sort();
        for col in &cols {
            validate_column_name(col)?;
        }
        cols
    };

    let client = session.checkout().await?;
    client.batch_execute("BEGIN").await?;

    let mut total_imported = 0u64;
    let mut batches = 0u32;
    let result = async {
        for chunk in rows_val.chunks(IMPORT_BATCH_SIZE) {
            let (sql, params) = build_batch_insert(&schema, &table, &columns, chunk)?;
            let param_refs: Vec<&(dyn ToSql + Sync)> = params
                .iter()
                .map(|p| p.as_ref() as &(dyn ToSql + Sync))
                .collect();
            let affected = client.execute(&sql, &param_refs[..]).await?;
            total_imported += affected;
            batches += 1;
        }
        Ok::<_, ToolError>(())
    }
    .await;

    match &result {
        Ok(_) => {
            let _ = client.batch_execute("COMMIT").await;
        }
        Err(_) => {
            let _ = client.batch_execute("ROLLBACK").await;
        }
    }
    result?;

    Ok(ToolOutcome::ok_json(json!({
        "table": format!("{schema}.{table}"),
        "rows_imported": total_imported,
        "batches": batches,
        "columns": columns,
    })))
}

fn build_batch_insert(
    schema: &str,
    table: &str,
    columns: &[String],
    rows: &[Value],
) -> Result<(String, Vec<Box<dyn ToSql + Sync + Send>>), ToolError> {
    let quoted_cols = columns
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let mut params: Vec<Box<dyn ToSql + Sync + Send>> = Vec::new();
    let mut value_groups = Vec::new();
    for row in rows {
        let obj = row
            .as_object()
            .ok_or_else(|| ToolError::InvalidArgs("each row must be a JSON object".into()))?;
        let mut placeholders = Vec::new();
        for col in columns {
            let val = obj.get(col).unwrap_or(&Value::Null);
            let idx = params.len() + 1;
            placeholders.push(format!("${idx}"));
            params.push(json_to_sql_param(None, val)?);
        }
        value_groups.push(format!("({})", placeholders.join(", ")));
    }
    let sql = format!(
        "INSERT INTO {} ({}) VALUES {}",
        quote_ref(schema, table),
        quoted_cols,
        value_groups.join(", ")
    );
    Ok((sql, params))
}

/// Run DDL validated for Admin mode inside a transaction.
pub async fn apply_ddl(
    session: &Arc<ToolSession>,
    sql: &str,
    dry_run: bool,
) -> Result<ToolOutcome, ToolError> {
    assert_ddl_statement(sql)?;
    match validate_write_sql(AccessMode::Admin, sql)? {
        SqlDecision::Allow => {}
        SqlDecision::Reject => {
            return Err(ToolError::Execution(
                "Security Error: DDL statement is not permitted.".into(),
            ));
        }
    }

    let client = session.checkout().await?;
    client.batch_execute("BEGIN").await?;
    let outcome = run_simple_query(&client, sql).await;
    let rolled_back = dry_run || outcome.is_err();
    if rolled_back {
        let _ = client.batch_execute("ROLLBACK").await;
    } else {
        let _ = client.batch_execute("COMMIT").await;
    }
    let (rows, rows_affected) = outcome?;
    if !rolled_back {
        let (connection_id, database) = session.active_context().await;
        session.mark_index_stale(&connection_id, &database);
    }
    Ok(ToolOutcome::ok_json(json!({
        "dry_run": dry_run,
        "rolled_back": rolled_back,
        "rows_affected": rows_affected,
        "rows": rows,
        "indexStale": !rolled_back,
    })))
}

/// `CREATE INDEX CONCURRENTLY` — must run outside a transaction.
pub async fn create_index_concurrently(
    session: &Arc<ToolSession>,
    sql: &str,
) -> Result<ToolOutcome, ToolError> {
    let upper = sql.trim().to_ascii_uppercase();
    if !upper.contains("CREATE INDEX") || !upper.contains("CONCURRENTLY") {
        return Err(ToolError::InvalidArgs(
            "sql must be a CREATE INDEX CONCURRENTLY statement".into(),
        ));
    }
    match validate_write_sql(AccessMode::Admin, sql)? {
        SqlDecision::Allow => {}
        SqlDecision::Reject => {
            return Err(ToolError::Execution(
                "Security Error: index statement is not permitted.".into(),
            ));
        }
    }

    let client = session.checkout().await?;
    let (rows, rows_affected) = run_simple_query(&client, sql).await?;
    let (connection_id, database) = session.active_context().await;
    session.mark_index_stale(&connection_id, &database);
    Ok(ToolOutcome::ok_json(json!({
        "rows_affected": rows_affected,
        "rows": rows,
        "note": "CREATE INDEX CONCURRENTLY runs outside a transaction.",
        "indexStale": true,
    })))
}

/// VACUUM / ANALYZE / REINDEX — cannot run inside a transaction.
pub async fn run_maintenance(
    session: &Arc<ToolSession>,
    args: &Value,
) -> Result<ToolOutcome, ToolError> {
    let action = args.get("action").and_then(|v| v.as_str()).ok_or_else(|| {
        ToolError::InvalidArgs("action is required (vacuum|analyze|reindex)".into())
    })?;
    let full = args.get("full").and_then(|v| v.as_bool()).unwrap_or(false);
    let table_ref = args.get("table").and_then(|v| v.as_str());

    let sql = match action.to_ascii_lowercase().as_str() {
        "vacuum" => build_vacuum_sql(table_ref, full)?,
        "analyze" => build_analyze_sql(table_ref)?,
        "reindex" => build_reindex_sql(table_ref)?,
        other => {
            return Err(ToolError::InvalidArgs(format!(
                "Unsupported action \"{other}\". Use vacuum, analyze, or reindex."
            )));
        }
    };

    match validate_write_sql(AccessMode::Admin, &sql)? {
        SqlDecision::Allow => {}
        SqlDecision::Reject => {
            return Err(ToolError::Execution(
                "Security Error: maintenance statement is not permitted.".into(),
            ));
        }
    }

    let client = session.checkout().await?;
    let (rows, rows_affected) = run_simple_query(&client, &sql).await?;
    Ok(ToolOutcome::ok_json(json!({
        "action": action,
        "sql": sql,
        "rows_affected": rows_affected,
        "rows": rows,
    })))
}

/// Cancel or terminate a backend by pid.
pub async fn terminate_query(
    session: &Arc<ToolSession>,
    args: &Value,
) -> Result<ToolOutcome, ToolError> {
    let pid = args
        .get("pid")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| ToolError::InvalidArgs("pid is required".into()))?;
    if pid <= 0 {
        return Err(ToolError::InvalidArgs(
            "pid must be a positive integer".into(),
        ));
    }
    let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);

    let client = session.checkout().await?;
    let own_pid: i32 = client
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    if pid == i64::from(own_pid) {
        return Err(ToolError::Execution(
            "refusing to cancel/terminate the current session backend".into(),
        ));
    }

    let target = client
        .query_opt(
            "SELECT pid, usename, state, query, usesuper FROM pg_stat_activity WHERE pid = $1",
            &[&(pid as i32)],
        )
        .await?;
    let Some(row) = target else {
        return Err(ToolError::Execution(format!(
            "No backend found with pid {pid}"
        )));
    };
    let usesuper: bool = row.get("usesuper");
    if usesuper {
        return Err(ToolError::Execution(
            "refusing to cancel/terminate a superuser backend — use a direct superuser session if required"
                .into(),
        ));
    }

    let fn_name = if force {
        "pg_terminate_backend"
    } else {
        "pg_cancel_backend"
    };
    let sql = format!("SELECT {fn_name}($1)");
    let result: bool = client.query_one(&sql, &[&(pid as i32)]).await?.get(0);

    Ok(ToolOutcome::ok_json(json!({
        "pid": pid,
        "force": force,
        "success": result,
        "target": {
            "usename": row.get::<_, Option<String>>("usename"),
            "state": row.get::<_, Option<String>>("state"),
            "query": row.get::<_, Option<String>>("query"),
        },
    })))
}

fn build_vacuum_sql(table_ref: Option<&str>, full: bool) -> Result<String, ToolError> {
    let mut sql = String::from("VACUUM");
    if full {
        sql.push_str(" FULL");
    }
    if let Some(ref_) = table_ref {
        let (schema, table) = parse_ref(ref_).map_err(ToolError::InvalidArgs)?;
        sql.push(' ');
        sql.push_str(&quote_ref(&schema, &table));
    }
    Ok(sql)
}

fn build_analyze_sql(table_ref: Option<&str>) -> Result<String, ToolError> {
    let mut sql = String::from("ANALYZE");
    if let Some(ref_) = table_ref {
        let (schema, table) = parse_ref(ref_).map_err(ToolError::InvalidArgs)?;
        sql.push(' ');
        sql.push_str(&quote_ref(&schema, &table));
    }
    Ok(sql)
}

fn build_reindex_sql(table_ref: Option<&str>) -> Result<String, ToolError> {
    let ref_ = table_ref.ok_or_else(|| {
        ToolError::InvalidArgs("table (schema.name) is required for reindex".into())
    })?;
    let (schema, table) = parse_ref(ref_).map_err(ToolError::InvalidArgs)?;
    Ok(format!("REINDEX TABLE {}", quote_ref(&schema, &table)))
}

fn assert_ddl_statement(sql: &str) -> Result<(), ToolError> {
    let upper = sql.trim().to_ascii_uppercase();
    let ddl_prefixes = [
        "CREATE ",
        "ALTER ",
        "DROP ",
        "TRUNCATE ",
        "COMMENT ON ",
        "GRANT ",
        "REVOKE ",
        "RENAME ",
    ];
    if !ddl_prefixes.iter().any(|p| upper.starts_with(p)) {
        return Err(ToolError::Execution(
            "apply_ddl only accepts DDL statements (CREATE, ALTER, DROP, TRUNCATE, …). Use execute_sql for DML."
                .into(),
        ));
    }
    Ok(())
}

fn pk_where_clause(
    pk: &Map<String, Value>,
    column_types: &HashMap<String, String>,
) -> Result<(String, Vec<Box<dyn ToSql + Sync + Send>>), ToolError> {
    let mut where_cols = Vec::new();
    let mut params: Vec<Box<dyn ToSql + Sync + Send>> = Vec::new();
    for (col, val) in pk {
        validate_column_name(col)?;
        let idx = params.len() + 1;
        where_cols.push(format!("{} = ${idx}", quote_ident(col)));
        params.push(json_to_sql_param(
            column_types.get(col).map(String::as_str),
            val,
        )?);
    }
    Ok((where_cols.join(" AND "), params))
}

async fn ensure_pk_row_cap(
    client: &Object,
    schema: &str,
    table: &str,
    where_sql: &str,
    params: &[Box<dyn ToSql + Sync + Send>],
) -> Result<(), ToolError> {
    let sql = format!(
        "SELECT COUNT(*)::bigint FROM {} WHERE {where_sql}",
        quote_ref(schema, table)
    );
    let param_refs: Vec<&(dyn ToSql + Sync)> = params
        .iter()
        .map(|p| p.as_ref() as &(dyn ToSql + Sync))
        .collect();
    let row = client.query_one(&sql, &param_refs[..]).await?;
    let count: i64 = row.get(0);
    if count > MUTATION_DIFF_ROW_CAP {
        return Err(ToolError::Execution(format!(
            "Mutation would affect {count} rows — diff capture is capped at {MUTATION_DIFF_ROW_CAP}. Narrow the primary key predicate."
        )));
    }
    Ok(())
}

async fn snapshot_rows(
    client: &Object,
    schema: &str,
    table: &str,
    where_sql: &str,
    params: &[Box<dyn ToSql + Sync + Send>],
) -> Result<Vec<Value>, ToolError> {
    let sql = format!(
        "SELECT * FROM {} WHERE {where_sql}",
        quote_ref(schema, table)
    );
    let param_refs: Vec<&(dyn ToSql + Sync)> = params
        .iter()
        .map(|p| p.as_ref() as &(dyn ToSql + Sync))
        .collect();
    let rows = client.query(&sql, &param_refs[..]).await?;
    Ok(simple_rows_to_json(&rows))
}

fn compute_row_diff(before: &[Value], after: &[Value]) -> Vec<Value> {
    let mut diffs = Vec::new();
    let pairs = before.len().min(after.len());
    for idx in 0..pairs {
        let (Some(b_obj), Some(a_obj)) = (before[idx].as_object(), after[idx].as_object()) else {
            continue;
        };
        for (col, before_val) in b_obj {
            let after_val = a_obj.get(col).unwrap_or(&Value::Null);
            if before_val != after_val {
                diffs.push(json!({
                    "row": idx,
                    "column": col,
                    "before": before_val,
                    "after": after_val,
                }));
            }
        }
    }
    diffs
}

fn validate_column_name(col: &str) -> Result<(), ToolError> {
    if !is_safe_ident(col) {
        return Err(ToolError::InvalidArgs(format!(
            "Invalid column name \"{col}\"."
        )));
    }
    Ok(())
}

fn json_to_sql_param(
    pg_type: Option<&str>,
    val: &Value,
) -> Result<Box<dyn ToSql + Sync + Send>, ToolError> {
    let typ = pg_type.unwrap_or("text").to_ascii_lowercase();
    match val {
        Value::Null => Ok(Box::new(None::<String>)),
        Value::Bool(b) => Ok(Box::new(*b)),
        Value::String(s) => {
            if typ.contains("json") {
                return Ok(Box::new(Json(val.clone())));
            }
            if typ.contains("uuid") {
                let parsed = Uuid::parse_str(s).map_err(|e| {
                    ToolError::InvalidArgs(format!("invalid uuid for column: {e}"))
                })?;
                return Ok(Box::new(parsed));
            }
            if typ.contains("int") || typ == "bigint" || typ == "smallint" {
                let n: i64 = s.parse().map_err(|e| {
                    ToolError::InvalidArgs(format!("invalid integer for column: {e}"))
                })?;
                return Ok(Box::new(n));
            }
            if typ.contains("numeric") || typ.contains("decimal") {
                let d = Decimal::from_str_exact(s).or_else(|_| s.parse::<Decimal>()).map_err(
                    |e| ToolError::InvalidArgs(format!("invalid numeric for column: {e}")),
                )?;
                return Ok(Box::new(d));
            }
            if typ.contains("timestamp") {
                if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                    return Ok(Box::new(dt));
                }
                if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
                    return Ok(Box::new(dt));
                }
            }
            if typ == "date" {
                let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| {
                    ToolError::InvalidArgs(format!("invalid date for column: {e}"))
                })?;
                return Ok(Box::new(d));
            }
            Ok(Box::new(s.clone()))
        }
        Value::Number(n) => {
            if typ.contains("json") {
                return Ok(Box::new(Json(val.clone())));
            }
            if typ.contains("int") || typ == "bigint" || typ == "smallint" {
                let i = n
                    .as_i64()
                    .ok_or_else(|| ToolError::InvalidArgs("integer out of range".into()))?;
                return Ok(Box::new(i));
            }
            if typ.contains("numeric") || typ.contains("decimal") {
                let d = Decimal::from_str_exact(&n.to_string()).map_err(|e| {
                    ToolError::InvalidArgs(format!("invalid numeric for column: {e}"))
                })?;
                return Ok(Box::new(d));
            }
            if let Some(f) = n.as_f64() {
                return Ok(Box::new(f));
            }
            Ok(Box::new(n.to_string()))
        }
        Value::Array(_) | Value::Object(_) => Ok(Box::new(Json(val.clone()))),
    }
}

async fn load_column_types(
    client: &Object,
    schema: &str,
    table: &str,
) -> Result<HashMap<String, String>, ToolError> {
    let rows = client
        .query(
            r#"SELECT a.attname AS name, format_type(a.atttypid, a.atttypmod) AS typ
               FROM pg_attribute a
               JOIN pg_class c ON c.oid = a.attrelid
               JOIN pg_namespace n ON n.oid = c.relnamespace
               WHERE n.nspname = $1 AND c.relname = $2
                 AND a.attnum > 0 AND NOT a.attisdropped"#,
            &[&schema, &table],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get::<_, String>("name"), r.get::<_, String>("typ")))
        .collect())
}

async fn run_simple_query(
    client: &Object,
    sql: &str,
) -> Result<(Vec<Value>, Option<u64>), ToolError> {
    let messages = client.simple_query(sql).await?;
    Ok(collect_simple_query(messages))
}

fn collect_simple_query(messages: Vec<SimpleQueryMessage>) -> (Vec<Value>, Option<u64>) {
    let mut rows = Vec::new();
    let mut rows_affected = None;
    for msg in messages {
        match msg {
            SimpleQueryMessage::Row(row) => {
                let mut map = Map::new();
                for col in row.columns() {
                    let cell = row
                        .try_get(col.name())
                        .ok()
                        .flatten()
                        .map(|s| Value::String(s.to_string()))
                        .unwrap_or(Value::Null);
                    map.insert(col.name().to_string(), cell);
                }
                rows.push(Value::Object(map));
            }
            SimpleQueryMessage::CommandComplete(n) => rows_affected = Some(n),
            SimpleQueryMessage::RowDescription(_) => {}
            _ => {}
        }
    }
    (rows, rows_affected)
}

fn simple_rows_to_json(rows: &[tokio_postgres::Row]) -> Vec<Value> {
    rows_to_json_vec(rows)
}

fn redact_row_results(
    session: &ToolSession,
    sql: Option<&str>,
    schema: Option<&str>,
    table: Option<&str>,
    rows: Vec<Value>,
) -> Vec<Value> {
    let filter = session.filter();
    if filter.pii_columns.is_empty() {
        return rows;
    }
    let tables = if let (Some(schema), Some(table)) = (schema, table) {
        vec![ObjectRef::new(schema, table)]
    } else if let Some(sql) = sql {
        select_table_refs(sql).unwrap_or_default()
    } else {
        Vec::new()
    };
    redact_pii_in_rows(rows, &filter.pii_columns, &tables).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_ddl_rejects_select() {
        assert!(assert_ddl_statement("SELECT 1").is_err());
    }

    #[test]
    fn assert_ddl_accepts_create() {
        assert!(assert_ddl_statement("CREATE TABLE t (id int)").is_ok());
    }

    #[test]
    fn execute_sql_enforces_read_table_policy() {
        use nexql_policy::{PolicyFilter, SqlDecision, enforce_read_table_policy, validate_readonly_sql};

        let sql = "SELECT * FROM auth.credentials";
        assert_eq!(validate_readonly_sql(sql).unwrap(), SqlDecision::Allow);
        let filter = PolicyFilter {
            deny_schemas: vec!["auth".into()],
            ..Default::default()
        };
        assert!(enforce_read_table_policy(&filter, sql).is_err());
    }

    #[test]
    fn build_vacuum_table() {
        let sql = build_vacuum_sql(Some("public.users"), false).unwrap();
        assert_eq!(sql, "VACUUM \"public\".\"users\"");
    }

    #[test]
    fn build_batch_insert_sql() {
        let rows = [json!({"id": 1, "name": "a"}), json!({"id": 2, "name": "b"})];
        let (sql, params) =
            build_batch_insert("public", "users", &["id".into(), "name".into()], &rows).unwrap();
        assert!(sql.starts_with("INSERT INTO \"public\".\"users\""));
        assert_eq!(params.len(), 4);
    }
}
