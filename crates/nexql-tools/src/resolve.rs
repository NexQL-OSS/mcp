// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! FK and enum value resolution for agent-facing tool output.

use std::collections::{HashMap, HashSet};

use deadpool_postgres::Object;
use nexql_index::model::{ColumnEntry, ForeignKeyEntry, ObjectEntry};
use nexql_index::{IndexQueryService, IndexStore, QueryPolicyFilter};
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::sql::{parse_ref, quote_ident, quote_ref};

pub const FK_RESOLVED_SUFFIX: &str = "__resolved";
pub const DEFAULT_RESOLVE_REFS_LIMIT: usize = 20;
pub const MAX_RESOLVE_FK_DISTINCT: usize = 50;

const LABEL_COLUMN_CANDIDATES: &[&str] = &["name", "title", "label", "display_name"];

/// Pick a human-readable label column on the referenced table.
pub fn pick_label_column(entry: &ObjectEntry) -> Option<String> {
    for candidate in LABEL_COLUMN_CANDIDATES {
        if entry.columns.iter().any(|c| c.name == *candidate) {
            return Some((*candidate).to_string());
        }
    }
    entry
        .columns
        .iter()
        .find(|c| {
            let tn = c.type_name.to_ascii_lowercase();
            (tn.contains("text") || tn.contains("character") || tn == "varchar")
                && c.is_pk != Some(true)
        })
        .map(|c| c.name.clone())
}

fn fk_for_column(entry: &ObjectEntry, col: &str) -> Option<(ForeignKeyEntry, usize)> {
    let fks = entry.foreign_keys.as_ref()?;
    for fk in fks {
        if let Some(idx) = fk.columns.iter().position(|c| c == col) {
            return Some((fk.clone(), idx));
        }
    }
    None
}

fn enum_values_for_column(
    col: &ColumnEntry,
    enum_index: &HashMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    let type_name = col.type_name.split('(').next()?.trim();
    let bare = type_name.rsplit('.').next()?.trim();
    enum_index
        .get(bare)
        .or_else(|| enum_index.get(type_name))
        .cloned()
}

/// Build enum type name → values map from index shards.
pub fn build_enum_index(
    store: &IndexStore,
    base: &std::path::Path,
    manifest: &nexql_index::model::IndexManifest,
) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    for shard in &manifest.shards {
        let Ok(Some(entries)) = store.read_shard_entries(base, &shard.file) else {
            continue;
        };
        for (ref_, entry) in entries {
            if entry.kind == nexql_index::model::DbObjectKind::Enum
                && let Some(values) = &entry.values
            {
                out.insert(ref_.rsplit('.').next().unwrap_or(&ref_).to_string(), values.clone());
                out.insert(ref_.clone(), values.clone());
            }
        }
    }
    out
}

pub async fn batch_resolve_fk_values(
    client: &Object,
    ref_table: &str,
    ref_col: &str,
    label_col: Option<&str>,
    ids: &[String],
    limit: usize,
) -> Result<HashMap<String, String>, ToolError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let (schema, table) = parse_ref(ref_table).map_err(ToolError::InvalidArgs)?;
    let id_list: Vec<&str> = ids.iter().map(String::as_str).collect();
    let sql = if let Some(label) = label_col {
        format!(
            "SELECT {}::text AS id, {}::text AS label FROM {} WHERE {}::text = ANY($1) LIMIT {}",
            quote_ident(ref_col),
            quote_ident(label),
            quote_ref(&schema, &table),
            quote_ident(ref_col),
            limit
        )
    } else {
        format!(
            "SELECT {}::text AS id, {}::text AS id FROM {} WHERE {}::text = ANY($1) LIMIT {}",
            quote_ident(ref_col),
            quote_ident(ref_col),
            quote_ref(&schema, &table),
            quote_ident(ref_col),
            limit
        )
    };
    let rows = client
        .query(&sql, &[&id_list])
        .await
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    let mut out = HashMap::new();
    for row in rows {
        let id: String = row.get(0);
        let label: String = row.get(1);
        out.insert(id, label);
    }
    Ok(out)
}

/// Resolve FK label columns on run_select columnar output.
pub async fn resolve_fks_on_payload(
    payload: &mut Value,
    store: &IndexStore,
    connection_id: &str,
    database: &str,
    table_refs: &[nexql_policy::ObjectRef],
    filter: &QueryPolicyFilter,
    client: &Object,
) -> Result<(), ToolError> {
    let base = store.base_dir(connection_id, database);
    let Some(manifest) = store.read_manifest(&base)? else {
        return Ok(());
    };

    let mut fk_columns: Vec<(String, ForeignKeyEntry, usize, String)> = Vec::new();
    for table in table_refs {
        let Some(entry) = store.get_object_entry(&base, &manifest, &table.schema, &table.name)?
        else {
            continue;
        };
        let Some(fks) = &entry.foreign_keys else {
            continue;
        };
        for fk in fks {
            for (idx, col) in fk.columns.iter().enumerate() {
                if filter.is_pii_column(&table.schema, &table.name, col) {
                    continue;
                }
                let ref_col = fk.ref_columns.get(idx).cloned().unwrap_or_else(|| "id".into());
                let ref_entry = parse_ref(&fk.ref_table)
                    .ok()
                    .and_then(|(s, n)| store.get_object_entry(&base, &manifest, &s, &n).ok())
                    .flatten();
                let label = ref_entry
                    .as_ref()
                    .and_then(pick_label_column)
                    .unwrap_or(ref_col.clone());
                fk_columns.push((col.clone(), fk.clone(), idx, label));
            }
        }
    }

    let obj = payload.as_object_mut().ok_or_else(|| {
        ToolError::Execution("run_select payload must be an object".into())
    })?;
    let col_names: Vec<String> = obj
        .get("columns")
        .and_then(|v| v.as_array())
        .map(|cols| {
            cols.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut rows = obj
        .get("rows")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut col_names = col_names;

    for (fk_col, fk, ref_idx, label_col) in fk_columns {
        let Some(col_idx) = col_names.iter().position(|c| c == &fk_col) else {
            continue;
        };
        let mut distinct = HashSet::new();
        for row in &rows {
            if let Some(cell) = row.get(col_idx)
                && !cell.is_null()
            {
                distinct.insert(value_to_key(cell));
            }
        }
        if distinct.is_empty() || distinct.len() > MAX_RESOLVE_FK_DISTINCT {
            continue;
        }
        let ids: Vec<String> = distinct.into_iter().collect();
        let ref_col = fk.ref_columns.get(ref_idx).cloned().unwrap_or_else(|| "id".into());
        let resolved = batch_resolve_fk_values(
            client,
            &fk.ref_table,
            &ref_col,
            Some(&label_col),
            &ids,
            DEFAULT_RESOLVE_REFS_LIMIT.max(ids.len()),
        )
        .await?;
        let resolved_col = format!("{fk_col}{FK_RESOLVED_SUFFIX}");
        if !col_names.contains(&resolved_col) {
            col_names.push(resolved_col.clone());
        }
        let resolved_idx = col_names
            .iter()
            .position(|c| c == &resolved_col)
            .unwrap_or(col_names.len().saturating_sub(1));
        for row in rows.iter_mut() {
            let key = row
                .get(col_idx)
                .map(value_to_key)
                .unwrap_or_default();
            let label = resolved.get(&key).cloned().unwrap_or_default();
            if let Some(row_arr) = row.as_array_mut() {
                if row_arr.len() <= resolved_idx {
                    row_arr.resize(resolved_idx + 1, Value::Null);
                }
                row_arr[resolved_idx] = json!(label);
            }
        }
    }

    obj.insert(
        "columns".into(),
        json!(col_names.iter().map(|c| json!(c)).collect::<Vec<_>>()),
    );
    obj.insert("rows".into(), json!(rows));
    Ok(())
}

fn value_to_key(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Load ref-table entries for describe_object FK label resolution.
pub fn label_column_for_fk(
    store: &IndexStore,
    base: &std::path::Path,
    manifest: &nexql_index::model::IndexManifest,
    fk: &ForeignKeyEntry,
) -> Option<String> {
    let (schema, name) = parse_ref(&fk.ref_table).ok()?;
    let entry = store
        .get_object_entry(base, manifest, &schema, &name)
        .ok()??;
    pick_label_column(&entry).or_else(|| fk.ref_columns.first().cloned())
}

pub async fn enrich_describe_object_with_store(
    mut value: Value,
    entry: &ObjectEntry,
    store: &IndexStore,
    svc: &IndexQueryService<'_>,
    resolve_refs: bool,
    resolve_limit: usize,
    client: Option<&Object>,
) -> Result<Value, ToolError> {
    let base = svc.base_dir();
    let manifest = store
        .read_manifest(&base)?
        .ok_or_else(|| ToolError::Execution("index manifest missing".into()))?;
    let enum_index = build_enum_index(store, &base, &manifest);

    let Some(columns) = value.get_mut("columns").and_then(|v| v.as_array_mut()) else {
        return Ok(value);
    };
    for col_val in columns.iter_mut() {
        let Some(col_obj) = col_val.as_object_mut() else {
            continue;
        };
        let col_name = col_obj
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let col_entry = entry.columns.iter().find(|c| c.name == col_name);

        if let Some(ce) = col_entry
            && let Some(vals) = enum_values_for_column(ce, &enum_index)
        {
            col_obj.insert("enumValues".into(), json!(vals));
        }

        if let Some((fk, ref_idx)) = fk_for_column(entry, &col_name) {
            let ref_col = fk
                .ref_columns
                .get(ref_idx)
                .cloned()
                .unwrap_or_else(|| "id".into());
            let label = label_column_for_fk(store, &base, &manifest, &fk).unwrap_or(ref_col.clone());
            let mut fk_meta = json!({
                "refTable": fk.ref_table,
                "refColumns": fk.ref_columns,
                "labelColumn": label,
            });
            if fk.inferred == Some(true) {
                fk_meta["inferred"] = json!(true);
            }
            col_obj.insert("fk".into(), fk_meta);

            if resolve_refs {
                let mut resolved: HashMap<String, String> = HashMap::new();
                if let Some(ce) = col_entry
                    && let Some(profile) = &ce.profile
                    && let Some(common) = &profile.common_values
                {
                    for v in common.iter().take(resolve_limit) {
                        resolved.insert(v.clone(), v.clone());
                    }
                }
                if resolved.len() < resolve_limit
                    && let Some(c) = client
                {
                    let ids: Vec<String> = resolved.keys().cloned().collect();
                    let sample_ids = if ids.is_empty() {
                        Vec::new()
                    } else {
                        ids
                    };
                    if !sample_ids.is_empty()
                        && let Ok(live) = batch_resolve_fk_values(
                            c,
                            &fk.ref_table,
                            &ref_col,
                            Some(&label),
                            &sample_ids,
                            resolve_limit,
                        )
                        .await
                    {
                        for (k, v) in live {
                            if v != k {
                                resolved.insert(k, v);
                            }
                        }
                    }
                }
                if !resolved.is_empty() {
                    col_obj.insert("resolvedValues".into(), json!(resolved));
                }
            }
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexql_index::model::{ColumnEntry, DbObjectKind};

    fn sample_entry() -> ObjectEntry {
        ObjectEntry {
            kind: DbObjectKind::Table,
            oid: 1,
            object_hash: "h".into(),
            comment: None,
            row_estimate: 0.0,
            size_bytes: 0,
            columns: vec![
                ColumnEntry {
                    name: "name".into(),
                    type_name: "text".into(),
                    not_null: true,
                    default_value: None,
                    comment: None,
                    ordinal: 1,
                    is_pk: None,
                    profile: None,
                    pii: None,
                },
                ColumnEntry {
                    name: "title".into(),
                    type_name: "varchar".into(),
                    not_null: false,
                    default_value: None,
                    comment: None,
                    ordinal: 2,
                    is_pk: None,
                    profile: None,
                    pii: None,
                },
            ],
            primary_key: None,
            foreign_keys: None,
            indexes: None,
            checks: None,
            excluded: None,
            definition: None,
            signature: None,
            language: None,
            volatility: None,
            body: None,
            values: None,
            base_type: None,
            constraint: None,
        }
    }

    #[test]
    fn pick_label_column_prefers_name() {
        assert_eq!(pick_label_column(&sample_entry()), Some("name".into()));
    }

    #[test]
    fn pick_label_column_falls_back_to_title() {
        let mut entry = sample_entry();
        entry.columns.retain(|c| c.name != "name");
        assert_eq!(pick_label_column(&entry), Some("title".into()));
    }
}
