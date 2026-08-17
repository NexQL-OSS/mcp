// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Client wiring — shared by the TUI and setup web UI.

use std::path::{Path, PathBuf};

use nexql_conn::{ConfigFile, ProfileConfig, write_with_backup};
use similar::TextDiff;

use crate::client_targets;
use crate::init_clients;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    Path,
    Npx,
    NpxLatest,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LaunchConfig {
    pub mode: LaunchMode,
    /// Pin for `LaunchMode::Npx` (e.g. "0.3.3"). Ignored for other modes.
    pub npx_version: Option<String>,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            mode: LaunchMode::Path,
            npx_version: None,
        }
    }
}

impl LaunchConfig {
    pub fn command_and_args(&self, profile_names: &[String]) -> (String, Vec<String>) {
        let mut profile_args = Vec::new();
        for name in profile_names {
            profile_args.push("--profile".to_string());
            profile_args.push(name.clone());
        }
        match self.mode {
            LaunchMode::Path => ("nexql-mcp".into(), profile_args),
            LaunchMode::Npx => {
                let version = self
                    .npx_version
                    .as_deref()
                    .filter(|v| !v.is_empty())
                    .unwrap_or(env!("CARGO_PKG_VERSION"));
                let mut args = vec![
                    "-y".into(),
                    format!("nexql-mcp@{version}"),
                ];
                args.extend(profile_args);
                ("npx".into(), args)
            }
            LaunchMode::NpxLatest => {
                let mut args = vec!["-y".into(), "nexql-mcp".into()];
                args.extend(profile_args);
                ("npx".into(), args)
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MergeableClientInfo {
    pub key: String,
    pub display_name: String,
    pub config_path: Option<String>,
    pub exists: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CopyOnlyClientInfo {
    pub key: String,
    pub display_name: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientListResponse {
    pub mergeable: Vec<MergeableClientInfo>,
    pub copy_only: Vec<CopyOnlyClientInfo>,
}

pub fn list_clients(workspace_root: Option<&Path>) -> ClientListResponse {
    let mergeable = client_targets::mergeable_targets()
        .into_iter()
        .map(|t| {
            let path = client_targets::config_path_for(t.key, workspace_root);
            let exists = path.as_ref().is_some_and(|p| p.exists());
            MergeableClientInfo {
                key: t.key.to_string(),
                display_name: t.display_name.to_string(),
                config_path: path.as_ref().map(|p| p.display().to_string()),
                exists,
            }
        })
        .collect();
    let copy_only = client_targets::COPY_ONLY_CLIENTS
        .iter()
        .map(|(key, display_name)| CopyOnlyClientInfo {
            key: (*key).to_string(),
            display_name: (*display_name).to_string(),
        })
        .collect();
    ClientListResponse {
        mergeable,
        copy_only,
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireDiff {
    pub client_key: String,
    pub display_name: String,
    pub config_path: String,
    pub unified_diff: String,
    pub new_content: String,
    pub valid: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireSnippet {
    pub client_key: String,
    pub display_name: String,
    pub content: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WirePreview {
    pub diffs: Vec<WireDiff>,
    pub snippets: Vec<WireSnippet>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireApplyResult {
    pub client_key: String,
    pub display_name: String,
    pub config_path: Option<String>,
    pub backup: Option<String>,
    pub applied: bool,
    pub skipped: bool,
    pub error: Option<String>,
}

pub fn preview_wire(
    config: &ConfigFile,
    profile_names: &[String],
    mergeable_client_keys: &[String],
    copy_only_client_keys: &[String],
    launch: &LaunchConfig,
    workspace_root: Option<&Path>,
) -> WirePreview {
    if profile_names.is_empty() {
        return WirePreview {
            diffs: Vec::new(),
            snippets: Vec::new(),
        };
    }

    let (command, args) = launch.command_and_args(profile_names);
    let first_name = &profile_names[0];
    let first_profile = config.profiles.get(first_name).cloned();

    let mut diffs = Vec::new();
    let targets = client_targets::mergeable_targets();
    for key in mergeable_client_keys {
        let Some(target) = targets.iter().find(|t| t.key == key.as_str()) else {
            continue;
        };
        let Some(path) = client_targets::config_path_for(target.key, workspace_root) else {
            diffs.push(WireDiff {
                client_key: key.clone(),
                display_name: target.display_name.to_string(),
                config_path: String::new(),
                unified_diff: String::new(),
                new_content: String::new(),
                valid: false,
                error: Some("could not resolve a config path on this OS".into()),
            });
            continue;
        };
        let old_content = std::fs::read_to_string(&path).unwrap_or_default();
        match client_targets::merge_entry(
            &old_content,
            target.shape,
            "nexql-mcp",
            &command,
            &args,
        ) {
            Ok(new_content) => {
                let diff = TextDiff::from_lines(&old_content, &new_content);
                let unified_diff = diff.unified_diff().context_radius(2).to_string();
                diffs.push(WireDiff {
                    client_key: key.clone(),
                    display_name: target.display_name.to_string(),
                    config_path: path.display().to_string(),
                    unified_diff,
                    new_content,
                    valid: true,
                    error: None,
                });
            }
            Err(e) => diffs.push(WireDiff {
                client_key: key.clone(),
                display_name: target.display_name.to_string(),
                config_path: path.display().to_string(),
                unified_diff: String::new(),
                new_content: String::new(),
                valid: false,
                error: Some(e),
            }),
        }
    }

    let url = first_profile
        .as_ref()
        .and_then(|p| nexql_conn::resolve_profile(first_name, p).ok())
        .and_then(|p| p.to_url().ok());

    let mut snippets = Vec::new();
    for key in copy_only_client_keys {
        let display_name = client_targets::COPY_ONLY_CLIENTS
            .iter()
            .find(|(k, _)| *k == key.as_str())
            .map(|(_, d)| *d)
            .unwrap_or(key.as_str());
        match init_clients::init_snippet(key, url.as_deref()) {
            Ok(content) => snippets.push(WireSnippet {
                client_key: key.clone(),
                display_name: display_name.to_string(),
                content,
                error: None,
            }),
            Err(e) => snippets.push(WireSnippet {
                client_key: key.clone(),
                display_name: display_name.to_string(),
                content: String::new(),
                error: Some(e),
            }),
        }
    }

    WirePreview {
        diffs,
        snippets,
    }
}

pub fn apply_wire(
    preview: &WirePreview,
    apply_keys: &[String],
) -> Vec<WireApplyResult> {
    let mut results = Vec::new();
    for diff in &preview.diffs {
        if !apply_keys.iter().any(|k| k == &diff.client_key) {
            results.push(WireApplyResult {
                client_key: diff.client_key.clone(),
                display_name: diff.display_name.clone(),
                config_path: Some(diff.config_path.clone()),
                backup: None,
                applied: false,
                skipped: true,
                error: None,
            });
            continue;
        }
        if !diff.valid {
            results.push(WireApplyResult {
                client_key: diff.client_key.clone(),
                display_name: diff.display_name.clone(),
                config_path: Some(diff.config_path.clone()),
                backup: None,
                applied: false,
                skipped: true,
                error: diff.error.clone(),
            });
            continue;
        }
        let path = PathBuf::from(&diff.config_path);
        match write_with_backup(&path, &diff.new_content) {
            Ok(backup) => results.push(WireApplyResult {
                client_key: diff.client_key.clone(),
                display_name: diff.display_name.clone(),
                config_path: Some(diff.config_path.clone()),
                backup: backup.as_ref().map(|b| b.display().to_string()),
                applied: true,
                skipped: false,
                error: None,
            }),
            Err(e) => results.push(WireApplyResult {
                client_key: diff.client_key.clone(),
                display_name: diff.display_name.clone(),
                config_path: Some(diff.config_path.clone()),
                backup: None,
                applied: false,
                skipped: true,
                error: Some(e.to_string()),
            }),
        }
    }
    results
}

/// TUI-facing diff row — mirrors the shape the terminal UI already renders.
#[derive(Debug, Clone)]
pub struct TuiDiffEntry {
    pub display_name: &'static str,
    pub config_path: PathBuf,
    pub old_content: String,
    pub new_content: String,
    pub apply: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct TuiSummaryEntry {
    pub display_name: String,
    pub path: Option<PathBuf>,
    pub backup: Option<PathBuf>,
    pub snippet: Option<String>,
    pub error: Option<String>,
    pub skipped: bool,
}

pub struct TuiWireOutcome {
    pub diffs: Vec<TuiDiffEntry>,
    pub summary: Vec<TuiSummaryEntry>,
}

pub fn build_tui_wire(
    config: &ConfigFile,
    profile_names: &[String],
    selected_mergeable_keys: &[&str],
    selected_copy_only_keys: &[&str],
    launch: &LaunchConfig,
    workspace_root: Option<&Path>,
) -> TuiWireOutcome {
    let preview = preview_wire(
        config,
        profile_names,
        &selected_mergeable_keys
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>(),
        &selected_copy_only_keys
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>(),
        launch,
        workspace_root,
    );

    let mut diffs = Vec::new();
    let mut summary = Vec::new();

    for d in preview.diffs {
        if d.valid {
            let old_content = std::fs::read_to_string(&d.config_path).unwrap_or_default();
            let display_name = client_targets::mergeable_targets()
                .iter()
                .find(|t| t.key == d.client_key)
                .map(|t| t.display_name)
                .unwrap_or("unknown");
            diffs.push(TuiDiffEntry {
                display_name,
                config_path: PathBuf::from(d.config_path),
                old_content,
                new_content: d.new_content,
                apply: None,
            });
        } else {
            summary.push(TuiSummaryEntry {
                display_name: d.display_name,
                path: if d.config_path.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(d.config_path))
                },
                backup: None,
                snippet: None,
                error: d.error,
                skipped: true,
            });
        }
    }

    for snippet in preview.snippets {
        let skipped = snippet.error.is_some();
        summary.push(TuiSummaryEntry {
            display_name: snippet.display_name,
            path: None,
            backup: None,
            snippet: snippet.error.is_none().then_some(snippet.content),
            error: snippet.error,
            skipped,
        });
    }

    TuiWireOutcome { diffs, summary }
}

pub fn finalize_tui_diffs(diffs: &[TuiDiffEntry]) -> Vec<TuiSummaryEntry> {
    let mut summary = Vec::new();
    for d in diffs {
        if d.apply == Some(true) {
            match write_with_backup(&d.config_path, &d.new_content) {
                Ok(backup) => summary.push(TuiSummaryEntry {
                    display_name: d.display_name.to_string(),
                    path: Some(d.config_path.clone()),
                    backup,
                    snippet: None,
                    error: None,
                    skipped: false,
                }),
                Err(e) => summary.push(TuiSummaryEntry {
                    display_name: d.display_name.to_string(),
                    path: Some(d.config_path.clone()),
                    backup: None,
                    snippet: None,
                    error: Some(e.to_string()),
                    skipped: true,
                }),
            }
        } else {
            summary.push(TuiSummaryEntry {
                display_name: d.display_name.to_string(),
                path: Some(d.config_path.clone()),
                backup: None,
                snippet: None,
                error: None,
                skipped: true,
            });
        }
    }
    summary
}

#[allow(dead_code)]
pub fn profile_from_wire_fields(
    url: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    dbname: Option<String>,
    user: Option<String>,
    password: Option<String>,
    password_command: Option<String>,
    password_file: Option<String>,
    sslmode: Option<String>,
    access_mode: Option<String>,
    schemas: Vec<String>,
    deny_schemas: Vec<String>,
    deny_tables: Vec<String>,
    pii_columns: Vec<String>,
    max_rows: Option<u32>,
    statement_timeout_ms: Option<u32>,
    credential_provider: Option<String>,
) -> ProfileConfig {
    ProfileConfig {
        url,
        host,
        port,
        dbname,
        user,
        password,
        password_command,
        password_file,
        sslmode,
        access_mode,
        schemas,
        deny_schemas,
        deny_tables,
        pii_columns,
        max_rows,
        statement_timeout_ms,
        credential_provider,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_path_uses_binary_name() {
        let (cmd, args) = LaunchConfig::default().command_and_args(&["local".into()]);
        assert_eq!(cmd, "nexql-mcp");
        assert_eq!(args, vec!["--profile", "local"]);
    }

    #[test]
    fn launch_npx_pins_version() {
        let launch = LaunchConfig {
            mode: LaunchMode::Npx,
            npx_version: Some("1.2.3".into()),
        };
        let (cmd, args) = launch.command_and_args(&["SSP Dev".into()]);
        assert_eq!(cmd, "npx");
        assert_eq!(args[0], "-y");
        assert_eq!(args[1], "nexql-mcp@1.2.3");
        assert_eq!(args[2], "--profile");
        assert_eq!(args[3], "SSP Dev");
    }

    #[test]
    fn preview_empty_profiles_returns_empty() {
        let cfg = ConfigFile::default();
        let preview = preview_wire(&cfg, &[], &["cursor".into()], &[], &LaunchConfig::default(), None);
        assert!(preview.diffs.is_empty());
        assert!(preview.snippets.is_empty());
    }
}
