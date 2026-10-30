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

/// Why writes are off for a connection, when they are: write/admin against a
/// superuser needs the explicit override. Callers run such a connection
/// read-only instead of refusing to start, because a one-click install that
/// ticked "allow changes" has no way to pass a flag or read stderr. The text
/// is shown to the model so it can tell the user.
pub fn superuser_write_notice(
    mode: AccessMode,
    is_superuser: bool,
    override_flag: bool,
) -> Option<String> {
    (is_superuser && mode.allows_writes() && !override_flag).then(|| {
        "Changes to data are switched off: this connection logs in as a database superuser, \
and nexql-mcp only allows writes through a role that is not one. Tell the user. To allow \
changes, connect as a role that is not a superuser and has the write privileges it needs."
            .to_owned()
    })
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
    fn superuser_write_notice_only_without_override() {
        assert!(superuser_write_notice(AccessMode::Read, true, false).is_none());
        assert!(superuser_write_notice(AccessMode::Write, true, false).is_some());
        assert!(superuser_write_notice(AccessMode::Admin, true, false).is_some());
        assert!(superuser_write_notice(AccessMode::Write, true, true).is_none());
        assert!(superuser_write_notice(AccessMode::Write, false, false).is_none());
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
