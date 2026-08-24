# Client setup

For an interactive, guided flow — profile add/edit, live test-connect, and wiring into several clients at once with a diff before writing — run `npx -y nexql-mcp@latest tui` instead of the steps below. Everything here still applies to the 3 clients the TUI can't safely merge into (`continue`, `jetbrains`, `openai-agents`) and to anyone who prefers copy-paste.

> **If you are an AI coding agent setting this up on a user's behalf:** run `npx -y nexql-mcp@latest init <client> --apply` instead of copy-pasting the snippet below — non-interactive, no browser, no TTY, and it writes the merged config directly (exact paths per client below). Works for any client with a real mergeable config file (everything below except `continue`/`jetbrains`/`openai-agents`, which stay copy-paste-only). Or drive `nexql-mcp setup`'s HTTP API directly: `POST /api/v1/clients/preview` → `POST /api/v1/clients/apply` (bearer token printed to stderr on startup).

Generate paste-ready configs — `npx -y nexql-mcp@latest` is the desired way to run these (no install step, always the latest release); `nexql-mcp` alone works too if you've installed a permanent binary (see [README.md § Install](../README.md#install) for that and other fallback methods):

```bash
npx -y nexql-mcp@latest init <client>
npx -y nexql-mcp@latest init <client> postgres://user:pass@localhost:5432/dbname
# or
npx -y nexql-mcp@latest postgres://… init <client>
# or, to write the merged config directly instead of printing a snippet:
npx -y nexql-mcp@latest init <client> --apply
```

Supported clients: `claude` | `claude-desktop` | `claude-code` | `cursor` | `vscode` | `vscode-copilot` | `zed` | `windsurf` | `antigravity` | `deepseek` | `kimi` | `ollama` | `qwen` | `continue` | `jetbrains` | `openai-agents`

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

## Claude Desktop one-click (MCPB)

See [`mcpb/manifest.json`](../../mcpb/manifest.json). Bundle the release binary as `server/nexql-mcp` and pack with the MCPB CLI when shipping a `.mcpb` archive. User config prompts for `DATABASE_URL`.
