# Client setup

For an interactive, guided flow — profile add/edit, live test-connect, and wiring into several clients at once with a diff before writing — run `npx -y nexql-mcp@latest tui` instead of the steps below. Everything here still applies to the 5 clients the TUI can't safely merge into (`continue`, `goose`, `opencode`, `jetbrains`, `openai-agents`) and to anyone who prefers copy-paste.

> **If you are an AI coding agent setting this up on a user's behalf:** run `npx -y nexql-mcp@latest init <client> --apply` instead of copy-pasting the snippet below — non-interactive, no browser, no TTY, and it writes the merged config directly (exact paths per client below). Works for any client with a real mergeable config file (everything below except `continue`/`goose`/`opencode`/`jetbrains`/`openai-agents`, which stay copy-paste-only). Or drive `nexql-mcp setup`'s HTTP API directly: `POST /api/v1/clients/preview` → `POST /api/v1/clients/apply` (bearer token printed to stderr on startup).

Generate paste-ready configs — `npx -y nexql-mcp@latest` is the desired way to run these (no install step, always the latest release); `nexql-mcp` alone works too if you've installed a permanent binary (see [README.md § Install](../README.md#install) for that and other fallback methods):

```bash
npx -y nexql-mcp@latest init <client>
npx -y nexql-mcp@latest init <client> postgres://user:pass@localhost:5432/dbname
# or
npx -y nexql-mcp@latest postgres://… init <client>
# or, to write the merged config directly instead of printing a snippet:
npx -y nexql-mcp@latest init <client> --apply
```

Supported clients: `claude` | `claude-desktop` | `claude-code` | `cursor` | `vscode` | `vscode-copilot` | `zed` | `windsurf` | `antigravity` | `gemini-cli` | `codex` | `copilot-cli` | `cline` | `lm-studio` | `kiro` | `kimi` | `qwen` | `goose` | `opencode` | `continue` | `jetbrains` | `openai-agents`

ChatGPT (chat, not Codex) can't launch local servers, so it has no `init` target. See [ChatGPT](#chatgpt-no-init-target).

**Context-window budget:** add `--tools query|dba|meta|full` (env `NEXQL_MCP_TOOLS`) to any of the commands above to restrict which tools are advertised to the client — advisory only, not a security boundary (that's `access_mode`). See [README.md § Tool surface / context budget](../README.md#tool-surface--context-budget).

---

## Claude Desktop (`claude` / `claude-desktop`)

Config file:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

```bash
npx -y nexql-mcp@latest init claude-desktop postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

Restart Claude Desktop after editing.

---

## Claude Code (`claude-code`)

Project file `.mcp.json` (or user-level Claude Code MCP settings):

```bash
npx -y nexql-mcp@latest init claude-code postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## Cursor (`cursor`)

Project: `.cursor/mcp.json` — or Cursor Settings → MCP.

```bash
npx -y nexql-mcp@latest init cursor postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## VS Code (`vscode`)

Workspace: `.vscode/mcp.json`.

```bash
npx -y nexql-mcp@latest init vscode postgres://dev@localhost:5432/appdb
```

```json
{
  "servers": {
    "nexql-mcp": {
      "type": "stdio",
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## VS Code Copilot Chat (`vscode-copilot`)

Same shape as VS Code MCP (`.vscode/mcp.json`):

```bash
npx -y nexql-mcp@latest init vscode-copilot postgres://dev@localhost:5432/appdb
```

```json
{
  "servers": {
    "nexql-mcp": {
      "type": "stdio",
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## Zed (`zed`)

Merge into Zed `settings.json` under `context_servers`:

```bash
npx -y nexql-mcp@latest init zed postgres://dev@localhost:5432/appdb
```

```json
{
  "context_servers": {
    "nexql-mcp": {
      "source": "custom",
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## Windsurf (`windsurf`)

Cascade MCP config (`mcp_config.json` / Settings → Cascade):

```bash
npx -y nexql-mcp@latest init windsurf postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## Google Antigravity (`antigravity`)

`~/.gemini/config/mcp_config.json`, shared by the Antigravity app, IDE and CLI (Settings → Customizations → Open MCP Config).

```bash
npx -y nexql-mcp@latest init antigravity postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## Gemini CLI (`gemini-cli`)

`--apply` merges into `~/.gemini/settings.json` and keeps every other setting. For one project only, paste the same block into `.gemini/settings.json`.

```bash
npx -y nexql-mcp@latest init gemini-cli postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## OpenAI Codex (`codex`)

`~/.codex/config.toml`. The Codex CLI, the IDE extension and the ChatGPT desktop app all read this file. `--apply` adds a `[mcp_servers.nexql-mcp]` table and leaves your other settings, servers and comments alone. The ChatGPT desktop app doesn't see your shell `PATH`, so the command is written as an absolute path, like Claude Desktop's.

```bash
npx -y nexql-mcp@latest init codex postgres://dev@localhost:5432/appdb
```

```toml
[mcp_servers.nexql-mcp]
command = "/usr/local/bin/nexql-mcp"
args = ["postgres://dev@localhost:5432/appdb"]
```

Trusted projects can also use `.codex/config.toml`.

---

## GitHub Copilot CLI (`copilot-cli`)

`~/.copilot/mcp-config.json`. Copilot CLI also reads `.mcp.json` and `.github/mcp.json` in the repository. Paste the same block there to share it with a project.

```bash
npx -y nexql-mcp@latest init copilot-cli postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "type": "local",
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"],
      "tools": ["*"]
    }
  }
}
```

---

## Cline (`cline`)

`--apply` writes the Cline CLI's `~/.cline/data/settings/cline_mcp_settings.json`, or `$CLINE_MCP_SETTINGS_PATH` when that is set. In the VS Code extension, open the MCP Servers icon → Configure → Configure MCP Servers and paste the snippet there.

```bash
npx -y nexql-mcp@latest init cline postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## LM Studio (`lm-studio`)

`~/.lmstudio/mcp.json` (`%USERPROFILE%\.lmstudio\mcp.json` on Windows), or in the app: Program tab → Install → Edit mcp.json. LM Studio is started from the Dock or Start menu, so the command is written as an absolute path.

```bash
npx -y nexql-mcp@latest init lm-studio postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## Kiro, formerly Amazon Q Developer CLI (`kiro`)

`~/.kiro/settings/mcp.json`, used by the Kiro IDE and the Kiro CLI. Amazon Q Developer CLI is now Kiro CLI. For one workspace only, paste the block into `.kiro/settings/mcp.json`.

```bash
npx -y nexql-mcp@latest init kiro postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## Kimi Code CLI (`kimi`)

`~/.kimi-code/mcp.json` (or `$KIMI_CODE_HOME/mcp.json`). For one project only, use `.kimi-code/mcp.json`.

```bash
npx -y nexql-mcp@latest init kimi postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## Qwen Code (`qwen`)

`--apply` merges into `~/.qwen/settings.json` and keeps every other setting. For one project only, use `.qwen/settings.json`.

```bash
npx -y nexql-mcp@latest init qwen postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## Continue (`continue`)

Merge into `~/.continue/config.yaml` (or project `.continue/config.yaml`):

```bash
npx -y nexql-mcp@latest init continue postgres://dev@localhost:5432/appdb
```

```yaml
mcpServers:
  - name: nexql-mcp
    command: nexql-mcp
    args:
      - "postgres://dev@localhost:5432/appdb"
```

---

## Goose (`goose`)

Copy-only: Goose keeps extensions in YAML. Merge into `~/.config/goose/config.yaml` (`%APPDATA%\Block\goose\config\config.yaml` on Windows), or add it with `goose configure`:

```bash
npx -y nexql-mcp@latest init goose postgres://dev@localhost:5432/appdb
```

```yaml
extensions:
  nexql-mcp:
    name: nexql-mcp
    type: stdio
    enabled: true
    cmd: nexql-mcp
    args: ["postgres://dev@localhost:5432/appdb"]
    envs: {}
    env_keys: []
    timeout: 300
```

---

## opencode (`opencode`)

Copy-only: opencode configs are often JSONC, and OpenCode v2 moved servers from `mcp.<name>` to `mcp.servers.<name>`. Merge into `~/.config/opencode/opencode.json` or the project's `opencode.json`:

```bash
npx -y nexql-mcp@latest init opencode postgres://dev@localhost:5432/appdb
```

```json
{
  "mcp": {
    "nexql-mcp": {
      "type": "local",
      "command": ["nexql-mcp", "postgres://dev@localhost:5432/appdb"],
      "enabled": true
    }
  }
}
```

On OpenCode v2, put the `nexql-mcp` entry under `"mcp": { "servers": { … } }` instead.

---

## JetBrains AI Assistant (`jetbrains`)

Settings → Tools → AI Assistant → MCP:

```bash
npx -y nexql-mcp@latest init jetbrains postgres://dev@localhost:5432/appdb
```

```json
{
  "mcpServers": {
    "nexql-mcp": {
      "command": "nexql-mcp",
      "args": ["postgres://dev@localhost:5432/appdb"]
    }
  }
}
```

---

## OpenAI Agents SDK (`openai-agents`)

```bash
npx -y nexql-mcp@latest init openai-agents postgres://dev@localhost:5432/appdb
```

Python (`agents.mcp.MCPServerStdio`):

```python
MCPServerStdio(params={
    "command": "nexql-mcp",
    "args": ["postgres://dev@localhost:5432/appdb"],
})
```

TOML-style params (for wrappers that load TOML):

```toml
[mcp.servers.nexql-mcp]
command = "nexql-mcp"
args = ["postgres://dev@localhost:5432/appdb"]
```

---

## ChatGPT (no `init` target)

ChatGPT's custom connectors (Developer mode) only talk to remote MCP servers over HTTPS, using SSE or streamable HTTP. ChatGPT can't start a local process, so there is no config file for `init` to write. Codex inside the ChatGPT desktop app is different: it reads `~/.codex/config.toml`, so use [`init codex`](#openai-codex-codex) for it.

To reach nexql-mcp from ChatGPT chat, run it with `--http` (streamable HTTP at `/mcp`, port 8899 by default) and put it behind a public HTTPS URL, such as a TLS reverse proxy or a tunnel.

Authentication is the hard part. ChatGPT offers three options for a connector: OAuth, no authentication, or a mix of the two. It doesn't send a static bearer token or custom headers. nexql-mcp's `--http-token` checks `Authorization: Bearer <token>`, so ChatGPT gets `401` when a token is set. nexql-mcp also refuses to listen on a non-loopback address without a token. That leaves two options:

- **Put an OAuth 2.1 server in front.** ChatGPT registers itself through dynamic client registration or a client ID metadata document, or uses a client you set up in advance. nexql-mcp's free build doesn't include an OAuth gateway, so you run the proxy yourself.
- **Use "No authentication" for a short test only.** Run nexql-mcp on `127.0.0.1` and point a tunnel at it. Anyone who learns the URL can then run queries as that profile's database role. Only do this with a read-only role on data you'd be happy to make public, and close the tunnel when you're done.

Which ChatGPT plans have Developer mode, and whether they can use write tools, is still changing. Check OpenAI's [Developer mode help article](https://help.openai.com/en/articles/12584461-developer-mode-and-mcp-apps-in-chatgpt) and the [custom MCP server guide](https://developers.openai.com/api/docs/guides/custom-mcp-server).

---

## Claude Desktop one-click (MCPB)

See [`mcpb/manifest.json`](../../mcpb/manifest.json). Bundle the release binary as `server/nexql-mcp` and pack with the MCPB CLI when shipping a `.mcpb` archive. User config prompts for `DATABASE_URL`.
