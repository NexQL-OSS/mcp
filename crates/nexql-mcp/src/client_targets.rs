//! Structured (not string-templated) MCP client config targets — the subset of
//! `init_clients::SUPPORTED_CLIENTS` that live at a real, mergeable on-disk JSON
//! file. Used by the TUI's client-picker/diff/write flow.
//!
//! Deliberately smaller than `SUPPORTED_CLIENTS`: `continue` (YAML), `jetbrains`
//! (no file, GUI-only), and `openai-agents` (Python/TOML snippet, not a client
//! config) have no safe merge target and stay copy-only via `init_snippet`.

use std::path::PathBuf;

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
    pub config_path: fn() -> Option<PathBuf>,
}

/// The 7 clients with a real, mergeable JSON config file.
pub fn mergeable_targets() -> Vec<ClientTarget> {
    vec![
        ClientTarget {
            key: "claude-desktop",
            display_name: "Claude Desktop",
            shape: ConfigShape::Claude,
            config_path: claude_desktop_config_path,
        },
        ClientTarget {
            key: "claude-code",
            display_name: "Claude Code",
            shape: ConfigShape::Claude,
            config_path: claude_code_config_path,
        },
        ClientTarget {
            key: "cursor",
            display_name: "Cursor",
            shape: ConfigShape::Claude,
            config_path: cursor_config_path,
        },
        ClientTarget {
            key: "vscode",
            display_name: "VS Code",
            shape: ConfigShape::VsCode,
            config_path: vscode_config_path,
        },
        ClientTarget {
            key: "vscode-copilot",
            display_name: "VS Code Copilot Chat",
            shape: ConfigShape::VsCode,
            config_path: vscode_config_path,
        },
        ClientTarget {
            key: "zed",
            display_name: "Zed",
            shape: ConfigShape::Zed,
            config_path: zed_config_path,
        },
        ClientTarget {
            key: "windsurf",
            display_name: "Windsurf",
            shape: ConfigShape::Claude,
            config_path: windsurf_config_path,
        },
    ]
}

/// Parse `existing` (empty string / missing file → `{}`), set
/// `value[top_key][server_key] = {command, args, ...}`, leaving every sibling
/// key untouched, and return the pretty-printed result.
///
/// Known limitation: this reformats the whole file (not a text-preserving
/// patch) — acceptable for v1, surfaced in the diff view before writing.
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

fn claude_code_config_path() -> Option<PathBuf> {
    std::env::current_dir().ok().map(|d| d.join(".mcp.json"))
}

fn cursor_config_path() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|d| d.join(".cursor").join("mcp.json"))
}

fn vscode_config_path() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|d| d.join(".vscode").join("mcp.json"))
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

fn windsurf_config_path() -> Option<PathBuf> {
    // Best-effort: not documented in docs/clients/README.md (Windsurf's own docs
    // point at Settings → Cascade). Codeium/Windsurf's on-disk MCP config lives
    // here as of this writing; verify against your installed version before relying
    // on the diff-write flow, and edit via Settings → Cascade if this is stale.
    home_dir().map(|h| h.join(".codeium").join("windsurf").join("mcp_config.json"))
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
    fn mergeable_targets_cover_seven_clients() {
        let targets = mergeable_targets();
        assert_eq!(targets.len(), 7);
        let keys: Vec<_> = targets.iter().map(|t| t.key).collect();
        for expected in [
            "claude-desktop",
            "claude-code",
            "cursor",
            "vscode",
            "vscode-copilot",
            "zed",
            "windsurf",
        ] {
            assert!(keys.contains(&expected), "missing {expected}");
        }
    }
}
