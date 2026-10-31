// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Read-only view of each client's current `nexql-mcp` entry, so the setup
//! page can show what is wired today and which profiles each client uses.
//!
//! Deliberately shape-agnostic: it looks for a `nexql-mcp` key under any
//! top-level table, in JSON or TOML, so new client targets show up here
//! without changes.

use std::path::Path;

use serde_json::Value;

use super::wire::list_clients;

const SERVER_KEY: &str = "nexql-mcp";

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientEntry {
    pub key: String,
    pub display_name: String,
    pub config_path: Option<String>,
    pub exists: bool,
    /// The `nexql-mcp` entry as found on disk, if any.
    pub entry: Option<Value>,
    /// Profiles named by `--profile` in the entry's args.
    pub profiles: Vec<String>,
    /// Set when the file exists but could not be read or parsed.
    pub error: Option<String>,
}

pub fn list_entries(workspace_root: Option<&Path>) -> Vec<ClientEntry> {
    list_clients(workspace_root)
        .mergeable
        .into_iter()
        .map(|client| {
            let mut out = ClientEntry {
                key: client.key,
                display_name: client.display_name,
                config_path: client.config_path.clone(),
                exists: client.exists,
                entry: None,
                profiles: Vec::new(),
                error: None,
            };
            let Some(path) = client.config_path.filter(|_| client.exists) else {
                return out;
            };
            match std::fs::read_to_string(&path) {
                Ok(text) => match find_entry(&text, path.ends_with(".toml")) {
                    Ok(entry) => {
                        out.profiles = entry.as_ref().map(profiles_in).unwrap_or_default();
                        out.entry = entry;
                    }
                    Err(e) => out.error = Some(e),
                },
                Err(e) => out.error = Some(e.to_string()),
            }
            out
        })
        .collect()
}

/// Find the `nexql-mcp` server entry under any top-level table.
fn find_entry(text: &str, is_toml: bool) -> Result<Option<Value>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let root: Value = if is_toml {
        let parsed: toml::Value =
            toml::from_str(text).map_err(|e| format!("not valid TOML: {e}"))?;
        serde_json::to_value(parsed).map_err(|e| e.to_string())?
    } else {
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?
    };
    let Some(obj) = root.as_object() else {
        return Ok(None);
    };
    Ok(obj
        .values()
        .filter_map(Value::as_object)
        .find_map(|servers| servers.get(SERVER_KEY).cloned()))
}

fn profiles_in(entry: &Value) -> Vec<String> {
    let args: Vec<&str> = entry
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut profiles = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if *arg == "--profile" {
            if let Some(name) = iter.next() {
                profiles.push((*name).to_string());
            }
        } else if let Some(name) = arg.strip_prefix("--profile=") {
            profiles.push(name.to_string());
        }
    }
    profiles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_entry_in_json_under_any_top_key() {
        let text = r#"{"servers": {"other": {}, "nexql-mcp": {"command": "npx", "args": ["-y", "nexql-mcp@latest", "--profile", "local", "--profile=billing"]}}}"#;
        let entry = find_entry(text, false).unwrap().unwrap();
        assert_eq!(entry["command"], "npx");
        assert_eq!(profiles_in(&entry), vec!["local", "billing"]);
    }

    #[test]
    fn finds_entry_in_toml() {
        let text =
            "[mcp_servers.nexql-mcp]\ncommand = \"nexql-mcp\"\nargs = [\"--profile\", \"dev\"]\n";
        let entry = find_entry(text, true).unwrap().unwrap();
        assert_eq!(profiles_in(&entry), vec!["dev"]);
    }

    #[test]
    fn missing_entry_and_empty_file_are_none() {
        assert!(find_entry("", false).unwrap().is_none());
        assert!(
            find_entry(r#"{"mcpServers": {"x": {}}}"#, false)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn invalid_json_is_reported() {
        assert!(find_entry("{nope", false).unwrap_err().contains("JSON"));
    }
}
