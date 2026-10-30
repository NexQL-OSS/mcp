// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! Paste-ready MCP client configs for `nexql-mcp init <client>`.

use crate::client_targets::ConfigShape;

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
    "gemini-cli",
    "codex",
    "copilot-cli",
    "cline",
    "lm-studio",
    "kiro",
    "kimi",
    "qwen",
    "goose",
    "opencode",
];

/// `init <client>` help text, listing every supported client.
pub fn client_arg_help() -> String {
    format!(
        "Target client: {}. If omitted, opens the TUI wizard.",
        SUPPORTED_CLIENTS.join(" | ")
    )
}

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
        "claude" | "claude-desktop" => desktop_app_snippet(
            "// Claude Desktop — merge into claude_desktop_config.json\n\
             // macOS: ~/Library/Application Support/Claude/claude_desktop_config.json\n\
             // Windows: %APPDATA%\\Claude\\claude_desktop_config.json",
            "//",
            "Claude Desktop",
            ConfigShape::Claude,
            "nexql",
            args,
        )?,
        "codex" => desktop_app_snippet(
            "# OpenAI Codex — merge into ~/.codex/config.toml\n\
             # The Codex CLI, IDE extension and ChatGPT desktop app all read this file.",
            "#",
            "ChatGPT's desktop app",
            ConfigShape::Codex,
            "nexql-mcp",
            args,
        )?,
        "lm-studio" => desktop_app_snippet(
            "// LM Studio — merge into ~/.lmstudio/mcp.json (Windows: %USERPROFILE%\\.lmstudio\\mcp.json)\n\
             // or in the app: Program tab → Install → Edit mcp.json",
            "//",
            "LM Studio",
            ConfigShape::Claude,
            "nexql-mcp",
            args,
        )?,
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
        "antigravity" => mcp_servers_snippet(
            "// Google Antigravity — merge into ~/.gemini/config/mcp_config.json\n\
             // (Settings → Customizations → Open MCP Config)",
            &args_json,
        ),
        "gemini-cli" => mcp_servers_snippet(
            "// Gemini CLI — merge into ~/.gemini/settings.json (or project .gemini/settings.json)",
            &args_json,
        ),
        "cline" => mcp_servers_snippet(
            "// Cline — CLI: merge into ~/.cline/data/settings/cline_mcp_settings.json\n\
             // VS Code extension: MCP Servers icon → Configure → Configure MCP Servers",
            &args_json,
        ),
        "kiro" => mcp_servers_snippet(
            "// Kiro IDE / CLI (formerly Amazon Q Developer CLI) — merge into\n\
             // ~/.kiro/settings/mcp.json (or workspace .kiro/settings/mcp.json)",
            &args_json,
        ),
        "kimi" => mcp_servers_snippet(
            "// Kimi Code CLI — merge into ~/.kimi-code/mcp.json (or project .kimi-code/mcp.json)",
            &args_json,
        ),
        "qwen" => mcp_servers_snippet(
            "// Qwen Code — merge into ~/.qwen/settings.json (or project .qwen/settings.json)",
            &args_json,
        ),
        "copilot-cli" => format!(
            r#"// GitHub Copilot CLI — merge into ~/.copilot/mcp-config.json
{{
  "mcpServers": {{
    "nexql-mcp": {{
      "type": "local",
      "command": "nexql-mcp",
      "args": {args},
      "tools": ["*"]
    }}
  }}
}}"#,
            args = args_json
        ),
        "goose" => format!(
            r#"# Goose — merge into ~/.config/goose/config.yaml
# (Windows: %APPDATA%\Block\goose\config\config.yaml), or run `goose configure`
extensions:
  nexql-mcp:
    name: nexql-mcp
    type: stdio
    enabled: true
    cmd: nexql-mcp
    args: {args}
    envs: {{}}
    env_keys: []
    timeout: 300"#,
            args = args_json
        ),
        "opencode" => {
            let command = args_as_json_array(
                &std::iter::once("nexql-mcp".to_owned())
                    .chain(args.iter().cloned())
                    .collect::<Vec<_>>(),
            );
            format!(
                r#"// opencode — merge into ~/.config/opencode/opencode.json (or project opencode.json)
// OpenCode v2 nests servers one level deeper: "mcp": {{ "servers": {{ "nexql-mcp": … }} }}
{{
  "mcp": {{
    "nexql-mcp": {{
      "type": "local",
      "command": {command},
      "enabled": true
    }}
  }}
}}"#
            )
        }
        other => {
            return Err(format!(
                "unknown client '{other}'. Supported: {}",
                SUPPORTED_CLIENTS.join("|")
            ));
        }
    };

    Ok(body)
}

/// A client launched from the Dock or Start menu rather than a shell: name the
/// program by absolute path (see `launch.rs`) and say so in a comment.
fn desktop_app_snippet(
    header: &str,
    comment: &str,
    app: &str,
    shape: ConfigShape,
    server_key: &str,
    args: &[String],
) -> Result<String, String> {
    let launch = crate::launch::absolute_launch("nexql-mcp", args);
    let entry = crate::client_targets::merge_entry_with_env(
        "",
        shape,
        server_key,
        &launch.command,
        &launch.args,
        &launch.env,
    )?;
    let note = if std::path::Path::new(&launch.command).is_absolute() {
        format!("{comment} The command is an absolute path: {app} does not see your shell PATH.")
    } else {
        format!(
            "{comment} nexql-mcp was not found on PATH. Replace \"command\" with its absolute path:\n\
             {comment} {app} does not see your shell PATH."
        )
    };
    Ok(format!("{header}\n{note}\n{entry}"))
}

/// The common `{"mcpServers": {"nexql-mcp": {command, args}}}` shape.
fn mcp_servers_snippet(header: &str, args_json: &str) -> String {
    format!(
        r#"{header}
{{
  "mcpServers": {{
    "nexql-mcp": {{
      "command": "nexql-mcp",
      "args": {args_json}
    }}
  }}
}}"#
    )
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

        // `@`, `#`, `?` inside a password must be percent-encoded. The decoded
        // secret is still a password, so the URL is not written into a snippet.
        for url in [
            "postgres://u:p%40ss@localhost:5432/db",
            "postgres://u:p%23ss@localhost:5432/db",
            "postgres://u:p%3Fss@localhost:5432/db",
            "postgres://u:p%2Fss@localhost:5432/db",
            "postgresql://u@localhost/db?password=s3cret%2Fpath",
        ] {
            assert!(url_contains_password(url), "{url}");
            assert!(init_snippet("cursor", Some(url)).is_err(), "{url}");
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
    fn removed_clients_are_unknown() {
        for client in ["deepseek", "ollama"] {
            assert!(!SUPPORTED_CLIENTS.contains(&client));
            assert!(init_snippet(client, None).is_err(), "{client}");
        }
    }

    #[test]
    fn json_snippets_parse_after_comment_lines() {
        let url = Some("postgres://u@localhost:5432/db");
        for client in [
            "antigravity",
            "gemini-cli",
            "copilot-cli",
            "cline",
            "lm-studio",
            "kiro",
            "kimi",
            "qwen",
            "opencode",
        ] {
            let snippet = init_snippet(client, url).unwrap();
            let json: String = snippet
                .lines()
                .filter(|l| !l.starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            let v: serde_json::Value =
                serde_json::from_str(&json).unwrap_or_else(|e| panic!("{client}: {e}\n{json}"));
            assert!(v.is_object(), "{client}");
        }
    }

    #[test]
    fn copilot_cli_and_opencode_shapes() {
        let copilot = init_snippet("copilot-cli", None).unwrap();
        assert!(copilot.contains("\"type\": \"local\""), "{copilot}");
        assert!(copilot.contains("\"tools\": [\"*\"]"), "{copilot}");

        let opencode = init_snippet("opencode", Some("postgres://u@h/db")).unwrap();
        assert!(opencode.contains("\"mcp\": {"), "{opencode}");
        assert!(
            opencode.contains(r#""command": ["nexql-mcp", "postgres://u@h/db"]"#),
            "{opencode}"
        );
    }

    #[test]
    fn codex_snippet_is_toml_under_mcp_servers() {
        let snippet = init_snippet_with_profile("codex", "shop").unwrap();
        let toml_text: String = snippet
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let v: toml::Value = toml::from_str(&toml_text).unwrap();
        let entry = &v["mcp_servers"]["nexql-mcp"];
        assert!(entry["command"].as_str().is_some(), "{snippet}");
        let args: Vec<_> = entry["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a.as_str())
            .collect();
        assert!(args.ends_with(&["--profile", "shop"]), "{snippet}");
    }

    #[test]
    fn goose_snippet_is_stdio_extension() {
        let goose = init_snippet("goose", Some("postgres://x")).unwrap();
        assert!(goose.contains("extensions:"), "{goose}");
        assert!(goose.contains("type: stdio"), "{goose}");
        assert!(goose.contains("cmd: nexql-mcp"), "{goose}");
    }

    #[test]
    fn vscode_uses_servers_key() {
        let s = init_snippet("vscode", None).unwrap();
        assert!(s.contains("\"servers\""));
        assert!(s.contains("\"type\": \"stdio\""));
    }
}
