// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Structured (not string-templated) MCP client config targets — the subset of
//! `init_clients::SUPPORTED_CLIENTS` that live at a real, mergeable on-disk JSON
//! (or, for Codex, TOML) file. Used by the TUI's client-picker/diff/write flow
//! and the setup web UI.
//!
//! Deliberately smaller than `SUPPORTED_CLIENTS`: `continue` and `goose`
//! (YAML), `jetbrains` (no file, GUI-only), `opencode` (JSONC, with a v1/v2
//! schema split) and `openai-agents` (Python/TOML snippet, not a client
//! config) have no safe merge target and stay copy-only via `init_snippet`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigShape {
    /// `{"mcpServers": {"<key>": {"command":.., "args":[..]}}}`
    Claude,
    /// `{"servers": {"<key>": {"type":"stdio", "command":.., "args":[..]}}}`
    VsCode,
    /// `{"context_servers": {"<key>": {"source":"custom", "command":.., "args":[..]}}}`
    Zed,
    /// `{"mcpServers": {"<key>": {"type":"local", "command":.., "args":[..], "tools":["*"]}}}`
    CopilotCli,
    /// TOML: `[mcp_servers.<key>]` with `command`, `args` and an `env` table.
    Codex,
}

impl ConfigShape {
    fn top_key(self) -> &'static str {
        match self {
            Self::Claude | Self::CopilotCli => "mcpServers",
            Self::VsCode => "servers",
            Self::Zed => "context_servers",
            Self::Codex => "mcp_servers",
        }
    }

    fn entry(self, command: &str, args: &[String], env: &[(String, String)]) -> Value {
        let args_json = Value::Array(args.iter().map(|a| Value::String(a.clone())).collect());
        let mut entry = match self {
            Self::Claude | Self::Codex => json!({ "command": command, "args": args_json }),
            Self::VsCode => json!({ "type": "stdio", "command": command, "args": args_json }),
            Self::Zed => json!({ "source": "custom", "command": command, "args": args_json }),
            Self::CopilotCli => json!({
                "type": "local",
                "command": command,
                "args": args_json,
                "tools": ["*"],
            }),
        };
        if !env.is_empty() {
            let env: Map<String, Value> = env
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            entry["env"] = Value::Object(env);
        }
        entry
    }
}

pub struct ClientTarget {
    pub key: &'static str,
    pub display_name: &'static str,
    pub shape: ConfigShape,
}

/// Copy-only clients surfaced in pickers alongside mergeable targets.
pub const COPY_ONLY_CLIENTS: &[(&str, &str)] = &[
    ("continue", "Continue (copy YAML snippet)"),
    ("goose", "Goose (copy YAML snippet)"),
    ("opencode", "opencode (copy JSON snippet)"),
    ("jetbrains", "JetBrains AI Assistant (copy snippet)"),
    ("openai-agents", "OpenAI Agents SDK (copy snippet)"),
];

/// The clients with a real, mergeable config file.
pub fn mergeable_targets() -> Vec<ClientTarget> {
    vec![
        ClientTarget {
            key: "claude-desktop",
            display_name: "Claude Desktop",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "claude-code",
            display_name: "Claude Code",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "cursor",
            display_name: "Cursor",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "vscode",
            display_name: "VS Code",
            shape: ConfigShape::VsCode,
        },
        ClientTarget {
            key: "vscode-copilot",
            display_name: "VS Code Copilot Chat",
            shape: ConfigShape::VsCode,
        },
        ClientTarget {
            key: "zed",
            display_name: "Zed",
            shape: ConfigShape::Zed,
        },
        ClientTarget {
            key: "windsurf",
            display_name: "Windsurf",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "antigravity",
            display_name: "Google Antigravity",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "gemini-cli",
            display_name: "Gemini CLI",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "codex",
            display_name: "OpenAI Codex (CLI / IDE / ChatGPT desktop)",
            shape: ConfigShape::Codex,
        },
        ClientTarget {
            key: "copilot-cli",
            display_name: "GitHub Copilot CLI",
            shape: ConfigShape::CopilotCli,
        },
        ClientTarget {
            key: "cline",
            display_name: "Cline CLI",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "lm-studio",
            display_name: "LM Studio",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "kiro",
            display_name: "Kiro (formerly Amazon Q Developer CLI)",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "kimi",
            display_name: "Kimi Code CLI",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "qwen",
            display_name: "Qwen Code",
            shape: ConfigShape::Claude,
        },
    ]
}

/// Resolve the on-disk config path for a mergeable client key.
pub fn config_path_for(key: &str, workspace_root: Option<&Path>) -> Option<PathBuf> {
    match key {
        "claude-desktop" => claude_desktop_config_path(),
        "claude-code" => project_or_workspace_path(workspace_root, ".mcp.json"),
        "cursor" => project_or_workspace_path(workspace_root, ".cursor/mcp.json"),
        "vscode" | "vscode-copilot" => {
            project_or_workspace_path(workspace_root, ".vscode/mcp.json")
        }
        "zed" => zed_config_path(),
        "windsurf" => home_dir().map(|h| h.join(".codeium/windsurf/mcp_config.json")),
        "antigravity" => home_dir().map(|h| h.join(".gemini/config/mcp_config.json")),
        "gemini-cli" => home_dir().map(|h| h.join(".gemini/settings.json")),
        "codex" => home_dir().map(|h| h.join(".codex/config.toml")),
        "copilot-cli" => home_dir().map(|h| h.join(".copilot/mcp-config.json")),
        "cline" => cline_settings_path(std::env::var_os("CLINE_MCP_SETTINGS_PATH"), home_dir()),
        "lm-studio" => home_dir().map(|h| h.join(".lmstudio/mcp.json")),
        "kiro" => home_dir().map(|h| h.join(".kiro/settings/mcp.json")),
        "kimi" => kimi_mcp_path(std::env::var_os("KIMI_CODE_HOME"), home_dir()),
        "qwen" => home_dir().map(|h| h.join(".qwen/settings.json")),
        _ => None,
    }
}

/// Cline CLI: `CLINE_MCP_SETTINGS_PATH`, else `~/.cline/data/settings/`.
fn cline_settings_path(
    override_path: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    match override_path.filter(|p| !p.is_empty()) {
        Some(path) => Some(PathBuf::from(path)),
        None => home.map(|h| h.join(".cline/data/settings/cline_mcp_settings.json")),
    }
}

/// Kimi Code CLI: `$KIMI_CODE_HOME/mcp.json`, else `~/.kimi-code/mcp.json`.
fn kimi_mcp_path(kimi_home: Option<std::ffi::OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    match kimi_home.filter(|p| !p.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir).join("mcp.json")),
        None => home.map(|h| h.join(".kimi-code/mcp.json")),
    }
}

fn project_or_workspace_path(workspace_root: Option<&Path>, rel: &str) -> Option<PathBuf> {
    let base = workspace_root
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    Some(base.join(rel))
}

/// Parse `existing` (empty string / missing file → `{}`), set
/// `value[top_key][server_key] = {command, args, ...}`, leaving every sibling
/// key untouched, and return the pretty-printed result.
pub fn merge_entry(
    existing: &str,
    shape: ConfigShape,
    server_key: &str,
    command: &str,
    args: &[String],
) -> Result<String, String> {
    merge_entry_with_env(existing, shape, server_key, command, args, &[])
}

/// [`merge_entry`] plus an `env` object on the server entry (omitted when empty).
pub fn merge_entry_with_env(
    existing: &str,
    shape: ConfigShape,
    server_key: &str,
    command: &str,
    args: &[String],
    env: &[(String, String)],
) -> Result<String, String> {
    if shape == ConfigShape::Codex {
        return merge_toml_entry(existing, shape.top_key(), server_key, command, args, env);
    }
    let mut root: Value = if existing.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(existing)
            .map_err(|e| format!("existing config is not valid JSON: {e}"))?
    };

    if !root.is_object() {
        return Err("existing config's top level is not a JSON object".into());
    }
    let root_obj = root.as_object_mut().expect("checked above");

    let top_key = shape.top_key();
    let servers = root_obj
        .entry(top_key)
        .or_insert_with(|| Value::Object(Map::new()));
    if !servers.is_object() {
        return Err(format!("existing `{top_key}` is not a JSON object"));
    }
    servers
        .as_object_mut()
        .expect("checked above")
        .insert(server_key.to_string(), shape.entry(command, args, env));

    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
}

/// TOML counterpart of [`merge_entry_with_env`]: sets
/// `[<top_key>.<server_key>]`, keeping every other table, key and comment.
fn merge_toml_entry(
    existing: &str,
    top_key: &str,
    server_key: &str,
    command: &str,
    args: &[String],
    env: &[(String, String)],
) -> Result<String, String> {
    use toml_edit::{Array, DocumentMut, Item, Table, value};

    let mut doc: DocumentMut = existing
        .parse()
        .map_err(|e| format!("existing config is not valid TOML: {e}"))?;

    let servers = doc.entry(top_key).or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(true);
        Item::Table(table)
    });
    let Some(servers) = servers.as_table_mut() else {
        return Err(format!("existing `{top_key}` is not a TOML table"));
    };

    let mut entry = Table::new();
    entry.insert("command", value(command));
    entry.insert("args", value(args.iter().collect::<Array>()));
    if !env.is_empty() {
        let mut env_table = Table::new();
        for (k, v) in env {
            env_table.insert(k, value(v));
        }
        entry.insert("env", Item::Table(env_table));
    }
    servers.insert(server_key, Item::Table(entry));

    Ok(doc.to_string())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn claude_desktop_config_path() -> Option<PathBuf> {
    match std::env::consts::OS {
        "macos" => home_dir()
            .map(|h| h.join("Library/Application Support/Claude/claude_desktop_config.json")),
        "windows" => std::env::var_os("APPDATA").map(|a| {
            PathBuf::from(a)
                .join("Claude")
                .join("claude_desktop_config.json")
        }),
        _ => home_dir().map(|h| h.join(".config/Claude/claude_desktop_config.json")),
    }
}

fn zed_config_path() -> Option<PathBuf> {
    match std::env::consts::OS {
        "macos" => home_dir().map(|h| h.join("Library/Application Support/Zed/settings.json")),
        "windows" => {
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Zed").join("settings.json"))
        }
        _ => home_dir().map(|h| h.join(".config/zed/settings.json")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_into_empty_creates_top_key() {
        let out = merge_entry(
            "",
            ConfigShape::Claude,
            "nexql-mcp",
            "nexql-mcp",
            &["--profile".into(), "local".into()],
        )
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["nexql-mcp"]["command"], "nexql-mcp");
        assert_eq!(v["mcpServers"]["nexql-mcp"]["args"][1], "local");
    }

    #[test]
    fn merge_preserves_unrelated_sibling_servers() {
        let existing = r#"{"mcpServers": {"other-server": {"command": "other", "args": []}}}"#;
        let out =
            merge_entry(existing, ConfigShape::Claude, "nexql-mcp", "nexql-mcp", &[]).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["other-server"]["command"], "other");
        assert_eq!(v["mcpServers"]["nexql-mcp"]["command"], "nexql-mcp");
    }

    #[test]
    fn merge_preserves_unrelated_top_level_keys() {
        let existing = r#"{"someOtherSetting": true, "servers": {}}"#;
        let out =
            merge_entry(existing, ConfigShape::VsCode, "nexql-mcp", "nexql-mcp", &[]).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["someOtherSetting"], true);
        assert_eq!(v["servers"]["nexql-mcp"]["type"], "stdio");
    }

    #[test]
    fn vscode_shape_sets_stdio_type() {
        let out = merge_entry("", ConfigShape::VsCode, "nexql-mcp", "nexql-mcp", &[]).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["servers"]["nexql-mcp"]["type"], "stdio");
    }

    #[test]
    fn zed_shape_sets_source_custom() {
        let out = merge_entry("", ConfigShape::Zed, "nexql-mcp", "nexql-mcp", &[]).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["context_servers"]["nexql-mcp"]["source"], "custom");
    }

    #[test]
    fn rejects_non_object_top_level() {
        let err =
            merge_entry("[]", ConfigShape::Claude, "nexql-mcp", "nexql-mcp", &[]).unwrap_err();
        assert!(err.contains("not a JSON object"));
    }

    #[test]
    fn rejects_invalid_json() {
        let err = merge_entry(
            "{not json",
            ConfigShape::Claude,
            "nexql-mcp",
            "nexql-mcp",
            &[],
        )
        .unwrap_err();
        assert!(err.contains("not valid JSON"));
    }

    #[test]
    fn mergeable_targets_cover_sixteen_clients() {
        let targets = mergeable_targets();
        assert_eq!(targets.len(), 16);
        let keys: Vec<_> = targets.iter().map(|t| t.key).collect();
        for expected in [
            "claude-desktop",
            "claude-code",
            "cursor",
            "vscode",
            "vscode-copilot",
            "zed",
            "windsurf",
            "antigravity",
            "gemini-cli",
            "codex",
            "copilot-cli",
            "cline",
            "lm-studio",
            "kiro",
            "kimi",
            "qwen",
        ] {
            assert!(keys.contains(&expected), "missing {expected}");
        }
        for removed in ["deepseek", "ollama"] {
            assert!(!keys.contains(&removed), "{removed} has no MCP config file");
        }
    }

    #[test]
    fn every_target_is_a_supported_client_and_resolves_a_path() {
        for target in mergeable_targets() {
            assert!(
                crate::init_clients::SUPPORTED_CLIENTS.contains(&target.key),
                "{} missing from SUPPORTED_CLIENTS",
                target.key
            );
            assert!(
                config_path_for(target.key, Some(Path::new("/tmp/p"))).is_some(),
                "{}",
                target.key
            );
        }
        for (key, _) in COPY_ONLY_CLIENTS {
            assert!(
                crate::init_clients::SUPPORTED_CLIENTS.contains(key),
                "{key}"
            );
            assert!(config_path_for(key, None).is_none(), "{key}");
        }
    }

    #[test]
    fn user_level_paths_match_client_docs() {
        let Some(home) = home_dir() else { return };
        for (key, rel) in [
            ("antigravity", ".gemini/config/mcp_config.json"),
            ("gemini-cli", ".gemini/settings.json"),
            ("codex", ".codex/config.toml"),
            ("copilot-cli", ".copilot/mcp-config.json"),
            ("lm-studio", ".lmstudio/mcp.json"),
            ("kiro", ".kiro/settings/mcp.json"),
            ("qwen", ".qwen/settings.json"),
        ] {
            assert_eq!(config_path_for(key, None), Some(home.join(rel)), "{key}");
        }
    }

    #[test]
    fn cline_path_honours_override_env() {
        let home = PathBuf::from("/home/u");
        assert_eq!(
            cline_settings_path(None, Some(home.clone())),
            Some(home.join(".cline/data/settings/cline_mcp_settings.json"))
        );
        assert_eq!(
            cline_settings_path(Some("".into()), Some(home.clone())),
            Some(home.join(".cline/data/settings/cline_mcp_settings.json"))
        );
        assert_eq!(
            cline_settings_path(Some("/etc/cline.json".into()), Some(home)),
            Some(PathBuf::from("/etc/cline.json"))
        );
    }

    #[test]
    fn kimi_path_honours_kimi_code_home() {
        let home = PathBuf::from("/home/u");
        assert_eq!(
            kimi_mcp_path(None, Some(home.clone())),
            Some(home.join(".kimi-code/mcp.json"))
        );
        assert_eq!(
            kimi_mcp_path(Some("/opt/kimi".into()), Some(home)),
            Some(PathBuf::from("/opt/kimi/mcp.json"))
        );
    }

    #[test]
    fn gemini_settings_keep_other_keys() {
        let existing = r#"{"theme": "GitHub", "general": {"vimMode": true}}"#;
        let out =
            merge_entry(existing, ConfigShape::Claude, "nexql-mcp", "nexql-mcp", &[]).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["theme"], "GitHub");
        assert_eq!(v["general"]["vimMode"], true);
        assert_eq!(v["mcpServers"]["nexql-mcp"]["command"], "nexql-mcp");
    }

    #[test]
    fn copilot_cli_shape_is_local_with_all_tools() {
        let out = merge_entry(
            "",
            ConfigShape::CopilotCli,
            "nexql-mcp",
            "nexql-mcp",
            &["--profile".into(), "shop".into()],
        )
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let entry = &v["mcpServers"]["nexql-mcp"];
        assert_eq!(entry["type"], "local");
        assert_eq!(entry["tools"][0], "*");
        assert_eq!(entry["args"][1], "shop");
    }

    #[test]
    fn codex_toml_into_empty_file() {
        let out = merge_entry(
            "",
            ConfigShape::Codex,
            "nexql-mcp",
            "nexql-mcp",
            &["--profile".into(), "shop".into()],
        )
        .unwrap();
        let v: toml::Value = toml::from_str(&out).unwrap();
        let entry = &v["mcp_servers"]["nexql-mcp"];
        assert_eq!(entry["command"].as_str(), Some("nexql-mcp"));
        assert_eq!(entry["args"][1].as_str(), Some("shop"));
        assert!(entry.get("env").is_none());
        assert!(!out.contains("[mcp_servers]\n"), "{out}");
    }

    #[test]
    fn codex_toml_keeps_comments_settings_and_other_servers() {
        let existing = r#"# my codex config
model = "gpt-5-codex"

[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp_servers.nexql-mcp]
command = "old"
"#;
        let out = merge_entry_with_env(
            existing,
            ConfigShape::Codex,
            "nexql-mcp",
            "/usr/bin/npx",
            &["-y".into(), "nexql-mcp@latest".into()],
            &[("PATH".into(), "/usr/bin:/bin".into())],
        )
        .unwrap();
        assert!(out.starts_with("# my codex config\n"), "{out}");
        let v: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(v["model"].as_str(), Some("gpt-5-codex"));
        assert_eq!(
            v["mcp_servers"]["context7"]["command"].as_str(),
            Some("npx")
        );
        let entry = &v["mcp_servers"]["nexql-mcp"];
        assert_eq!(entry["command"].as_str(), Some("/usr/bin/npx"));
        assert_eq!(entry["args"][1].as_str(), Some("nexql-mcp@latest"));
        assert_eq!(entry["env"]["PATH"].as_str(), Some("/usr/bin:/bin"));
    }

    #[test]
    fn codex_toml_rejects_invalid_or_non_table() {
        let err = merge_entry("not = = toml", ConfigShape::Codex, "k", "c", &[]).unwrap_err();
        assert!(err.contains("not valid TOML"), "{err}");
        let err = merge_entry("mcp_servers = 3", ConfigShape::Codex, "k", "c", &[]).unwrap_err();
        assert!(err.contains("not a TOML table"), "{err}");
    }

    #[test]
    fn cursor_path_uses_workspace_root() {
        let root = PathBuf::from("/tmp/my-project");
        let path = config_path_for("cursor", Some(&root)).unwrap();
        assert_eq!(path, root.join(".cursor/mcp.json"));
    }
}
