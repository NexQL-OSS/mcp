//! TOML config — `~/.config/nexql-mcp/config.toml` or `$NEXQL_MCP_CONFIG`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ConnError;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ConfigFile {
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profiles: HashMap<String, ProfileConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
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
    pub sslcert: Option<String>,
    pub sslkey: Option<String>,
    pub sslrootcert: Option<String>,
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

    /// Insert or replace a profile, setting it as `default_profile` if none is set yet.
    pub fn upsert_profile(&mut self, name: impl Into<String>, profile: ProfileConfig) {
        let name = name.into();
        if self.default_profile.is_none() {
            self.default_profile = Some(name.clone());
        }
        self.profiles.insert(name, profile);
    }

    pub fn remove_profile(&mut self, name: &str) -> Option<ProfileConfig> {
        if self.default_profile.as_deref() == Some(name) {
            self.default_profile = self.profiles.keys().find(|k| *k != name).cloned();
        }
        self.profiles.remove(name)
    }

    pub fn to_toml_string(&self) -> Result<String, ConnError> {
        toml::to_string_pretty(self).map_err(|e| ConnError::Config(e.to_string()))
    }

    /// Write to `path`, backing up any existing file first. Atomic: writes to a
    /// sibling `.tmp` file then renames over the target (same filesystem).
    pub fn save(&self, path: &Path) -> Result<Option<PathBuf>, ConnError> {
        let rendered = self.to_toml_string()?;
        write_with_backup(path, &rendered)
    }
}

/// Write `content` to `path`, backing up any existing file first (sibling
/// `<path>.bak-<unix-ts>`) and writing atomically via a sibling `.tmp` file +
/// rename. Shared by `ConfigFile::save` and the TUI's client-config writer —
/// both need the same "never silently clobber, always leave a recovery copy"
/// guarantee.
pub fn write_with_backup(path: &Path, content: &str) -> Result<Option<PathBuf>, ConnError> {
    let backup = if path.exists() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut backup_os = path.as_os_str().to_os_string();
        backup_os.push(format!(".bak-{ts}"));
        let backup_path = PathBuf::from(backup_os);
        std::fs::copy(path, &backup_path)?;
        Some(backup_path)
    } else {
        None
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp_os = path.as_os_str().to_os_string();
    tmp_os.push(".tmp");
    let tmp_path = PathBuf::from(tmp_os);
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;

    Ok(backup)
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

    #[test]
    fn upsert_sets_default_profile_when_empty() {
        let mut cfg = ConfigFile::default();
        cfg.upsert_profile(
            "local",
            ProfileConfig {
                url: Some("postgres://dev@localhost:5432/appdb".into()),
                ..Default::default()
            },
        );
        assert_eq!(cfg.default_profile.as_deref(), Some("local"));
        assert!(cfg.profiles.contains_key("local"));
    }

    #[test]
    fn upsert_does_not_override_existing_default() {
        let mut cfg = ConfigFile {
            default_profile: Some("prod".into()),
            ..Default::default()
        };
        cfg.upsert_profile("local", ProfileConfig::default());
        assert_eq!(cfg.default_profile.as_deref(), Some("prod"));
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut cfg = ConfigFile::default();
        cfg.upsert_profile(
            "local",
            ProfileConfig {
                url: Some("postgres://dev@localhost:5432/appdb".into()),
                max_rows: Some(500),
                ..Default::default()
            },
        );
        let backup = cfg.save(&path).unwrap();
        assert!(backup.is_none(), "no prior file — no backup expected");

        let loaded = ConfigFile::load_path(&path).unwrap();
        assert_eq!(loaded.default_profile.as_deref(), Some("local"));
        assert_eq!(loaded.profiles["local"].max_rows, Some(500));
    }

    #[test]
    fn save_backs_up_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "default_profile = \"old\"\n").unwrap();

        let cfg = ConfigFile::default();
        let backup = cfg.save(&path).unwrap();
        let backup = backup.expect("existing file must be backed up");
        assert!(backup.exists());
        let backed_up = std::fs::read_to_string(&backup).unwrap();
        assert!(backed_up.contains("old"));
    }

    #[test]
    fn remove_profile_reassigns_default() {
        let mut cfg = ConfigFile::default();
        cfg.upsert_profile("local", ProfileConfig::default());
        cfg.upsert_profile("prod", ProfileConfig::default());
        cfg.default_profile = Some("local".into());

        cfg.remove_profile("local");
        assert_eq!(cfg.default_profile.as_deref(), Some("prod"));
        assert!(!cfg.profiles.contains_key("local"));
    }
}
