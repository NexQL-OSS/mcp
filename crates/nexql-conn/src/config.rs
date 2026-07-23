//! TOML config — `~/.config/nexql-mcp/config.toml` or `$NEXQL_MCP_CONFIG`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ConnError;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConfigFile {
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profiles: HashMap<String, ProfileConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProfileConfig {
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub dbname: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub password_command: Option<String>,
    pub password_file: Option<String>,
    pub sslmode: Option<String>,
    pub access_mode: Option<String>,
    #[serde(default)]
    pub schemas: Vec<String>,
    #[serde(default)]
    pub deny_schemas: Vec<String>,
    #[serde(default)]
    pub deny_tables: Vec<String>,
    #[serde(default)]
    pub pii_columns: Vec<String>,
    pub max_rows: Option<u32>,
    pub credential_provider: Option<String>,
}

impl ConfigFile {
    pub fn parse_str(s: &str) -> Result<Self, ConnError> {
        toml::from_str(s).map_err(|e| ConnError::Config(e.to_string()))
    }

    pub fn load_path(path: &Path) -> Result<Self, ConnError> {
        let raw = std::fs::read_to_string(path)?;
        Self::parse_str(&raw)
    }

    /// Resolve config path: `$NEXQL_MCP_CONFIG` → `~/.config/nexql-mcp/config.toml`.
    pub fn default_path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("NEXQL_MCP_CONFIG") {
            return Some(PathBuf::from(p));
        }
        dirs_config().map(|d| d.join("nexql-mcp").join("config.toml"))
    }
}

fn dirs_config() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example_shaped_config() {
        let cfg = ConfigFile::parse_str(
            r#"
default_profile = "local"

[profiles.local]
url = "postgres://dev@localhost:5432/appdb"
access_mode = "read"

[profiles.prod]
host = "prod.example.com"
dbname = "app"
user = "readonly_agent"
password_command = "op read op://vault/pg/password"
sslmode = "verify-full"
schemas = ["public", "billing"]
deny_tables = ["auth.*"]
pii_columns = ["public.users.ssn"]
max_rows = 200
"#,
        )
        .unwrap();
        assert_eq!(cfg.default_profile.as_deref(), Some("local"));
        assert_eq!(
            cfg.profiles["local"].url.as_deref(),
            Some("postgres://dev@localhost:5432/appdb")
        );
        assert_eq!(cfg.profiles["prod"].max_rows, Some(200));
        assert_eq!(cfg.profiles["prod"].deny_tables, vec!["auth.*"]);
    }
}
