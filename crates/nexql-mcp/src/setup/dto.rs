// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Request/response types for the setup web API.

use serde::{Deserialize, Serialize};

use nexql_conn::ProfileConfig;

use super::wire::{LaunchConfig, WireApplyResult, WirePreview};

#[derive(Debug, Deserialize)]
pub struct ProfileCreateRequest {
    pub name: String,
    #[serde(flatten)]
    pub upsert: ProfileUpsertRequest,
}

#[derive(Debug, Serialize)]
pub struct MetaResponse {
    pub version: String,
    pub config_path: String,
    pub default_profile: Option<String>,
    pub workspace_root: Option<String>,
    pub keyring_available: bool,
    pub password_storage: String,
    pub secrets_dir: Option<String>,
    pub installed_binary_version: Option<String>,
    pub launch_modes: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct ProfileListItem {
    pub name: String,
    pub is_default: bool,
    pub access_mode: String,
    pub host_or_url: String,
}

#[derive(Debug, Serialize)]
pub struct ProfileResponse {
    pub name: String,
    pub profile: SanitizedProfile,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SanitizedProfile {
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub dbname: Option<String>,
    pub user: Option<String>,
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
    pub statement_timeout_ms: Option<u32>,
    pub credential_provider: Option<String>,
    /// `None` keeps the profile default (heuristics enabled). `Some(false)`
    /// opts out and must survive a wizard save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitive_name_heuristics: Option<bool>,
    pub password_stored: bool,
    pub has_inline_password: bool,
}

impl From<&ProfileConfig> for SanitizedProfile {
    fn from(p: &ProfileConfig) -> Self {
        Self {
            url: p.url.clone(),
            host: p.host.clone(),
            port: p.port,
            dbname: p.dbname.clone(),
            user: p.user.clone(),
            password_command: p.password_command.clone(),
            password_file: p.password_file.clone(),
            sslmode: p.sslmode.clone(),
            sslcert: p.sslcert.clone(),
            sslkey: p.sslkey.clone(),
            sslrootcert: p.sslrootcert.clone(),
            access_mode: p.access_mode.clone(),
            schemas: p.schemas.clone(),
            deny_schemas: p.deny_schemas.clone(),
            deny_tables: p.deny_tables.clone(),
            pii_columns: p.pii_columns.clone(),
            max_rows: p.max_rows,
            statement_timeout_ms: p.statement_timeout_ms,
            credential_provider: p.credential_provider.clone(),
            sensitive_name_heuristics: p.sensitive_name_heuristics,
            password_stored: matches!(
                p.credential_provider.as_deref(),
                Some("keyring") | Some("os_keyring") | Some("encrypted_file")
            ),
            has_inline_password: p.password.is_some(),
        }
    }
}

impl From<SanitizedProfile> for ProfileConfig {
    fn from(s: SanitizedProfile) -> Self {
        ProfileConfig {
            url: s.url,
            host: s.host,
            port: s.port,
            dbname: s.dbname,
            user: s.user,
            password: None,
            password_command: s.password_command,
            password_file: s.password_file,
            sslmode: s.sslmode,
            sslcert: s.sslcert,
            sslkey: s.sslkey,
            sslrootcert: s.sslrootcert,
            access_mode: s.access_mode,
            schemas: s.schemas,
            deny_schemas: s.deny_schemas,
            deny_tables: s.deny_tables,
            pii_columns: s.pii_columns,
            max_rows: s.max_rows,
            statement_timeout_ms: s.statement_timeout_ms,
            credential_provider: s.credential_provider,
            sensitive_name_heuristics: s.sensitive_name_heuristics,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ProfileUpsertRequest {
    pub profile: SanitizedProfile,
    pub password: Option<String>,
    pub set_default: Option<bool>,
    pub confirm_elevated_access: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SetPasswordRequest {
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct TestStartResponse {
    pub job_id: String,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct TestStatusResponse {
    pub job_id: String,
    pub status: &'static str,
    pub server_version: Option<String>,
    pub is_superuser: Option<bool>,
    pub latency_ms: Option<f64>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClientsPreviewRequest {
    pub client_keys: Vec<String>,
    pub copy_only_keys: Vec<String>,
    pub profile_names: Vec<String>,
    pub launch: LaunchConfig,
}

#[derive(Debug, Deserialize)]
pub struct ClientsApplyRequest {
    pub preview: WirePreview,
    pub apply_keys: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ClientsApplyResponse {
    pub results: Vec<WireApplyResult>,
}

#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub imported: usize,
    pub backup: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExportResponse {
    pub format: String,
    pub toml: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub toml: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_name_heuristics_round_trips_through_the_wizard_dto() {
        let profile = ProfileConfig {
            sensitive_name_heuristics: Some(false),
            ..ProfileConfig::default()
        };
        let sanitized = SanitizedProfile::from(&profile);
        assert_eq!(sanitized.sensitive_name_heuristics, Some(false));
        let restored = ProfileConfig::from(sanitized);
        assert_eq!(restored.sensitive_name_heuristics, Some(false));

        let enabled = SanitizedProfile::from(&ProfileConfig::default());
        assert_eq!(enabled.sensitive_name_heuristics, None);
        let json = serde_json::to_string(&enabled).unwrap();
        let parsed: SanitizedProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.sensitive_name_heuristics, None);
        assert_eq!(ProfileConfig::from(parsed).sensitive_name_heuristics, None);

        let opted_out = SanitizedProfile {
            sensitive_name_heuristics: Some(false),
            ..SanitizedProfile::default()
        };
        let json = serde_json::to_string(&opted_out).unwrap();
        assert!(json.contains("sensitive_name_heuristics"));
        let parsed: SanitizedProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(
            ProfileConfig::from(parsed).sensitive_name_heuristics,
            Some(false)
        );
    }
}
