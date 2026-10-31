/**
 * Content for the guided tour on the home page. Commands and flags come from
 * the crate (crates/nexql-mcp/src/main.rs) and docs/clients/README.md; keep
 * them in sync when those change.
 */

export type AppKind = "bundle" | "cli" | "other";

export interface TourApp {
  id: string;
  name: string;
  /** `nexql-mcp init <client>` id, for apps it can wire. */
  client?: string;
  kind: AppKind;
  hint: string;
}

export const APPS: TourApp[] = [
  { id: "claude-desktop", name: "Claude Desktop", kind: "bundle", hint: "One download, no terminal" },
  { id: "claude-code", name: "Claude Code", client: "claude-code", kind: "cli", hint: "Terminal" },
  { id: "cursor", name: "Cursor", client: "cursor", kind: "cli", hint: "Editor" },
  { id: "vscode", name: "VS Code", client: "vscode", kind: "cli", hint: "Copilot agent mode" },
  { id: "codex", name: "Codex", client: "codex", kind: "cli", hint: "CLI, IDE and ChatGPT desktop app" },
  { id: "gemini-cli", name: "Gemini CLI", client: "gemini-cli", kind: "cli", hint: "Terminal" },
  { id: "windsurf", name: "Windsurf", client: "windsurf", kind: "cli", hint: "Editor" },
  { id: "antigravity", name: "Antigravity", client: "antigravity", kind: "cli", hint: "Editor" },
  { id: "zed", name: "Zed", client: "zed", kind: "cli", hint: "Editor" },
  { id: "copilot-cli", name: "Copilot CLI", client: "copilot-cli", kind: "cli", hint: "GitHub's terminal agent" },
  { id: "cline", name: "Cline", client: "cline", kind: "cli", hint: "Cline CLI" },
  { id: "kiro", name: "Kiro", client: "kiro", kind: "cli", hint: "Editor and CLI" },
  { id: "lm-studio", name: "LM Studio", client: "lm-studio", kind: "cli", hint: "Local models" },
  { id: "qwen", name: "Qwen Code", client: "qwen", kind: "cli", hint: "Terminal" },
  { id: "kimi", name: "Kimi Code", client: "kimi", kind: "cli", hint: "Terminal" },
  { id: "other", name: "Something else", kind: "other", hint: "Any app that runs local MCP servers" },
];

export const DEFAULT_APP = "claude-desktop";

export interface Stop {
  id: string;
  label: string;
  minutes: string;
}

/** The route, in order. `id` is the section anchor. */
export const STOPS: Stop[] = [
  { id: "app", label: "Pick your app", minutes: "10 sec" },
  { id: "install", label: "Install", minutes: "1 min" },
  { id: "connect", label: "Connect a database", minutes: "2 min" },
  { id: "ask", label: "Ask your first question", minutes: "1 min" },
  { id: "more", label: "Add more databases", minutes: "1 min" },
  { id: "safe", label: "Stay in control", minutes: "1 min" },
];

export interface Platform {
  id: string;
  name: string;
  icon?: string;
  host: string;
  port: string;
  ssl: "" | "require";
  where: string;
  network: string;
}

/** Where people's databases live, and where each one hides its connection details. */
export const PLATFORMS: Platform[] = [
  {
    id: "local",
    name: "On this computer",
    icon: "postgresql",
    host: "localhost",
    port: "5432",
    ssl: "",
    where: "Use localhost and the port Postgres listens on (5432 unless you changed it). For Docker, use the port you published, e.g. -p 5433:5432 means 5433.",
    network: "Nothing to open up. Postgres only needs to accept TCP connections, which it does by default.",
  },
  {
    id: "supabase",
    name: "Supabase",
    icon: "supabase",
    host: "aws-0-REGION.pooler.supabase.com",
    port: "5432",
    ssl: "require",
    where: "In your project, open Connect at the top of the dashboard and copy the session pooler connection string.",
    network: "Reachable from anywhere by default. Use the session pooler if your network has no IPv6.",
  },
  {
    id: "neon",
    name: "Neon",
    icon: "neon",
    host: "ep-NAME.REGION.aws.neon.tech",
    port: "5432",
    ssl: "require",
    where: "In the Neon console, click Connect on your project dashboard and copy the connection string.",
    network: "Reachable from anywhere by default. The string already includes sslmode=require.",
  },
  {
    id: "rds",
    name: "AWS RDS / Aurora",
    icon: "aws",
    host: "NAME.ID.REGION.rds.amazonaws.com",
    port: "5432",
    ssl: "require",
    where: "In the RDS console, open your database and copy the endpoint and port from Connectivity & security.",
    network: "Your computer must be allowed in: the instance needs public access, and its security group an inbound rule for your IP on port 5432. Or connect over your VPN or a bastion.",
  },
  {
    id: "gcp",
    name: "Google Cloud SQL",
    host: "127.0.0.1",
    port: "5432",
    ssl: "",
    where: "The simplest route is the Cloud SQL Auth Proxy: run it on your computer and connect to 127.0.0.1. Otherwise use the instance's public IP from its Overview page.",
    network: "With the proxy, nothing to open up. With a public IP, add your IP under Connections → Authorized networks and use sslmode=require.",
  },
  {
    id: "azure",
    name: "Azure Database",
    icon: "azure",
    host: "NAME.postgres.database.azure.com",
    port: "5432",
    ssl: "require",
    where: "In the Azure portal, open your flexible server and copy the server name from Overview.",
    network: "Add a firewall rule for your IP under Networking. Azure requires SSL.",
  },
  {
    id: "timescale",
    name: "Timescale",
    icon: "timescale",
    host: "ID.PROJECT.tsdb.cloud.timescale.com",
    port: "PORT",
    ssl: "require",
    where: "Open your service in the Timescale console and copy the connection details from its overview.",
    network: "Reachable from anywhere unless you set up IP allow lists.",
  },
  {
    id: "elsewhere",
    name: "Somewhere else",
    host: "db.example.com",
    port: "5432",
    ssl: "require",
    where: "Ask whoever runs the database for the host, port, database name and a user. A user that can only read is best.",
    network: "Your computer needs to reach that host and port. A VPN or SSH tunnel counts.",
  },
];

/** Starter prompts for stop 4. Each uses tools in the default `query` set. */
export const PROMPTS = [
  "What's in this database?",
  "Which tables hold customer orders, and how are they linked?",
  "How much revenue did we make each month this year?",
  "Show me five example rows from the orders table.",
  "Why is this query slow? SELECT … (paste it)",
];
