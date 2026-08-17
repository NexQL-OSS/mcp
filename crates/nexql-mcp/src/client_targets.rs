// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Structured (not string-templated) MCP client config targets — the subset of
//! `init_clients::SUPPORTED_CLIENTS` that live at a real, mergeable on-disk JSON
//! file. Used by the TUI's client-picker/diff/write flow and the setup web UI.
//!
//! Deliberately smaller than `SUPPORTED_CLIENTS`: `continue` (YAML), `jetbrains`
//! (no file, GUI-only), and `openai-agents` (Python/TOML snippet, not a client
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
}

impl ConfigShape {
    fn top_key(self) -> &'static str {
        match self {
            Self::Claude => "mcpServers",
            Self::VsCode => "servers",
            Self::Zed => "context_servers",
        }
    }

    fn entry(self, command: &str, args: &[String]) -> Value {
        let args_json = Value::Array(args.iter().map(|a| Value::String(a.clone())).collect());
        match self {
            Self::Claude => json!({ "command": command, "args": args_json }),
            Self::VsCode => json!({ "type": "stdio", "command": command, "args": args_json }),
            Self::Zed => json!({ "source": "custom", "command": command, "args": args_json }),
        }
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
    ("jetbrains", "JetBrains AI Assistant (copy snippet)"),
    ("openai-agents", "OpenAI Agents SDK (copy snippet)"),
];

/// The clients with a real, mergeable JSON config file.
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
            display_name: "Antigravity (Google DeepMind AI)",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "deepseek",
            display_name: "DeepSeek AI / DeepSeek Coder",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "kimi",
            display_name: "Kimi (Moonshot AI)",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "ollama",
            display_name: "Ollama (Local LLMs)",
            shape: ConfigShape::Claude,
        },
        ClientTarget {
            key: "qwen",
            display_name: "Qwen (Alibaba Cloud AI)",
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
        "antigravity" => home_dir().map(|h| {
            h.join(".gemini")
                .join("antigravity-ide")
                .join("mcp_config.json")
        }),
        "deepseek" => project_or_workspace_path(workspace_root, ".deepseek/mcp.json").or_else(|| {
            home_dir().map(|h| h.join(".config/deepseek/mcp.json"))
        }),
        "kimi" => project_or_workspace_path(workspace_root, ".kimi/mcp.json")
            .or_else(|| home_dir().map(|h| h.join(".config/kimi/mcp.json"))),
        "ollama" => project_or_workspace_path(workspace_root, ".ollama/mcp.json")
            .or_else(|| home_dir().map(|h| h.join(".ollama/mcp.json"))),
        "qwen" => project_or_workspace_path(workspace_root, ".qwen/mcp.json")
            .or_else(|| home_dir().map(|h| h.join(".config/qwen/mcp.json"))),
        _ => None,
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
        .insert(server_key.to_string(), shape.entry(command, args));

    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
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
    fn mergeable_targets_cover_twelve_clients() {
        let targets = mergeable_targets();
        assert_eq!(targets.len(), 12);
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
            "deepseek",
            "kimi",
            "ollama",
            "qwen",
        ] {
            assert!(keys.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn cursor_path_uses_workspace_root() {
        let root = PathBuf::from("/tmp/my-project");
        let path = config_path_for("cursor", Some(&root)).unwrap();
        assert_eq!(path, root.join(".cursor/mcp.json"));
    }
}
