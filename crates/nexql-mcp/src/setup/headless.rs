// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Non-interactive setup — profile creation + optional client wiring without the browser UI.

use std::path::PathBuf;

use nexql_conn::{ConfigFile, ProfileConfig, params_from_url, resolve_profile, test_connection};

use crate::client_targets;
use crate::setup::wire::{LaunchConfig, apply_wire, preview_wire};

#[derive(Debug, Clone)]
pub struct HeadlessOptions {
    pub name: String,
    pub dsn: String,
    pub clients: Vec<String>,
    pub apply: bool,
    pub skip_test: bool,
    pub config_path: PathBuf,
    pub workspace_root: Option<PathBuf>,
}

pub async fn run(options: HeadlessOptions) -> Result<(), String> {
    let name = options.name.trim();
    if name.is_empty() {
        return Err("--name is required for headless setup".into());
    }

    let dsn = options.dsn.trim();
    if dsn.is_empty() {
        return Err(
            "headless setup requires a connection string — pass --dsn or set NEXQL_MCP_DSN".into(),
        );
    }

    if dsn_supplied_via_argv() && url_has_password(dsn) {
        eprintln!(
            "warning: password in --dsn on the command line may be visible in shell history — \
             prefer NEXQL_MCP_DSN or password_command after setup"
        );
    }

    let profile = ProfileConfig {
        url: Some(dsn.to_string()),
        ..Default::default()
    };

    if !options.skip_test {
        let params = resolve_profile(name, &profile).map_err(|e| e.to_string())?;
        let report = test_connection(&params)
            .await
            .map_err(|e| format!("connection test failed for profile \"{name}\": {e}"))?;
        println!(
            "✓ Connection verified: {} (latency: {:.0}ms)",
            report.server_version,
            report.latency.as_secs_f64() * 1000.0
        );
    }

    if let Some(parent) = options.config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let (mut config, _) = if options.config_path.exists() {
        ConfigFile::load_path_migrated(&options.config_path).map_err(|e| e.to_string())?
    } else {
        (ConfigFile::default(), Default::default())
    };

    config.default_profile = Some(name.to_string());
    config
        .upsert_profile_prepared(name.to_string(), profile)
        .map_err(|e| e.to_string())?;

    let backup = config
        .save(&options.config_path)
        .map_err(|e| e.to_string())?;
    println!(
        "✓ Profile '{name}' saved to {}",
        options.config_path.display()
    );
    if let Some(b) = backup {
        println!("  (previous config backed up to {})", b.display());
    }

    if options.apply {
        if options.clients.is_empty() {
            return Err("--apply requires at least one --client (e.g. --client cursor)".into());
        }
        apply_clients(
            &config,
            name,
            &options.clients,
            options.workspace_root.as_deref(),
        )?;
    }

    Ok(())
}

fn apply_clients(
    config: &ConfigFile,
    profile_name: &str,
    client_names: &[String],
    workspace_root: Option<&std::path::Path>,
) -> Result<(), String> {
    let keys: Vec<String> = client_names
        .iter()
        .map(|c| normalize_client_key(c))
        .collect();

    let targets = client_targets::mergeable_targets();
    for key in &keys {
        if !targets.iter().any(|t| t.key == key.as_str()) {
            let mergeable_list = targets.iter().map(|t| t.key).collect::<Vec<_>>().join(", ");
            return Err(format!(
                "unknown or copy-only client \"{key}\" — mergeable clients: {mergeable_list}"
            ));
        }
    }

    if !config.profiles.contains_key(profile_name) {
        return Err(format!(
            "profile \"{profile_name}\" not found after save — internal error"
        ));
    }

    let profile_names = vec![profile_name.to_string()];
    let launch = LaunchConfig::default();
    let preview = preview_wire(config, &profile_names, &keys, &[], &launch, workspace_root);
    let results = apply_wire(&preview, &keys);

    let mut had_error = false;
    for r in &results {
        if let Some(ref err) = r.error {
            had_error = true;
            eprintln!("✗ {}: {err}", r.display_name);
            continue;
        }
        if r.applied {
            let path = r.config_path.as_deref().unwrap_or("?");
            println!("✓ wrote {} ({path})", r.display_name);
            if let Some(ref backup) = r.backup {
                println!("  (previous config backed up to {backup})");
            }
        } else if r.skipped {
            println!("- skipped {}", r.display_name);
        }
    }

    if had_error {
        return Err("failed to apply one or more client configs".into());
    }
    Ok(())
}

fn normalize_client_key(client_name: &str) -> String {
    let key = client_name.trim().to_ascii_lowercase();
    if key == "claude" {
        "claude-desktop".to_string()
    } else {
        key
    }
}

fn dsn_supplied_via_argv() -> bool {
    std::env::args().any(|arg| arg == "--dsn")
}

fn url_has_password(dsn: &str) -> bool {
    params_from_url(dsn)
        .ok()
        .and_then(|p| p.password.map(|p| !p.is_empty()))
        .unwrap_or(false)
}
