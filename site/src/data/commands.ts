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
  { flag: "postgres://…", description: "Full connection string. Highest precedence — beats flags, environment and config file." },
  { flag: "--profile", description: "Use a named profile from config.toml. Repeatable to preload several." },
  { flag: "-d, --dbname", description: "Database name, when you are not passing a full URL." },
  { flag: "--host", description: "Host to connect to." },
  { flag: "-p, --port", description: "Port. Defaults to 5432." },
  { flag: "-U, --user", description: "Role to connect as." },
  { flag: "--config", description: "Config file location. Defaults to ~/.config/nexql-mcp/config.toml." },
  { flag: "--workspace-root", env: "NEXQL_MCP_WORKSPACE_ROOT", description: "Where to look for a workspace-local .nexql/config.toml." },
  { flag: "--access-mode", description: "read (default), write or admin. Enforced when the tool call arrives, on top of a read-only transaction default." },
  { flag: "--max-rows", description: "Ceiling on rows returned per query. Per-call limits cannot exceed it." },
  { flag: "--embeddings", env: "NEXQL_MCP_EMBEDDINGS", description: "local adds MiniLM vectors to the index and fuses them with lexical hits. Runs on your machine; off by default." },
  { flag: "--tools", env: "NEXQL_MCP_TOOLS", description: "Trim the tool surface to fit the context budget: query (21), dba (26), meta (13 + discover_tools), full (54, default)." },
  { flag: "--stdio", description: "Stdio transport. Already the default — no port, nothing to authenticate." },
  { flag: "--http", description: "Serve streamable HTTP instead. For clients that cannot spawn a process." },
  { flag: "--http-port", description: "Port for the HTTP transport. Defaults to 8899." },
  { flag: "--bind", description: "Bind address. Anything other than loopback requires a bearer token." },
  { flag: "--http-token", env: "NEXQL_MCP_HTTP_TOKEN", description: "Bearer token for HTTP. Mandatory off loopback — the server refuses to start without it." },
  { flag: "--http-rate-limit", env: "NEXQL_MCP_HTTP_RATE_LIMIT", description: "Requests per 60 seconds, per token or per client IP. 0 disables the limit." },
  { flag: "--i-know-what-im-doing", description: "Permit write or admin mode against a superuser connection. Deliberately awkward to type." },
  { flag: "--managed-extension", env: "NEXQL_MCP_MANAGED", description: "Read-only mode for embedding in another product. Drops the profile-mutation tools entirely." },
];

export const cliCommands: CliCommand[] = [
  {
    name: "nexql-mcp",
    usage: "nexql-mcp [OPTIONS] [postgres://…]",
    description:
      "Run the MCP server. Stdio unless --http is passed. With no connection resolved from anywhere, it opens the interactive TUI instead of failing.",
    subcommands: [
      {
        name: "doctor",
        description: "Check the connection, permissions, extensions and index state. Run this first when something is not working — it isolates the problem from any MCP client.",
      },
      {
        name: "init",
        usage: "init [CLIENT] [--tui]",
        description: "Write the right config for a named client, merging into what is already there and backing it up first. With no client named, opens the wizard.",
        flags: [
          { flag: "--tui", description: "Open the wizard even when a client was named." },
        ],
      },
      {
        name: "onboarding",
        description: "Guided model and client onboarding, for setting up from scratch.",
      },
      {
        name: "tui",
        description: "Full-screen profile editor and client wiring. Shows a diff and writes a timestamped backup before changing any client config. Keys: n new, e edit, d delete, t test, w wire, q quit.",
      },
      {
        name: "query",
        usage: "query <SQL> [-f table|json|csv]",
        description: "Run a read-only SELECT or WITH straight from the shell — useful for checking that a profile reaches what you expect.",
        flags: [{ flag: "-f, --format", description: "table (default), json or csv." }],
      },
      {
        name: "diff",
        usage: "diff <source_schema> <target_schema> [--migration]",
        description: "Diff two schemas — tables, columns and indexes.",
        flags: [{ flag: "--migration", description: "Emit ordered migration SQL instead of the structured diff." }],
      },
      {
        name: "index",
        description: "Build and maintain the offline schema index — what makes search_schema and get_join_path fast.",
        subcommands: [
          {
            name: "build",
            description: "Build the index from scratch. Run once after install, and after a migration that moved a lot of objects.",
            flags: [
              {
                flag: "--depth",
                env: "NEXQL_MCP_INDEX_DEPTH",
                description: "How deep to go. structure indexes objects and columns, stats adds statistics, profiles adds the value data sample_values reads. Deeper takes longer.",
              },
            ],
          },
          { name: "status", description: "When the index was built, at what depth, and how stale it is." },
          { name: "refresh", description: "Bring the index up to date incrementally. Cheaper than a rebuild and enough for most changes." },
          {
            name: "clear",
            description: "Delete the index for the current profile and database.",
            flags: [{ flag: "--all", description: "Delete every index, for every profile." }],
          },
        ],
      },
      {
        name: "profile",
        description: "Create, test and move connection profiles without hand-editing TOML.",
        subcommands: [
          { name: "list", description: "Every configured profile, with its access mode. Never prints secrets." },
          {
            name: "add",
            usage: "add <name> [OPTIONS]",
            description: "Add a profile, testing the connection before saving unless told not to.",
            flags: [
              { flag: "--url", description: "Full connection URL, instead of the discrete fields." },
              { flag: "--host, --port, --dbname, --user", description: "Connection details, when assembling a URL is awkward." },
              { flag: "--password", description: "Password inline. Moved into the OS keyring the next time the config loads." },
              { flag: "--password-command", description: "Command that prints the password to stdout — point it at 1Password, Vault, or a script. Keeps the secret out of the file." },
              { flag: "--password-file", description: "Read the password from a file." },
              { flag: "--credential-provider", description: "aws-iam mints short-lived RDS IAM tokens instead of using a static password." },
              { flag: "--access-mode", description: "Access mode for this profile." },
              { flag: "--set-default", description: "Make this the profile used when none is named." },
              { flag: "--no-test", description: "Save without testing the connection first." },
            ],
          },
          { name: "set-password", usage: "set-password <name>", description: "Store a password for a profile in the OS keyring." },
          { name: "test", usage: "test [name]", description: "Connect using the profile and report what happened." },
          {
            name: "export",
            description: "Export profiles for sharing or committing. Secrets are omitted by construction.",
            flags: [
              { flag: "--format", description: "project writes a workspace .nexql/config.toml; full writes the whole profile set." },
              { flag: "--output", description: "Write to a file instead of stdout." },
            ],
          },
          { name: "import", usage: "import <path>", description: "Import profiles from an export." },
          { name: "migrate-secrets", description: "Move any remaining plaintext passwords out of config.toml and into the OS keyring." },
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
