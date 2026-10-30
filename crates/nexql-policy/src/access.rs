// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::PolicyError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    #[default]
    Read,
    Write,
    Admin,
}

impl FromStr for AccessMode {
    type Err = PolicyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "admin" => Ok(Self::Admin),
            other => Err(PolicyError::Denied(format!(
                "unknown access mode '{other}' (expected read|write|admin)"
            ))),
        }
    }
}

impl AccessMode {
    pub fn allows_writes(self) -> bool {
        matches!(self, Self::Write | Self::Admin)
    }

    pub fn allows_admin(self) -> bool {
        matches!(self, Self::Admin)
    }
}

/// Role created by `nexql-mcp profile readonly-role`.
pub const NEXQL_READONLY_ROLE: &str = "nexql_readonly";

/// Copy-paste SQL for a least-privilege read role (PostgreSQL 14+).
///
/// `pg_read_all_data` is the supported grant. The password placeholder must be
/// replaced before the statement is run.
pub fn readonly_role_sql() -> &'static str {
    "\
-- Least-privilege login for nexql-mcp (PostgreSQL 14+).
-- Replace the password, then point the profile at this role instead of a superuser.
CREATE ROLE nexql_readonly WITH LOGIN PASSWORD 'replace-with-a-secret';
GRANT pg_read_all_data TO nexql_readonly;"
}

/// Prominent warning when the connected role is a superuser.
pub fn superuser_warning() -> String {
    format!(
        "WARNING: connected as a superuser. A prompt-injected query can read server files \
(pg_read_file) and control the database server. Create role `{NEXQL_READONLY_ROLE}` \
(GRANT pg_read_all_data) and reconnect as that role. Copy-paste SQL: \
`nexql-mcp profile readonly-role`."
    )
}

/// Refuse write/admin against a superuser unless override is set.
pub fn check_superuser_guard(
    mode: AccessMode,
    is_superuser: bool,
    override_flag: bool,
) -> Result<(), PolicyError> {
    if is_superuser && mode.allows_writes() && !override_flag {
        return Err(PolicyError::Denied(
            "refusing write/admin mode against a superuser — pass --i-know-what-im-doing to override"
                .into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes() {
        assert_eq!("read".parse::<AccessMode>().unwrap(), AccessMode::Read);
        assert_eq!("WRITE".parse::<AccessMode>().unwrap(), AccessMode::Write);
        assert_eq!("admin".parse::<AccessMode>().unwrap(), AccessMode::Admin);
        assert!("nope".parse::<AccessMode>().is_err());
    }

    #[test]
    fn superuser_guard() {
        assert!(check_superuser_guard(AccessMode::Read, true, false).is_ok());
        assert!(check_superuser_guard(AccessMode::Write, true, false).is_err());
        assert!(check_superuser_guard(AccessMode::Write, true, true).is_ok());
        assert!(check_superuser_guard(AccessMode::Write, false, false).is_ok());
    }

    #[test]
    fn readonly_role_sql_is_copyable() {
        let sql = readonly_role_sql();
        assert!(sql.contains("CREATE ROLE nexql_readonly"));
        assert!(sql.contains("GRANT pg_read_all_data TO nexql_readonly"));
        assert!(superuser_warning().contains("WARNING"));
        assert!(superuser_warning().contains("nexql-mcp profile readonly-role"));
    }
}
