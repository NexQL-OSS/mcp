export const SITE_VERSION = "0.3.0";
export const SITE_URL = "https://nexql-mcp.astrx.dev";

/** Sibling NexQL properties, cross-linked from the header, footer and hero. */
export const NEXQL_URL = "https://nexql.astrx.dev/";
export const THEMES_URL = "https://nexql-themes.astrx.dev/";
export const REPO_URL = "https://github.com/NexQL-OSS/mcp";
export const RELEASES_URL = "https://github.com/NexQL-OSS/mcp/releases/latest";
export const REGISTRY_URL =
  "https://registry.modelcontextprotocol.io/servers/io.github.NexQL-OSS/nexql-mcp";

/**
 * Facts repeated across pages. Sourced from the Rust crate, not from marketing
 * copy — see crates/nexql-tools/src/registry.rs (ToolName::ACTIVE) and
 * crates/nexql-mcp/src/main.rs (clap definitions).
 */
export const FACTS = {
  /** ToolName::ACTIVE — registry.rs. Keep in sync with data/tools.ts. */
  toolCount: 54,
  pgVersions: "12–17",
  httpPort: 8899,
  httpBind: "127.0.0.1",
  rateLimit: 600,
  license: "GPL-3.0-only",
} as const;
