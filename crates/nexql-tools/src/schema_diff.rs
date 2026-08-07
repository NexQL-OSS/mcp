// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Schema snapshot load + pure diff/migration (ported from core SchemaDiffEngine).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_postgres::Client;

use crate::error::ToolError;
use crate::sql::is_safe_ident;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SchemaSnapshot {
    pub tables: Vec<TableSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TableSnapshot {
    pub name: String,
    pub schema: String,
    pub columns: Vec<ColumnSnapshot>,
    pub constraints: Vec<ConstraintSnapshot>,
    pub indexes: Vec<IndexSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnSnapshot {
    pub column_name: String,
    pub data_type: String,
    pub not_null: bool,
    pub default_value: Option<String>,
    pub ordinal: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConstraintSnapshot {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub definition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexSnapshot {
    pub name: String,
    pub definition: String,
    pub is_unique: bool,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffStatus {
    Added,
    Removed,
    Changed,
    Unchanged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableDiff {
    pub name: String,
    pub status: DiffStatus,
    pub column_diffs: Vec<ColumnDiff>,
    pub constraint_diffs: Vec<ConstraintDiff>,
    pub index_diffs: Vec<IndexDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDiff {
    pub name: String,
    pub status: DiffStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<ColumnSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<ColumnSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintDiff {
    pub name: String,
    pub status: DiffStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<ConstraintSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<ConstraintSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexDiff {
    pub name: String,
    pub status: DiffStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<IndexSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<IndexSnapshot>,
}

pub fn require_safe_schema(schema: &str) -> Result<(), ToolError> {
    if !is_safe_ident(schema) {
        return Err(ToolError::InvalidArgs(format!(
            "Invalid schema name \"{schema}\""
        )));
    }
    Ok(())
}

pub async fn load_schema_snapshot(
    client: &Client,
    schema: &str,
) -> Result<SchemaSnapshot, ToolError> {
    require_safe_schema(schema)?;
    let table_rows = client
        .query(
            r#"
            SELECT c.relname AS name
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname = $1
              AND c.relkind = 'r'
              AND NOT c.relispartition
            ORDER BY c.relname
            "#,
            &[&schema],
        )
        .await?;

    let mut tables = Vec::with_capacity(table_rows.len());
    for row in &table_rows {
        let name: String = row.get("name");
        let columns = load_columns(client, schema, &name).await?;
        let constraints = load_constraints(client, schema, &name).await?;
        let indexes = load_indexes(client, schema, &name).await?;
        tables.push(TableSnapshot {
            name,
            schema: schema.to_owned(),
            columns,
            constraints,
            indexes,
        });
    }
    Ok(SchemaSnapshot { tables })
}

async fn load_columns(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnSnapshot>, ToolError> {
    let rows = client
        .query(
            r#"
            SELECT
              a.attname AS column_name,
              pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
              a.attnotnull AS not_null,
              pg_get_expr(ad.adbin, ad.adrelid) AS default_value,
              a.attnum AS ordinal
            FROM pg_attribute a
            JOIN pg_class c ON c.oid = a.attrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum
            WHERE n.nspname = $1
              AND c.relname = $2
              AND a.attnum > 0
              AND NOT a.attisdropped
            ORDER BY a.attnum
            "#,
            &[&schema, &table],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| ColumnSnapshot {
            column_name: r.get("column_name"),
            data_type: r.get("data_type"),
            not_null: r.get("not_null"),
            default_value: r.get("default_value"),
            ordinal: r.get::<_, i16>("ordinal") as i32,
        })
        .collect())
}

async fn load_constraints(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<ConstraintSnapshot>, ToolError> {
    let rows = client
        .query(
            r#"
            SELECT
              con.conname AS name,
              con.contype::text AS type,
              pg_get_constraintdef(con.oid, true) AS definition
            FROM pg_constraint con
            JOIN pg_class c ON c.oid = con.conrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname = $1
              AND c.relname = $2
            ORDER BY con.conname
            "#,
            &[&schema, &table],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| ConstraintSnapshot {
            name: r.get("name"),
            type_: r.get("type"),
            definition: r.get("definition"),
        })
        .collect())
}

async fn load_indexes(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<IndexSnapshot>, ToolError> {
    let rows = client
        .query(
            r#"
            SELECT
              i.relname AS name,
              pg_get_indexdef(i.oid) AS definition,
              ix.indisunique AS is_unique,
              ix.indisprimary AS is_primary
            FROM pg_index ix
            JOIN pg_class t ON t.oid = ix.indrelid
            JOIN pg_namespace n ON n.oid = t.relnamespace
            JOIN pg_class i ON i.oid = ix.indexrelid
            WHERE n.nspname = $1
              AND t.relname = $2
            ORDER BY i.relname
            "#,
            &[&schema, &table],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| IndexSnapshot {
            name: r.get("name"),
            definition: r.get("definition"),
            is_unique: r.get("is_unique"),
            is_primary: r.get("is_primary"),
        })
        .collect())
}

pub fn compute_schema_diff(source: &SchemaSnapshot, target: &SchemaSnapshot) -> Vec<TableDiff> {
    let source_map: std::collections::HashMap<&str, &TableSnapshot> =
        source.tables.iter().map(|t| (t.name.as_str(), t)).collect();
    let target_map: std::collections::HashMap<&str, &TableSnapshot> =
        target.tables.iter().map(|t| (t.name.as_str(), t)).collect();

    let mut names: Vec<&str> = source_map
        .keys()
        .chain(target_map.keys())
        .copied()
        .collect();
    names.sort();
    names.dedup();

    let mut diffs = Vec::new();
    for table_name in names {
        let src = source_map.get(table_name).copied();
        let tgt = target_map.get(table_name).copied();
        match (src, tgt) {
            (None, Some(tgt_table)) => diffs.push(TableDiff {
                name: table_name.to_owned(),
                status: DiffStatus::Added,
                column_diffs: tgt_table
                    .columns
                    .iter()
                    .map(|c| ColumnDiff {
                        name: c.column_name.clone(),
                        status: DiffStatus::Added,
                        before: None,
                        after: Some(c.clone()),
                    })
                    .collect(),
                constraint_diffs: tgt_table
                    .constraints
                    .iter()
                    .map(|c| ConstraintDiff {
                        name: c.name.clone(),
                        status: DiffStatus::Added,
                        before: None,
                        after: Some(c.clone()),
                    })
                    .collect(),
                index_diffs: tgt_table
                    .indexes
                    .iter()
                    .map(|i| IndexDiff {
                        name: i.name.clone(),
                        status: DiffStatus::Added,
                        before: None,
                        after: Some(i.clone()),
                    })
                    .collect(),
            }),
            (Some(src_table), None) => diffs.push(TableDiff {
                name: table_name.to_owned(),
                status: DiffStatus::Removed,
                column_diffs: src_table
                    .columns
                    .iter()
                    .map(|c| ColumnDiff {
                        name: c.column_name.clone(),
                        status: DiffStatus::Removed,
                        before: Some(c.clone()),
                        after: None,
                    })
                    .collect(),
                constraint_diffs: src_table
                    .constraints
                    .iter()
                    .map(|c| ConstraintDiff {
                        name: c.name.clone(),
                        status: DiffStatus::Removed,
                        before: Some(c.clone()),
                        after: None,
                    })
                    .collect(),
                index_diffs: src_table
                    .indexes
                    .iter()
                    .map(|i| IndexDiff {
                        name: i.name.clone(),
                        status: DiffStatus::Removed,
                        before: Some(i.clone()),
                        after: None,
                    })
                    .collect(),
            }),
            (Some(src_table), Some(tgt_table)) => {
                let column_diffs = diff_columns(&src_table.columns, &tgt_table.columns);
                let constraint_diffs =
                    diff_constraints(&src_table.constraints, &tgt_table.constraints);
                let index_diffs = diff_indexes(&src_table.indexes, &tgt_table.indexes);
                let has_changes = column_diffs
                    .iter()
                    .any(|d| d.status != DiffStatus::Unchanged)
                    || constraint_diffs
                        .iter()
                        .any(|d| d.status != DiffStatus::Unchanged)
                    || index_diffs
                        .iter()
                        .any(|d| d.status != DiffStatus::Unchanged);
                diffs.push(TableDiff {
                    name: table_name.to_owned(),
                    status: if has_changes {
                        DiffStatus::Changed
                    } else {
                        DiffStatus::Unchanged
                    },
                    column_diffs,
                    constraint_diffs,
                    index_diffs,
                });
            }
            (None, None) => {}
        }
    }

    let order = |s: DiffStatus| match s {
        DiffStatus::Changed => 0,
        DiffStatus::Added => 1,
        DiffStatus::Removed => 2,
        DiffStatus::Unchanged => 3,
    };
    diffs.sort_by_key(|d| order(d.status));
    diffs
}

fn diff_columns(src: &[ColumnSnapshot], tgt: &[ColumnSnapshot]) -> Vec<ColumnDiff> {
    let src_map: std::collections::HashMap<&str, &ColumnSnapshot> =
        src.iter().map(|c| (c.column_name.as_str(), c)).collect();
    let tgt_map: std::collections::HashMap<&str, &ColumnSnapshot> =
        tgt.iter().map(|c| (c.column_name.as_str(), c)).collect();
    let mut diffs = Vec::new();
    for (name, src_col) in &src_map {
        match tgt_map.get(name) {
            None => diffs.push(ColumnDiff {
                name: (*name).to_owned(),
                status: DiffStatus::Removed,
                before: Some((*src_col).clone()),
                after: None,
            }),
            Some(tgt_col) => {
                let changed = src_col.data_type != tgt_col.data_type
                    || src_col.not_null != tgt_col.not_null
                    || src_col.default_value.as_deref().unwrap_or("")
                        != tgt_col.default_value.as_deref().unwrap_or("");
                diffs.push(ColumnDiff {
                    name: (*name).to_owned(),
                    status: if changed {
                        DiffStatus::Changed
                    } else {
                        DiffStatus::Unchanged
                    },
                    before: Some((*src_col).clone()),
                    after: Some((*tgt_col).clone()),
                });
            }
        }
    }
    for (name, tgt_col) in &tgt_map {
        if !src_map.contains_key(name) {
            diffs.push(ColumnDiff {
                name: (*name).to_owned(),
                status: DiffStatus::Added,
                before: None,
                after: Some((*tgt_col).clone()),
            });
        }
    }
    diffs
}

fn diff_constraints(src: &[ConstraintSnapshot], tgt: &[ConstraintSnapshot]) -> Vec<ConstraintDiff> {
    let src_map: std::collections::HashMap<&str, &ConstraintSnapshot> =
        src.iter().map(|c| (c.name.as_str(), c)).collect();
    let tgt_map: std::collections::HashMap<&str, &ConstraintSnapshot> =
        tgt.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut diffs = Vec::new();
    for (name, src_con) in &src_map {
        match tgt_map.get(name) {
            None => diffs.push(ConstraintDiff {
                name: (*name).to_owned(),
                status: DiffStatus::Removed,
                before: Some((*src_con).clone()),
                after: None,
            }),
            Some(tgt_con) => {
                let changed = src_con.definition != tgt_con.definition;
                diffs.push(ConstraintDiff {
                    name: (*name).to_owned(),
                    status: if changed {
                        DiffStatus::Changed
                    } else {
                        DiffStatus::Unchanged
                    },
                    before: Some((*src_con).clone()),
                    after: Some((*tgt_con).clone()),
                });
            }
        }
    }
    for (name, tgt_con) in &tgt_map {
        if !src_map.contains_key(name) {
            diffs.push(ConstraintDiff {
                name: (*name).to_owned(),
                status: DiffStatus::Added,
                before: None,
                after: Some((*tgt_con).clone()),
            });
        }
    }
    diffs
}

fn diff_indexes(src: &[IndexSnapshot], tgt: &[IndexSnapshot]) -> Vec<IndexDiff> {
    let src_map: std::collections::HashMap<&str, &IndexSnapshot> =
        src.iter().map(|i| (i.name.as_str(), i)).collect();
    let tgt_map: std::collections::HashMap<&str, &IndexSnapshot> =
        tgt.iter().map(|i| (i.name.as_str(), i)).collect();
    let mut diffs = Vec::new();
    for (name, src_idx) in &src_map {
        match tgt_map.get(name) {
            None => diffs.push(IndexDiff {
                name: (*name).to_owned(),
                status: DiffStatus::Removed,
                before: Some((*src_idx).clone()),
                after: None,
            }),
            Some(tgt_idx) => {
                let changed = src_idx.definition != tgt_idx.definition;
                diffs.push(IndexDiff {
                    name: (*name).to_owned(),
                    status: if changed {
                        DiffStatus::Changed
                    } else {
                        DiffStatus::Unchanged
                    },
                    before: Some((*src_idx).clone()),
                    after: Some((*tgt_idx).clone()),
                });
            }
        }
    }
    for (name, tgt_idx) in &tgt_map {
        if !src_map.contains_key(name) {
            diffs.push(IndexDiff {
                name: (*name).to_owned(),
                status: DiffStatus::Added,
                before: None,
                after: Some((*tgt_idx).clone()),
            });
        }
    }
    diffs
}

/// Migrate **source** schema toward **target** (source = current, target = desired).
pub fn build_migration_statements(
    source_schema: &str,
    target_schema: &str,
    diffs: &[TableDiff],
) -> Vec<String> {
    let mut stmts = Vec::new();
    for table in diffs {
        if table.status == DiffStatus::Unchanged {
            continue;
        }
        if table.status == DiffStatus::Added {
            let cols: Vec<String> = table
                .column_diffs
                .iter()
                .filter(|c| c.status == DiffStatus::Added)
                .filter_map(|c| c.after.as_ref())
                .map(|c| {
                    let nn = if c.not_null { " NOT NULL" } else { "" };
                    let def = c
                        .default_value
                        .as_ref()
                        .map(|d| format!(" DEFAULT {d}"))
                        .unwrap_or_default();
                    format!("  \"{}\" {}{}{}", c.column_name, c.data_type, nn, def)
                })
                .collect();
            stmts.push(format!(
                "-- Table added in {target_schema}\nCREATE TABLE \"{source_schema}\".\"{}\" (\n{}\n);",
                table.name,
                cols.join(",\n")
            ));
            continue;
        }
        if table.status == DiffStatus::Removed {
            stmts.push(format!(
                "-- Table removed in {target_schema}\n-- DROP TABLE \"{source_schema}\".\"{}\"; -- Uncomment to drop",
                table.name
            ));
            continue;
        }

        stmts.push(format!("-- Changes for table: {}", table.name));
        for col in &table.column_diffs {
            match col.status {
                DiffStatus::Added => {
                    if let Some(after) = &col.after {
                        let nn = if after.not_null { " NOT NULL" } else { "" };
                        let def = after
                            .default_value
                            .as_ref()
                            .map(|d| format!(" DEFAULT {d}"))
                            .unwrap_or_default();
                        stmts.push(format!(
                            "ALTER TABLE \"{source_schema}\".\"{}\"\n  ADD COLUMN \"{}\" {}{}{};",
                            table.name, col.name, after.data_type, nn, def
                        ));
                    }
                }
                DiffStatus::Removed => stmts.push(format!(
                    "-- ALTER TABLE \"{source_schema}\".\"{}\"\n--   DROP COLUMN \"{}\"; -- Uncomment to drop",
                    table.name, col.name
                )),
                DiffStatus::Changed => {
                    if let (Some(before), Some(after)) = (&col.before, &col.after) {
                        if before.data_type != after.data_type {
                            stmts.push(format!(
                                "ALTER TABLE \"{source_schema}\".\"{}\"\n  ALTER COLUMN \"{}\" TYPE {};",
                                table.name, col.name, after.data_type
                            ));
                        }
                        if before.not_null != after.not_null {
                            let op = if after.not_null { "SET" } else { "DROP" };
                            stmts.push(format!(
                                "ALTER TABLE \"{source_schema}\".\"{}\"\n  ALTER COLUMN \"{}\" {op} NOT NULL;",
                                table.name, col.name
                            ));
                        }
                        let before_def = before.default_value.as_deref().unwrap_or("");
                        let after_def = after.default_value.as_deref().unwrap_or("");
                        if before_def != after_def {
                            if after_def.is_empty() {
                                stmts.push(format!(
                                    "ALTER TABLE \"{source_schema}\".\"{}\"\n  ALTER COLUMN \"{}\" DROP DEFAULT;",
                                    table.name, col.name
                                ));
                            } else {
                                stmts.push(format!(
                                    "ALTER TABLE \"{source_schema}\".\"{}\"\n  ALTER COLUMN \"{}\" SET DEFAULT {after_def};",
                                    table.name, col.name
                                ));
                            }
                        }
                    }
                }
                DiffStatus::Unchanged => {}
            }
        }
        for con in &table.constraint_diffs {
            match con.status {
                DiffStatus::Added => {
                    if let Some(after) = &con.after {
                        stmts.push(format!(
                            "ALTER TABLE \"{source_schema}\".\"{}\"\n  ADD CONSTRAINT \"{}\" {};",
                            table.name, con.name, after.definition
                        ));
                    }
                }
                DiffStatus::Removed => stmts.push(format!(
                    "-- ALTER TABLE \"{source_schema}\".\"{}\"\n--   DROP CONSTRAINT \"{}\"; -- Uncomment to drop",
                    table.name, con.name
                )),
                _ => {}
            }
        }
        for idx in &table.index_diffs {
            match idx.status {
                DiffStatus::Added => {
                    if let Some(after) = &idx.after {
                        let rewritten = after.definition.replace(
                            &format!("ON {target_schema}."),
                            &format!("ON {source_schema}."),
                        );
                        stmts.push(format!("{rewritten};"));
                    }
                }
                DiffStatus::Removed => {
                    stmts.push(format!(
                        "-- DROP INDEX \"{}\"; -- Uncomment to drop",
                        idx.name
                    ));
                }
                _ => {}
            }
        }
    }
    stmts
}

pub fn diffs_to_json(diffs: &[TableDiff]) -> Value {
    json!(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str) -> ColumnSnapshot {
        ColumnSnapshot {
            column_name: name.into(),
            data_type: ty.into(),
            not_null: false,
            default_value: None,
            ordinal: 1,
        }
    }

    #[test]
    fn detects_added_table() {
        let source = SchemaSnapshot { tables: vec![] };
        let target = SchemaSnapshot {
            tables: vec![TableSnapshot {
                name: "orders".into(),
                schema: "public".into(),
                columns: vec![col("id", "integer")],
                constraints: vec![],
                indexes: vec![],
            }],
        };
        let diffs = compute_schema_diff(&source, &target);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, DiffStatus::Added);
        let stmts = build_migration_statements("public", "public", &diffs);
        assert!(stmts[0].contains("CREATE TABLE"));
    }

    #[test]
    fn detects_column_type_change() {
        let source = SchemaSnapshot {
            tables: vec![TableSnapshot {
                name: "t".into(),
                schema: "public".into(),
                columns: vec![col("n", "integer")],
                constraints: vec![],
                indexes: vec![],
            }],
        };
        let mut tgt_col = col("n", "bigint");
        tgt_col.ordinal = 1;
        let target = SchemaSnapshot {
            tables: vec![TableSnapshot {
                name: "t".into(),
                schema: "public".into(),
                columns: vec![tgt_col],
                constraints: vec![],
                indexes: vec![],
            }],
        };
        let diffs = compute_schema_diff(&source, &target);
        assert_eq!(diffs[0].status, DiffStatus::Changed);
        let stmts = build_migration_statements("public", "public", &diffs);
        assert!(stmts.iter().any(|s| s.contains("TYPE bigint")));
    }
}
