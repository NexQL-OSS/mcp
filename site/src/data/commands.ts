export interface CliFlag {
  flag: string;
  env?: string;
  description: string;
}

export interface CliCommand {
  name: string;
  usage?: string;
  description: string;
  flags?: CliFlag[];
  subcommands?: CliCommand[];
}

export const globalFlags: CliFlag[] = [
  { flag: "postgres://…", description: "Connection string (highest-precedence source)." },
  { flag: "--profile", description: "Named profile from config.toml." },
  { flag: "-d, --dbname", description: "Database name override." },
  { flag: "--host", description: "PostgreSQL host." },
  { flag: "-p, --port", description: "PostgreSQL port." },
  { flag: "-U, --user", description: "PostgreSQL user." },
  { flag: "--config", description: "Path to config.toml (or NEXQL_MCP_CONFIG)." },
  { flag: "--workspace-root", env: "NEXQL_MCP_WORKSPACE_ROOT", description: "Project root for .nexql/config.toml discovery." },
  { flag: "--access-mode", description: "read (default), write, or admin." },
  { flag: "--max-rows", description: "Cap rows returned per query." },
  { flag: "--embeddings", env: "NEXQL_MCP_EMBEDDINGS", description: "off | local — MiniLM embeddings for index and search." },
  { flag: "--tools", env: "NEXQL_MCP_TOOLS", description: "query | dba | meta | full — tool surface profile." },
  { flag: "--stdio", description: "Stdio MCP transport (default for desktop clients)." },
  { flag: "--http", description: "Streamable HTTP transport." },
  { flag: "--http-port", description: "HTTP port (default 8899)." },
  { flag: "--bind", description: "HTTP bind address (default 127.0.0.1)." },
  { flag: "--http-token", env: "NEXQL_MCP_HTTP_TOKEN", description: "Bearer token; required for non-loopback HTTP binds." },
  { flag: "--http-rate-limit", env: "NEXQL_MCP_HTTP_RATE_LIMIT", description: "Requests per 60s per token/IP (0 disables)." },
  { flag: "--i-know-what-im-doing", description: "Allow write/admin against superuser connections." },
  { flag: "--managed-extension", env: "NEXQL_MCP_MANAGED", description: "Read-only managed mode; excludes profile mutation tools." },
];

export const cliCommands: CliCommand[] = [
  {
    name: "nexql-mcp",
    usage: "nexql-mcp [OPTIONS] [postgres://…]",
    description:
      "Run the MCP server (stdio or HTTP). With no connection resolved, launches the interactive TUI.",
    subcommands: [
      {
        name: "doctor",
        description: "Verify connection, permissions, extensions, and index state.",
      },
      {
        name: "init",
        usage: "init [CLIENT] [--tui]",
        description: "Print paste-ready MCP JSON for a client, or open the TUI wizard.",
        flags: [
          { flag: "--tui", description: "Force interactive onboarding wizard." },
        ],
      },
      {
        name: "onboarding",
        description: "Interactive AI model onboarding wizard.",
      },
      {
        name: "tui",
        description: "Profile editor + multi-client config merge with diff and backup.",
      },
      {
        name: "query",
        usage: "query <SQL> [-f table|json|csv]",
        description: "Execute a read-only SELECT/WITH in the terminal.",
        flags: [{ flag: "-f, --format", description: "Output format: table, json, csv." }],
      },
      {
        name: "diff",
        usage: "diff <source_schema> <target_schema> [--migration]",
        description: "Compare two schemas on the current connection.",
        flags: [{ flag: "--migration", description: "Emit step-by-step migration SQL." }],
      },
      {
        name: "index",
        description: "Offline schema index maintenance.",
        subcommands: [
          {
            name: "build",
            description: "Build or rebuild the schema index.",
            flags: [
              {
                flag: "--depth",
                env: "NEXQL_MCP_INDEX_DEPTH",
                description: "structure | stats | profiles (profiles enables sample_values data).",
              },
            ],
          },
          { name: "status", description: "Show index build status." },
          { name: "refresh", description: "Incremental index refresh." },
          {
            name: "clear",
            description: "Clear index data.",
            flags: [{ flag: "--all", description: "Clear every index under the index root." }],
          },
        ],
      },
      {
        name: "profile",
        description: "Manage connection profiles in config.toml.",
        subcommands: [
          { name: "list", description: "List configured profiles." },
          {
            name: "add",
            usage: "add <name> [OPTIONS]",
            description: "Add a new profile.",
            flags: [
              { flag: "--url", description: "postgres:// connection URL." },
              { flag: "--host, --port, --dbname, --user", description: "Discrete connection fields." },
              { flag: "--password", description: "Inline password (migrates to keyring on load)." },
              { flag: "--password-command", description: "Shell command to fetch password." },
              { flag: "--password-file", description: "Read password from file." },
              { flag: "--credential-provider", description: "e.g. aws-iam for RDS IAM." },
              { flag: "--access-mode", description: "read, write, or admin." },
              { flag: "--set-default", description: "Set as default_profile." },
              { flag: "--no-test", description: "Skip live connection test." },
            ],
          },
          { name: "set-password", usage: "set-password <name>", description: "Set password for a profile (keyring)." },
          { name: "test", usage: "test [name]", description: "Test a profile connection." },
          {
            name: "export",
            description: "Export profile(s) without secrets.",
            flags: [
              { flag: "--format", description: "project (.nexql/config.toml) or full." },
              { flag: "--output", description: "Output file (default stdout)." },
            ],
          },
          { name: "import", usage: "import <path>", description: "Import profile from file." },
          { name: "migrate-secrets", description: "Migrate legacy plaintext passwords to OS keyring." },
        ],
      },
    ],
  },
];

export const initClients = [
  "claude",
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
  "continue",
  "jetbrains",
  "openai-agents",
];
