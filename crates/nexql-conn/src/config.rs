// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

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

/// Project-level configuration (`.nexql/config.toml` or `.nexql-mcp.toml`).
/// Security: may select profiles and tighten policy only.
/// Credentials, URLs, and loosening operations are rejected.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct ProjectConfigFile {
    /// Selects a profile from the user's global config.
    pub default_profile: Option<String>,
    /// Restrict access mode (can only tighten, e.g. "write" -> "read").
    pub access_mode: Option<String>,
    /// Additional schema restrictions.
    #[serde(default)]
    pub schemas: Vec<String>,
    /// Additional schema denials.
    #[serde(default)]
    pub deny_schemas: Vec<String>,
    /// Additional table denials.
    #[serde(default)]
    pub deny_tables: Vec<String>,
    /// Additional PII columns.
    #[serde(default)]
    pub pii_columns: Vec<String>,
    /// Maximum rows (can only lower, not raise).
    pub max_rows: Option<u32>,
    /// Optional project-local index storage directory (relative to `.nexql/`).
    pub index_dir: Option<String>,
}

const PROJECT_FORBIDDEN_KEYS: &[&str] = &[
    "url",
    "host",
    "port",
    "dbname",
    "user",
    "password",
    "password_command",
    "password_file",
    "sslcert",
    "sslkey",
    "sslrootcert",
    "credential_provider",
];

/// Load and sanitize a project config file.
/// Returns the parsed config and a list of security warnings for stripped fields.
pub fn load_project_config(path: &Path) -> Result<(ProjectConfigFile, Vec<String>), ConnError> {
    let raw = std::fs::read_to_string(path)?;
    let raw_value: toml::Value = toml::from_str(&raw)
        .map_err(|e| ConnError::Config(format!("Failed to parse project TOML: {e}")))?;

    let mut warnings = Vec::new();

    if let Some(table) = raw_value.as_table() {
        for &key in PROJECT_FORBIDDEN_KEYS {
            if table.contains_key(key) {
                warnings.push(format!(
                    "security: project config '{}' contains forbidden field '{}' — stripped",
                    path.display(),
                    key
                ));
            }
        }
        if table.contains_key("profiles") {
            warnings.push(format!(
                "security: project config '{}' contains [profiles] section — project configs cannot define connection profiles, only select them via default_profile",
                path.display()
            ));
        }
    }

    let config: ProjectConfigFile = toml::from_str(&raw)
        .map_err(|e| ConnError::Config(format!("Failed to deserialize project config: {e}")))?;

    Ok((config, warnings))
}

/// Search for `.nexql/config.toml` or `.nexql-mcp.toml` ascending from `start_dir`.
/// Stops at filesystem root or a directory containing `.git`.
pub fn find_project_config(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = start_dir.to_path_buf();
    loop {
        let candidate = dir.join(".nexql").join("config.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        let flat = dir.join(".nexql-mcp.toml");
        if flat.is_file() {
            return Some(flat);
        }
        if dir.join(".git").exists() {
            return None;
        }
        if !dir.pop() {
            return None;
        }
    }
}

impl ProjectConfigFile {
    /// Return the more restrictive of two access modes.
    pub fn tighten_access_mode(&self, base: &str) -> String {
        let Some(ref project_mode) = self.access_mode else {
            return base.to_string();
        };
        let rank = |m: &str| match m.to_lowercase().as_str() {
            "admin" => 3,
            "write" => 2,
            "read" | "read_only" => 1,
            _ => 0,
        };
        if rank(project_mode) < rank(base) {
            project_mode.clone()
        } else {
            base.to_string()
        }
    }
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

    /// Export a secret-sanitized ProjectConfigFile suitable for `.nexql/config.toml`.
    /// Policy fields are taken from `default_profile` when set, otherwise the first profile.
    pub fn export_shareable(&self) -> ProjectConfigFile {
        let policy_source = self
            .default_profile
            .as_ref()
            .and_then(|name| self.profiles.get(name))
            .or_else(|| self.profiles.values().next());
        ProjectConfigFile {
            default_profile: self.default_profile.clone(),
            access_mode: policy_source.and_then(|p| p.access_mode.clone()),
            schemas: policy_source
                .map(|p| p.schemas.clone())
                .unwrap_or_default(),
            deny_schemas: policy_source
                .map(|p| p.deny_schemas.clone())
                .unwrap_or_default(),
            deny_tables: policy_source
                .map(|p| p.deny_tables.clone())
                .unwrap_or_default(),
            pii_columns: policy_source
                .map(|p| p.pii_columns.clone())
                .unwrap_or_default(),
            max_rows: policy_source.and_then(|p| p.max_rows),
            index_dir: None,
        }
    }

    /// Export a full config with secrets stripped, preserving profile structure.
    pub fn export_full_sanitized(&self) -> Self {
        let mut sanitized = self.clone();
        for profile in sanitized.profiles.values_mut() {
            profile.password = None;
            profile.password_command = None;
            profile.password_file = None;
            profile.credential_provider = None;
            if let Some(ref url) = profile.url
                && let Ok(mut parsed) = url::Url::parse(url)
                && (parsed.password().is_some() || !parsed.username().is_empty())
            {
                let _ = parsed.set_username("");
                let _ = parsed.set_password(None);
                profile.url = Some(parsed.to_string());
            }
        }
        sanitized
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

    #[test]
    fn find_project_config_ascending() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        let nexql_dir = root.path().join(".nexql");
        std::fs::create_dir_all(&nexql_dir).unwrap();
        let config_path = nexql_dir.join("config.toml");
        std::fs::write(&config_path, "default_profile = \"staging\"\n").unwrap();

        let found = find_project_config(&nested);
        assert_eq!(found, Some(config_path));
    }

    #[test]
    fn find_project_config_stops_at_git() {
        let root = tempfile::tempdir().unwrap();
        let git_dir = root.path().join("repo").join(".git");
        let nested = root.path().join("repo").join("sub");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        let outer_nexql = root.path().join(".nexql");
        std::fs::create_dir_all(&outer_nexql).unwrap();
        std::fs::write(
            outer_nexql.join("config.toml"),
            "default_profile = \"root\"\n",
        )
        .unwrap();

        let found = find_project_config(&nested);
        assert!(found.is_none());
    }

    #[test]
    fn project_config_strips_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
default_profile = "staging"
password = "hacked"
host = "attacker.com"
access_mode = "read"
deny_tables = ["users.*"]
"#,
        )
        .unwrap();

        let (cfg, warnings) = load_project_config(&path).unwrap();
        assert_eq!(cfg.default_profile.as_deref(), Some("staging"));
        assert_eq!(cfg.access_mode.as_deref(), Some("read"));
        assert_eq!(cfg.deny_tables, vec!["users.*"]);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("forbidden field 'password'"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("forbidden field 'host'"))
        );
    }

    #[test]
    fn project_config_rejects_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
default_profile = "staging"

[profiles.evil]
url = "postgres://evil.com/db"
"#,
        )
        .unwrap();

        let (cfg, warnings) = load_project_config(&path).unwrap();
        assert_eq!(cfg.default_profile.as_deref(), Some("staging"));
        assert!(warnings.iter().any(|w| w.contains("[profiles] section")));
    }

    #[test]
    fn access_mode_tightens_only() {
        let proj = ProjectConfigFile {
            access_mode: Some("read".into()),
            ..Default::default()
        };
        assert_eq!(proj.tighten_access_mode("write"), "read");
        assert_eq!(proj.tighten_access_mode("admin"), "read");

        let proj2 = ProjectConfigFile {
            access_mode: Some("admin".into()),
            ..Default::default()
        };
        assert_eq!(proj2.tighten_access_mode("read"), "read");
    }

    #[test]
    fn export_shareable_includes_default_profile_policy() {
        let mut cfg = ConfigFile {
            default_profile: Some("team".into()),
            ..Default::default()
        };
        cfg.upsert_profile(
            "team",
            ProfileConfig {
                deny_schemas: vec!["auth".into()],
                pii_columns: vec!["public.users.ssn".into()],
                max_rows: Some(500),
                ..Default::default()
            },
        );
        let proj = cfg.export_shareable();
        assert_eq!(proj.default_profile.as_deref(), Some("team"));
        assert_eq!(proj.deny_schemas, vec!["auth"]);
        assert_eq!(proj.pii_columns, vec!["public.users.ssn"]);
        assert_eq!(proj.max_rows, Some(500));
    }

    #[test]
    fn export_full_sanitized_strips_passwords() {
        let mut cfg = ConfigFile::default();
        cfg.upsert_profile(
            "prod",
            ProfileConfig {
                url: Some("postgres://user:secret@prod.host:5432/appdb".into()),
                password: Some("secret".into()),
                password_command: Some("op read ...".into()),
                ..Default::default()
            },
        );
        let exported = cfg.export_full_sanitized();
        let p = &exported.profiles["prod"];
        assert!(p.password.is_none());
        assert!(p.password_command.is_none());
        assert_eq!(p.url.as_deref(), Some("postgres://prod.host:5432/appdb"));
    }
}
