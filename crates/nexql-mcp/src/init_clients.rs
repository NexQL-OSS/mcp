//! Paste-ready MCP client configs for `nexql-mcp init <client>`.

/// Clients accepted by `nexql-mcp init`.
pub const SUPPORTED_CLIENTS: &[&str] = &[
    "claude",
    "claude-desktop",
    "claude-code",
    "cursor",
    "vscode",
    "vscode-copilot",
    "zed",
    "windsurf",
    "continue",
    "jetbrains",
    "openai-agents",
];

/// Build a paste-ready config snippet for `client`.
///
/// When `url` is `Some`, it is included in the server's `args` (or equivalent).
/// Returns `Err` with a helpful message for unknown clients.
pub fn init_snippet(client: &str, url: Option<&str>) -> Result<String, String> {
    let key = client.trim().to_ascii_lowercase();
    let args_json = args_as_json_array(url);
    let args_yaml = args_as_yaml_list(url);
    let args_toml = args_as_toml_array(url);

    let body = match key.as_str() {
        "claude" | "claude-desktop" => format!(
            r#"// Claude Desktop — merge into claude_desktop_config.json
// macOS: ~/Library/Application Support/Claude/claude_desktop_config.json
// Windows: %APPDATA%\Claude\claude_desktop_config.json
{{
  "mcpServers": {{
    "nexql": {{
      "command": "nexql-mcp",
      "args": {args}
    }}
  }}
}}"#,
            args = args_json
        ),
        "claude-code" => format!(
            r#"// Claude Code — save as .mcp.json (project) or ~/.claude.json mcpServers
{{
  "mcpServers": {{
    "nexql-mcp": {{
      "command": "nexql-mcp",
      "args": {args}
    }}
  }}
}}"#,
            args = args_json
        ),
        "cursor" => format!(
            r#"// Cursor — save as .cursor/mcp.json (project) or Cursor Settings → MCP
{{
  "mcpServers": {{
    "nexql-mcp": {{
      "command": "nexql-mcp",
      "args": {args}
    }}
  }}
}}"#,
            args = args_json
        ),
        "vscode" | "vscode-copilot" => format!(
            r#"// VS Code / Copilot Chat — save as .vscode/mcp.json
{{
  "servers": {{
    "nexql-mcp": {{
      "type": "stdio",
      "command": "nexql-mcp",
      "args": {args}
    }}
  }}
}}"#,
            args = args_json
        ),
        "zed" => format!(
            r#"// Zed — merge into settings.json (context_servers)
{{
  "context_servers": {{
    "nexql-mcp": {{
      "source": "custom",
      "command": "nexql-mcp",
      "args": {args}
    }}
  }}
}}"#,
            args = args_json
        ),
        "windsurf" => format!(
            r#"// Windsurf — Cascade MCP config (mcp_config.json / Settings → Cascade)
{{
  "mcpServers": {{
    "nexql-mcp": {{
      "command": "nexql-mcp",
      "args": {args}
    }}
  }}
}}"#,
            args = args_json
        ),
        "continue" => {
            let args_block = match url {
                Some(_) => format!("    args:\n{args_yaml}"),
                None => "    args: []".into(),
            };
            format!(
                r#"# Continue — merge into ~/.continue/config.yaml (or .continue/config.yaml)
mcpServers:
  - name: nexql-mcp
    command: nexql-mcp
{args_block}"#
            )
        }
        "jetbrains" => format!(
            r#"// JetBrains AI Assistant — Settings → Tools → AI Assistant → MCP
{{
  "mcpServers": {{
    "nexql-mcp": {{
      "command": "nexql-mcp",
      "args": {args}
    }}
  }}
}}"#,
            args = args_json
        ),
        "openai-agents" => format!(
            r#"# OpenAI Agents SDK — MCPServerStdio params (Python) or equivalent JSON
# Python:
#   MCPServerStdio(params={{"command": "nexql-mcp", "args": {args_py}}})
#
# TOML-style params (for wrappers that load TOML):
[mcp.servers.nexql-mcp]
command = "nexql-mcp"
args = {args}"#,
            args = args_toml,
            args_py = args_json
        ),
        other => {
            return Err(format!(
                "unknown client '{other}'. Supported: {}",
                SUPPORTED_CLIENTS.join("|")
            ));
        }
    };

    Ok(body)
}

fn args_as_json_array(url: Option<&str>) -> String {
    match url {
        Some(u) => format!("[\"{}\"]", escape_json_string(u)),
        None => "[]".into(),
    }
}

fn args_as_yaml_list(url: Option<&str>) -> String {
    match url {
        Some(u) => format!("      - \"{}\"", escape_yaml_double_quoted(u)),
        None => String::new(), // omit list items; caller uses `args:` with nothing → empty
    }
}

fn args_as_toml_array(url: Option<&str>) -> String {
    match url {
        Some(u) => format!("[\"{}\"]", escape_toml_string(u)),
        None => "[]".into(),
    }
}

fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn escape_yaml_double_quoted(s: &str) -> String {
    // YAML double-quoted: escape \, ", and control-ish chars we care about.
    escape_json_string(s)
}

fn escape_toml_string(s: &str) -> String {
    escape_json_string(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_clients_nonempty_and_mention_nexql_mcp() {
        let url = Some("postgres://u:p@localhost:5432/db");
        for &client in SUPPORTED_CLIENTS {
            let snippet = init_snippet(client, url).unwrap_or_else(|e| panic!("{client}: {e}"));
            assert!(!snippet.trim().is_empty(), "{client}: empty snippet");
            assert!(
                snippet.contains("nexql-mcp"),
                "{client}: snippet must mention nexql-mcp\n{snippet}"
            );
            assert!(
                snippet.contains("postgres://u:p@localhost:5432/db"),
                "{client}: URL missing from args\n{snippet}"
            );
        }
    }

    #[test]
    fn without_url_args_empty_still_mentions_binary() {
        for &client in SUPPORTED_CLIENTS {
            let snippet = init_snippet(client, None).expect(client);
            assert!(snippet.contains("nexql-mcp"), "{client}");
            // Should not invent a connection string.
            assert!(!snippet.contains("postgres://"), "{client}: unexpected URL");
        }
    }

    #[test]
    fn unknown_client_errors() {
        let err = init_snippet("not-a-client", None).unwrap_err();
        assert!(err.contains("unknown client"));
        assert!(err.contains("cursor"));
    }

    #[test]
    fn continue_is_yaml_openai_is_tomlish() {
        let cont = init_snippet("continue", Some("postgres://x")).unwrap();
        assert!(cont.contains("mcpServers:"));
        assert!(cont.contains("command: nexql-mcp"));

        let agents = init_snippet("openai-agents", Some("postgres://x")).unwrap();
        assert!(agents.contains("[mcp.servers.nexql-mcp]"));
        assert!(agents.contains("command = \"nexql-mcp\""));
    }

    #[test]
    fn vscode_uses_servers_key() {
        let s = init_snippet("vscode", None).unwrap();
        assert!(s.contains("\"servers\""));
        assert!(s.contains("\"type\": \"stdio\""));
    }
}
