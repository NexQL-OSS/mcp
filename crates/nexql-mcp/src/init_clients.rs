// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

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
    "antigravity",
    "deepseek",
    "kimi",
    "ollama",
    "qwen",
];

/// True when `url` carries a password (userinfo or a `password=` query
/// parameter), or cannot be parsed, so its absence cannot be proven. Those
/// must never be written into a client config; save a profile and pass
/// `--profile` instead.
pub fn url_contains_password(url: &str) -> bool {
    let Ok(params) = nexql_conn::params_from_url(url) else {
        return true;
    };
    if params.password.is_some_and(|password| !password.is_empty()) {
        return true;
    }
    url::Url::parse(url).is_ok_and(|parsed| {
        parsed
            .query_pairs()
            .any(|(key, value)| key.eq_ignore_ascii_case("password") && !value.is_empty())
    })
}

/// Build a paste-ready config snippet for `client`.
///
/// When `url` is `Some`, it is included in the server's `args` (or equivalent).
/// A URL that contains a password, or cannot be parsed, is refused.
/// Returns `Err` with a helpful message for unknown clients.
pub fn init_snippet(client: &str, url: Option<&str>) -> Result<String, String> {
    if let Some(url) = url {
        if let Err(e) = nexql_conn::params_from_url(url) {
            return Err(format!(
                "could not parse the connection URL ({e}). Percent-encode reserved characters \
                 in the password (for example # as %23, / as %2F, ? as %3F) and retry."
            ));
        }
        if url_contains_password(url) {
            return Err(
                "refusing to write a database password into client config. Save a profile \
                 (`nexql-mcp init <client> <url>` stores it in the keyring) and reference it \
                 with --profile."
                    .into(),
            );
        }
    }
    let args = url.map(|value| vec![value.to_owned()]).unwrap_or_default();
    render_snippet(client, &args)
}

/// Snippet that references a saved profile instead of embedding a URL.
pub fn init_snippet_with_profile(client: &str, profile: &str) -> Result<String, String> {
    render_snippet(client, &["--profile".to_owned(), profile.to_owned()])
}

fn render_snippet(client: &str, args: &[String]) -> Result<String, String> {
    let key = client.trim().to_ascii_lowercase();
    let args_json = args_as_json_array(args);
    let args_yaml = args_as_yaml_list(args);
    let args_toml = args_as_toml_array(args);

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
            let args_block = if args.is_empty() {
                "    args: []".to_owned()
            } else {
                format!("    args:\n{args_yaml}")
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
        "antigravity" => format!(
            r#"// Antigravity (Google DeepMind AI IDE) — merge into ~/.gemini/antigravity-ide/mcp_config.json
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
        "deepseek" => format!(
            r#"// DeepSeek AI / DeepSeek Coder — save as .deepseek/mcp.json or ~/.config/deepseek/mcp.json
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
        "kimi" => format!(
            r#"// Kimi (Moonshot AI) — save as .kimi/mcp.json or ~/.config/kimi/mcp.json
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
        "ollama" => format!(
            r#"// Ollama (Local LLMs) — save as .ollama/mcp.json or ~/.ollama/mcp.json
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
        "qwen" => format!(
            r#"// Qwen (Alibaba Cloud AI) — save as .qwen/mcp.json or ~/.config/qwen/mcp.json
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
        other => {
            return Err(format!(
                "unknown client '{other}'. Supported: {}",
                SUPPORTED_CLIENTS.join("|")
            ));
        }
    };

    Ok(body)
}

fn args_as_json_array(args: &[String]) -> String {
    if args.is_empty() {
        return "[]".into();
    }
    let parts: Vec<String> = args
        .iter()
        .map(|arg| format!("\"{}\"", escape_json_string(arg)))
        .collect();
    format!("[{}]", parts.join(", "))
}

fn args_as_yaml_list(args: &[String]) -> String {
    args.iter()
        .map(|arg| format!("      - \"{}\"", escape_yaml_double_quoted(arg)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn args_as_toml_array(args: &[String]) -> String {
    if args.is_empty() {
        return "[]".into();
    }
    let parts: Vec<String> = args
        .iter()
        .map(|arg| format!("\"{}\"", escape_toml_string(arg)))
        .collect();
    format!("[{}]", parts.join(", "))
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
        let url = Some("postgres://u@localhost:5432/db");
        for &client in SUPPORTED_CLIENTS {
            let snippet = init_snippet(client, url).unwrap_or_else(|e| panic!("{client}: {e}"));
            assert!(!snippet.trim().is_empty(), "{client}: empty snippet");
            assert!(
                snippet.contains("nexql-mcp"),
                "{client}: snippet must mention nexql-mcp\n{snippet}"
            );
            assert!(
                snippet.contains("postgres://u@localhost:5432/db"),
                "{client}: URL missing from args\n{snippet}"
            );
        }
    }

    #[test]
    fn password_url_is_refused_and_profile_snippet_omits_it() {
        let url = "postgres://u:s3cret-batch1@localhost:5432/db";
        let err = init_snippet("cursor", Some(url)).unwrap_err();
        assert!(err.contains("password"), "{err}");
        let snippet = init_snippet_with_profile("cursor", "u_db").unwrap();
        assert!(snippet.contains("--profile"), "{snippet}");
        assert!(snippet.contains("u_db"), "{snippet}");
        assert!(!snippet.contains("s3cret-batch1"), "{snippet}");
    }

    #[test]
    fn unparseable_or_query_password_urls_are_refused() {
        // `#` starts a fragment, so this URL does not parse; it must not be
        // treated as passwordless and embedded verbatim.
        let unparseable = "postgres://u:p#ss@localhost:5432/db";
        assert!(url_contains_password(unparseable));
        let err = init_snippet("cursor", Some(unparseable)).unwrap_err();
        assert!(err.contains("Percent-encode"), "{err}");

        let query = "postgres://u@localhost:5432/db?password=s3cret";
        assert!(url_contains_password(query));
        assert!(init_snippet("cursor", Some(query)).is_err());

        assert!(!url_contains_password(
            "postgres://u@localhost:5432/db?sslmode=require"
        ));
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
