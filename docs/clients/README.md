# Client setup

For an interactive, guided flow — profile add/edit, live test-connect, and wiring into several clients at once with a diff before writing — run `nexql-mcp tui` instead of the steps below. Everything here still applies to the 3 clients the TUI can't safely merge into (`continue`, `jetbrains`, `openai-agents`) and to anyone who prefers copy-paste.

Generate paste-ready configs:

```bash
nexql-mcp init <client>
nexql-mcp init <client> postgres://user:pass@localhost:5432/dbname
# or
nexql-mcp postgres://… init <client>
```

Supported clients: `claude` | `claude-desktop` | `claude-code` | `cursor` | `vscode` | `vscode-copilot` | `zed` | `windsurf` | `antigravity` | `deepseek` | `kimi` | `ollama` | `qwen` | `continue` | `jetbrains` | `openai-agents`

Install the binary first (`cargo install --path crates/nexql-mcp`, GitHub Release, `npx -y nexql-mcp`, or Docker) so the `command` resolves on `PATH`.

---

## Claude Desktop (`claude` / `claude-desktop`)

Config file:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

```bash
nexql-mcp init claude-desktop postgres://dev@localhost:5432/appdb
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
nexql-mcp init claude-code postgres://dev@localhost:5432/appdb
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
nexql-mcp init cursor postgres://dev@localhost:5432/appdb
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
nexql-mcp init vscode postgres://dev@localhost:5432/appdb
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
nexql-mcp init vscode-copilot postgres://dev@localhost:5432/appdb
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
nexql-mcp init zed postgres://dev@localhost:5432/appdb
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
nexql-mcp init windsurf postgres://dev@localhost:5432/appdb
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
nexql-mcp init continue postgres://dev@localhost:5432/appdb
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
nexql-mcp init jetbrains postgres://dev@localhost:5432/appdb
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
nexql-mcp init openai-agents postgres://dev@localhost:5432/appdb
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
