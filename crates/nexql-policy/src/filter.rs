// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Schema / table allow-deny and PII column filters.

use crate::error::PolicyError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectRef {
    pub schema: String,
    pub name: String,
}

impl ObjectRef {
    pub fn new(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            name: name.into(),
        }
    }

    pub fn qualified(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PolicyFilter {
    /// If non-empty, only these schemas are visible.
    pub allow_schemas: Vec<String>,
    pub deny_schemas: Vec<String>,
    /// Globs like `auth.*` or exact `public.users`.
    pub deny_tables: Vec<String>,
    /// `schema.table.column` entries excluded from sample/search.
    pub pii_columns: Vec<String>,
}

impl PolicyFilter {
    pub fn allows_schema(&self, schema: &str) -> bool {
        if self.deny_schemas.iter().any(|s| s == schema) {
            return false;
        }
        if self.allow_schemas.is_empty() {
            return true;
        }
        self.allow_schemas.iter().any(|s| s == schema)
    }

    pub fn allows_table(&self, schema: &str, table: &str) -> bool {
        if !self.allows_schema(schema) {
            return false;
        }
        !self
            .deny_tables
            .iter()
            .any(|g| table_glob_matches(g, schema, table))
    }

    pub fn filter_refs<'a>(
        &self,
        refs: impl IntoIterator<Item = &'a ObjectRef>,
    ) -> Vec<&'a ObjectRef> {
        refs.into_iter()
            .filter(|r| self.allows_table(&r.schema, &r.name))
            .collect()
    }

    pub fn require_table(&self, schema: &str, table: &str) -> Result<(), PolicyError> {
        if self.allows_table(schema, table) {
            Ok(())
        } else {
            Err(PolicyError::Denied(format!(
                "table {schema}.{table} is denied by policy"
            )))
        }
    }
}

/// Placeholder substituted for PII column values in query results.
pub const PII_REDACTED: &str = "<redacted>";

pub fn is_pii_column(pii: &[String], schema: &str, table: &str, column: &str) -> bool {
    let qualified = format!("{schema}.{table}.{column}");
    pii.iter().any(|p| p == &qualified)
}

/// True when `column` is flagged PII on any table referenced by the query.
pub fn column_matches_pii_policy(
    pii_columns: &[String],
    tables: &[ObjectRef],
    column: &str,
) -> bool {
    if pii_columns.is_empty() || tables.is_empty() {
        return false;
    }
    tables
        .iter()
        .any(|t| is_pii_column(pii_columns, &t.schema, &t.name, column))
}

fn table_glob_matches(glob: &str, schema: &str, table: &str) -> bool {
    if let Some((gs, gt)) = glob.split_once('.') {
        let schema_ok = gs == "*" || gs == schema;
        let table_ok = gt == "*" || gt == table;
        schema_ok && table_ok
    } else {
        glob == table || glob == "*"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_allow_deny() {
        let f = PolicyFilter {
            allow_schemas: vec!["public".into(), "billing".into()],
            deny_schemas: vec!["billing".into()],
            ..Default::default()
        };
        assert!(f.allows_schema("public"));
        assert!(!f.allows_schema("billing"));
        assert!(!f.allows_schema("auth"));
    }

    #[test]
    fn table_deny_glob() {
        let f = PolicyFilter {
            deny_tables: vec!["auth.*".into()],
            ..Default::default()
        };
        assert!(!f.allows_table("auth", "sessions"));
        assert!(f.allows_table("public", "users"));
    }

    #[test]
    fn pii_columns() {
        let pii = vec!["public.users.ssn".into(), "public.users.email".into()];
        assert!(is_pii_column(&pii, "public", "users", "ssn"));
        assert!(!is_pii_column(&pii, "public", "users", "id"));
    }

    #[test]
    fn column_matches_pii_policy_uses_query_tables() {
        let pii = vec!["public.users.ssn".into()];
        let tables = vec![ObjectRef::new("public", "users")];
        assert!(column_matches_pii_policy(&pii, &tables, "ssn"));
        assert!(!column_matches_pii_policy(&pii, &tables, "id"));
        assert!(!column_matches_pii_policy(&pii, &[], "ssn"));
    }

    #[test]
    fn filter_refs() {
        let f = PolicyFilter {
            deny_tables: vec!["auth.*".into()],
            ..Default::default()
        };
        let a = ObjectRef::new("public", "users");
        let b = ObjectRef::new("auth", "sessions");
        let kept = f.filter_refs([&a, &b]);
        assert_eq!(kept, vec![&a]);
    }
}
