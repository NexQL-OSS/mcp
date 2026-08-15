/**
 * Feature pages.
 *
 * Facts here are drawn from the Rust crates, not from marketing copy —
 * crates/nexql-tools/src/registry.rs for tool names and profiles, and
 * crates/nexql-mcp/src/main.rs for flags and defaults. If the binary changes,
 * this file changes with it.
 */

export interface FeatureSection {
  title: string;
  body: string;
  points?: string[];
}

export interface FeatureDoc {
  slug: string;
  title: string;
  /** Mono kicker above the headline. */
  subtitle: string;
  /** Serif-italic clause that closes the headline. */
  titleEm: string;
  /** One-paragraph summary — used on the index grid and as the page lede. */
  description: string;
  bullets: string[];
  sections?: FeatureSection[];
  tools?: string[];
  related?: string[];
}

export const features: FeatureDoc[] = [
  {
    slug: "schema-index",
    title: "The index is",
    titleEm: "the whole point.",
    subtitle: "Schema index",
    description:
      "NexQL MCP reads your catalog once and keeps the result on disk: a lexical search index, a join graph built from real foreign keys, and a value profile for every column worth profiling. Agents ask it questions instead of asking the database — or worse, guessing.",
    bullets: [
      "TF-IDF lexical search over tables, columns and comments",
      "Join graph from pg_constraint, with inferred edges as fallback",
      "Per-column value profiles: most-common values, cardinality, null rate",
      "Optional local MiniLM embeddings, fused with lexical hits via RRF",
      "Stored under ~/.local/share/nexql-mcp — nothing leaves the machine",
    ],
    sections: [
      {
        title: "Why not just send the schema?",
        body: "Because a real database does not fit in a context window, and the parts that do fit are rarely the parts that matter. A hundred-table schema dumped into a prompt costs thousands of tokens every session and still leaves the model guessing which of four similarly-named tables holds the number you asked for. An index answers the narrow question — which objects match this intent — and returns four ranked hits instead of everything.",
        points: [
          "search_schema returns ranked objects with the columns that matched",
          "Cost is bounded by the answer, not by the size of the schema",
          "Rebuilt incrementally, so it survives migrations",
        ],
      },
      {
        title: "Joins come from constraints, not from column names",
        body: "Guessing that orders.customer_id joins customers.id is easy. Guessing the path from refunds to customers across three tables is where models invent joins that run, return rows, and are quietly wrong. get_join_path walks the declared foreign keys first and only falls back to inferred edges when the catalog has nothing — and it tells you which kind it used.",
      },
      {
        title: "Values without a query",
        body: "Asking what a status column can contain normally means a SELECT DISTINCT against a production table. The index profiles those columns during the build, so sample_values answers from disk. No scan, no lock, no surprise on a billion-row table.",
      },
    ],
    tools: ["search_schema", "get_join_path", "sample_values", "find_missing_fks", "rebuild_index"],
    related: ["query-tools", "safety"],
  },
  {
    slug: "query-tools",
    title: "Bounded reads,",
    titleEm: "not a SQL socket.",
    subtitle: "Query tools",
    description:
      "run_select is not a passthrough. SQL is parsed with pg_query before it runs, rejected if it mutates anything in read mode, capped at a row limit, and returned with pagination metadata so the agent knows there is more.",
    bullets: [
      "Parsed as an AST — never prefix-matched against 'SELECT'",
      "Parameterized execution, so values never get concatenated into SQL",
      "Default row caps, overridable per call and per profile",
      "explain_query and deep_plan_analysis for plan shape",
      "auto_tune_query proposes indexes from a real plan",
      "export_query writes CSV or JSON for larger extracts",
    ],
    sections: [
      {
        title: "String checks are not safety",
        body: "A read-only guard that checks whether a statement starts with SELECT is defeated by a CTE, a comment, or a semicolon. NexQL MCP hands the statement to pg_query — the actual PostgreSQL parser, as a library — and inspects the resulting tree. A statement that mutates is refused whatever it looks like.",
      },
      {
        title: "Plans, then indexes",
        body: "auto_tune_query does not pattern-match SQL. It runs the plan, finds the scans that dominate cost, and proposes indexes against those. deep_plan_analysis goes further and reports the specific pathologies — repeated scans of the same relation, estimates that diverge sharply from actuals — that explain why a query is slow rather than just that it is.",
      },
    ],
    tools: ["run_select", "explain_query", "deep_plan_analysis", "auto_tune_query", "export_query"],
    related: ["schema-index", "performance"],
  },
  {
    slug: "performance",
    title: "The questions you",
    titleEm: "get paged about.",
    subtitle: "Performance & DBA",
    description:
      "Lock chains, slow queries, index usage, bloat, and missing foreign keys — live from the running server and from pg_stat_statements. The diagnostics a DBA reaches for, available to an agent by name.",
    bullets: [
      "slow_queries from pg_stat_statements, ranked by total time",
      "find_blocking_locks resolves the chain, not just the blocked PID",
      "suggest_indexes from sequential scans and unindexed foreign keys",
      "find_unused_indexes excludes constraint-backed indexes",
      "bloat_report estimates dead-tuple ratio per table",
      "db_health_check for a single-call snapshot",
    ],
    sections: [
      {
        title: "Named for the symptom",
        body: "An agent choosing between fifty tools does it by name. find_blocking_locks is a better name than query when the question is why is this insert hanging, because the model can tell from the name alone that this is the call. That is the argument for a wide, specifically-named tool surface over one generic escape hatch — and the argument for tool profiles when that surface costs too much context.",
      },
      {
        title: "Safe by construction",
        body: "terminate_query guards against terminating your own session and against superuser backends. create_index_concurrently runs outside a transaction because CONCURRENTLY requires it. run_maintenance covers VACUUM, ANALYZE and REINDEX. All three sit behind admin mode.",
      },
    ],
    tools: ["slow_queries", "suggest_indexes", "db_health_check", "find_blocking_locks", "bloat_report"],
    related: ["query-tools", "write-admin"],
  },
  {
    slug: "safety",
    title: "Refused at call time,",
    titleEm: "not hidden from the list.",
    subtitle: "Safety model",
    description:
      "Every tool appears in tools/list. The access mode on the active profile decides which ones actually execute — so an agent that tries to write in read mode gets a clear refusal it can reason about, instead of a tool that mysteriously does not exist.",
    bullets: [
      "Three modes: read (default), write, admin",
      "default_transaction_read_only = ON on every pool connection",
      "Write and admin against a superuser need --i-know-what-im-doing",
      "Per-profile schemas, deny_tables, pii_columns and max_rows",
      "dry_run on execute_sql and apply_ddl rolls the transaction back",
      "PII columns masked in results",
    ],
    sections: [
      {
        title: "Why not hide the tools?",
        body: "Because a missing tool is an ambiguous signal. The model cannot distinguish between this server cannot write and this server will not write for you right now, so it retries, rephrases, or invents a workaround. An explicit refusal that names the required access mode is something it can act on — usually by telling you what to change.",
      },
      {
        title: "Two layers, not one",
        body: "The access mode is enforced in the tool dispatcher, and the connection itself is opened read-only at the transaction level. A bug in the first layer does not become a write, because the second layer is PostgreSQL refusing on its own terms.",
        points: [
          "Layer one: access-mode check before dispatch",
          "Layer two: default_transaction_read_only on the session",
          "Layer three: pg_query AST inspection of the statement",
        ],
      },
    ],
    related: ["connections", "write-admin"],
  },
  {
    slug: "connections",
    title: "Many databases,",
    titleEm: "no secrets in the repo.",
    subtitle: "Connections & profiles",
    description:
      "Named profiles in ~/.config/nexql-mcp/config.toml, with password_command for shelling out to your secret manager, AWS IAM auth, schema allowlists, and per-workspace .nexql/config.toml discovery.",
    bullets: [
      "switch_connection at runtime — no server restart",
      "password_command keeps credentials out of config files",
      "Profile export and import without plaintext secrets",
      "Workspace-local .nexql/config.toml, discovered from --workspace-root",
      "nexql-mcp tui for guided multi-client wiring",
      "setup_connection over MCP elicitation, for clients with no terminal",
    ],
    sections: [
      {
        title: "Production is a different profile",
        body: "Profiles carry more than a connection string. Each one has its own access mode, row cap, schema allowlist, denied tables and PII column list — so a prod profile can be read-only with masked columns while a local one is wide open, and switching between them is a single tool call rather than an edit-and-restart.",
      },
    ],
    tools: ["list_connections", "switch_connection", "setup_connection", "save_profile"],
    related: ["safety", "vscode-extension"],
  },
  {
    slug: "write-admin",
    title: "Mutations are",
    titleEm: "opt-in and inspected.",
    subtitle: "Write & admin",
    description:
      "DML needs write mode; DDL needs admin. Both parse the statement first, both support dry runs that roll back, and check_ddl_safety will tell you what a migration is about to lock before you apply it.",
    bullets: [
      "execute_sql: DML in write mode, DML and DDL in admin",
      "edit_row for parameterized insert, update and delete by primary key",
      "import_data for batched inserts from a JSON rows array",
      "check_ddl_safety analyses the DDL AST for lock risk",
      "apply_ddl runs in a transaction, with optional dry_run",
      "Managed extension mode drops profile-mutation tools entirely",
    ],
    sections: [
      {
        title: "Know the lock before you take it",
        body: "Most migration incidents are a statement that needed an ACCESS EXCLUSIVE lock on a table that was busy. check_ddl_safety parses the DDL and reports the lock level and the risk, so the agent — and you — can see that adding a column with a volatile default is not the same as adding a nullable one.",
      },
    ],
    tools: ["execute_sql", "apply_ddl", "create_index_concurrently", "run_maintenance"],
    related: ["safety", "performance"],
  },
  {
    slug: "http-transport",
    title: "Stdio by default,",
    titleEm: "HTTP when you need it.",
    subtitle: "Transport",
    description:
      "Desktop clients spawn the binary and talk over stdio — no ports, no daemon. When something remote needs to reach it, --http serves streamable HTTP on 8899, bound to loopback, with a bearer token required for anything else.",
    bullets: [
      "stdio is the default and needs no configuration",
      "--http serves streamable HTTP on port 8899",
      "--bind defaults to 127.0.0.1; non-loopback binds require a token",
      "NEXQL_MCP_HTTP_TOKEN or --http-token for bearer auth",
      "600 requests per 60s per token, or per IP when there is no token",
      "--http-rate-limit 0 disables the limit",
    ],
    sections: [
      {
        title: "The token is not optional off loopback",
        body: "Binding to anything other than 127.0.0.1 without a bearer token is refused at startup rather than warned about. An MCP server with database access on an open port is a credential, and the failure mode of getting that wrong is not one worth leaving to a config comment.",
      },
    ],
    related: ["connections", "safety"],
  },
  {
    slug: "vscode-extension",
    title: "The extension",
    titleEm: "spawns this binary.",
    subtitle: "NexQL for VS Code",
    description:
      "Enable postgresExplorer.mcp.enabled and the NexQL extension launches nexql-mcp as a subprocess, reusing the connections you already configured. It appears as a tool provider in Copilot Chat.",
    bullets: [
      "Reuses postgresExplorer.connections — no second setup",
      "Binary resolution: setting → NEXQL_MCP_BIN → bundled → PATH",
      "Runs as a spawned subprocess, not in-process",
      "Optional: the server is fully standalone without it",
    ],
    sections: [
      {
        title: "Why a subprocess",
        body: "nexql-mcp is GPL-3.0-only; the NexQL extension is not. Invoking the binary as a separate program over stdio keeps them separate works rather than one combined program, which is what makes the licence combination sound. It is also simply better engineering — the server can crash, be upgraded, or be swapped without taking the editor with it.",
      },
    ],
    related: ["connections", "schema-index"],
  },
];

export function getFeature(slug: string): FeatureDoc | undefined {
  return features.find((f) => f.slug === slug);
}
