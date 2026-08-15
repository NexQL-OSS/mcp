/**
 * Knowledge base.
 *
 * Short answers to questions that come up once the server is running. Facts are
 * taken from the Rust crates (see crates/nexql-mcp/src/main.rs and
 * crates/nexql-tools/src/registry.rs), not from earlier site copy.
 */

export interface KbArticle {
  id: string;
  category: string;
  tags: string[];
  title: string;
  summary: string;
  body: string;
}

export const kbArticles: KbArticle[] = [
  {
    id: "what-is-nexql-mcp",
    category: "Overview",
    tags: ["introduction", "mcp"],
    title: "What is NexQL MCP?",
    summary: "A standalone Rust binary that gives an MCP client schema-aware access to PostgreSQL.",
    body: "Any MCP client can spawn it over stdio. It ships an offline schema index, an inferred join graph, per-column value profiles, and 54 tools named for specific jobs rather than one generic query call. It defaults to read-only and runs entirely on your machine.",
  },
  {
    id: "vs-raw-mcp",
    category: "Overview",
    tags: ["comparison"],
    title: "How is this different from a generic Postgres MCP server?",
    summary: "The difference is everything that happens before the query runs.",
    body: "A generic server exposes connect and query, and the model works out the rest — usually from a schema dump pasted into context. NexQL MCP indexes the catalog first, so the agent searches for the right objects, confirms the join against real foreign keys, and checks a column's actual values before writing SQL. It adds access modes, plan analysis, DDL lock-safety checks and multi-environment profiles on top.",
  },
  {
    id: "schema-index",
    category: "Features",
    tags: ["index", "search"],
    title: "How does the schema index work?",
    summary: "TF-IDF search, a join graph, value profiles, and optional local embeddings.",
    body: "Build it with `nexql-mcp index build` or the rebuild_index tool. It stores a lexical index over tables, columns and comments; a join graph from pg_constraint with inferred edges as fallback; and value profiles for columns worth profiling. Agents use search_schema, get_join_path and sample_values against it. Enable --embeddings local to add MiniLM vectors, fused with the lexical hits via reciprocal rank fusion.",
  },
  {
    id: "index-depth",
    category: "Operations",
    tags: ["index", "build"],
    title: "What does index build --depth control?",
    summary: "structure, stats, or profiles — profiles is what makes sample_values useful.",
    body: "structure indexes objects and columns. stats adds table and column statistics. profiles additionally collects most-common values and cardinality, which is what sample_values reads. Deeper builds take longer and read more, so on a very large database start at structure and go deeper once you know the index is useful to you.",
  },
  {
    id: "access-modes",
    category: "Security",
    tags: ["read", "write", "admin"],
    title: "What do the access modes do?",
    summary: "read is the default; write and admin unlock mutation tools at call time.",
    body: "Every tool is listed to the client in all modes. Calling a write tool in read mode returns a refusal naming the mode it needs, rather than the tool being absent — an absent tool is ambiguous and makes agents retry or improvise. Underneath, connections are opened with default_transaction_read_only = ON, so the guard does not depend on the dispatcher alone. Write or admin against a superuser connection additionally requires --i-know-what-im-doing.",
  },
  {
    id: "sql-validation",
    category: "Security",
    tags: ["sql", "validation"],
    title: "How is read-only SQL actually enforced?",
    summary: "The statement is parsed with pg_query, not prefix-matched.",
    body: "Checking that a statement starts with SELECT is defeated by a leading comment, a CTE that writes, or a second statement after a semicolon. NexQL MCP parses the SQL with pg_query — PostgreSQL's own parser as a library — and inspects the tree. Anything that mutates is refused in read mode regardless of how it is written.",
  },
  {
    id: "pii-masking",
    category: "Security",
    tags: ["pii", "config"],
    title: "Masking sensitive columns",
    summary: "List them as pii_columns on the profile and results come back masked.",
    body: "Add fully-qualified schema.table.column entries to pii_columns in the profile. Pair it with read access mode, a schemas allowlist and deny_tables when you are pointing an agent at production — the four together mean the agent can reason about the shape of the data without reading the values you care about.",
  },
  {
    id: "dry-run-writes",
    category: "Security",
    tags: ["write", "ddl"],
    title: "Dry-running a write",
    summary: "dry_run: true runs the statement in a transaction and rolls it back.",
    body: "Supported on execute_sql and apply_ddl. You get the real error if it would fail, and no change if it would succeed. create_index_concurrently and run_maintenance cannot dry-run, because CONCURRENTLY and VACUUM run outside a transaction by design — use check_ddl_safety before those instead.",
  },
  {
    id: "ddl-safety",
    category: "Security",
    tags: ["ddl", "migration"],
    title: "Checking a migration before you run it",
    summary: "check_ddl_safety reports the lock level each statement will take.",
    body: "Most migration incidents are a statement needing a stronger lock than expected on a table that was busy. check_ddl_safety parses the DDL and reports what it will lock and for how long it plausibly holds it — enough to tell that adding a nullable column is safe at peak and adding one with a volatile default is not.",
  },
  {
    id: "profiles",
    category: "Configuration",
    tags: ["config", "profiles"],
    title: "Connection profiles",
    summary: "Named profiles in ~/.config/nexql-mcp/config.toml, one per environment.",
    body: "A profile carries connection details plus access_mode, schemas, deny_tables, pii_columns and max_rows. Override the config path with --config or NEXQL_MCP_CONFIG. A workspace can carry its own .nexql/config.toml, found from --workspace-root. Switch between profiles at runtime with switch_connection — no restart.",
  },
  {
    id: "secrets",
    category: "Configuration",
    tags: ["secrets", "security"],
    title: "Keeping passwords out of config files",
    summary: "password_command shells out to your secret manager; inline passwords move to the keyring.",
    body: "Set password_command to anything that prints the password to stdout — `op read op://vault/pg/password`, a vault CLI, a script. For RDS, credential_provider = \"aws-iam\" mints short-lived IAM tokens instead. Inline password values migrate to the OS keyring when the config loads, and `nexql-mcp profile migrate-secrets` moves legacy plaintext across in one pass. Profile exports never include secrets.",
  },
  {
    id: "tool-profiles",
    category: "Agents",
    tags: ["context", "tools"],
    title: "Trimming the tool surface",
    summary: "--tools query|dba|meta|full trades capability for context budget.",
    body: "All 54 tools in tools/list costs context on every turn. query exposes 21 schema and query tools and suits a coding agent. dba exposes 26 monitoring and maintenance tools. meta exposes 13 plus a discover_tools call so the agent can activate more on demand — the right choice on the tightest budgets. full is the default.",
  },
  {
    id: "agent-explore",
    category: "Agents",
    tags: ["workflow"],
    title: "Workflow: exploring an unfamiliar database",
    summary: "orient → search_schema → describe_object → get_join_path → sample_values → run_select.",
    body: "Orienting first tells the agent which profile, database and access mode it is in, which prevents a whole class of confident answers about the wrong environment. Searching before describing keeps context small. Confirming the join before writing SQL is what stops plausible-looking queries that join on the wrong column and return rows anyway.",
  },
  {
    id: "agent-perf",
    category: "Agents",
    tags: ["workflow", "dba"],
    title: "Workflow: diagnosing something slow",
    summary: "db_health_check → slow_queries → locks → explain_query → suggest_indexes.",
    body: "Start wide. A health check often makes the next step obvious. If something is hanging rather than slow, list_running_queries and find_blocking_locks explain more incidents than plans do — the query is usually fine and simply waiting. Reach for explain_query and deep_plan_analysis once you have a specific statement.",
  },
  {
    id: "join-path",
    category: "Features",
    tags: ["joins"],
    title: "Where join paths come from",
    summary: "Declared foreign keys first, inferred edges only as a fallback.",
    body: "get_join_path walks pg_constraint and tells you which edges were declared versus inferred, so you know how much to trust the path. If a lot of edges come back inferred, find_missing_fks will show you the columns that look like foreign keys but have no constraint behind them — usually worth fixing in the schema rather than working around.",
  },
  {
    id: "pg-stat-statements",
    category: "Performance",
    tags: ["monitoring", "extensions"],
    title: "slow_queries needs pg_stat_statements",
    summary: "CREATE EXTENSION IF NOT EXISTS pg_stat_statements — everything else works without it.",
    body: "slow_queries and the historical side of auto_tune_query read from pg_stat_statements. Without the extension you still get live diagnostics through list_running_queries, find_blocking_locks, table_stats and index_usage. run_doctor reports whether the extension is present.",
  },
  {
    id: "build-index",
    category: "Operations",
    tags: ["index", "doctor"],
    title: "Setup order: doctor, then index build",
    summary: "doctor proves the connection works before you debug anything else.",
    body: "Run `nexql-mcp <dsn> doctor` first — it checks connectivity, permissions, extensions and index state independently of any MCP client, which narrows problems down fast. Then `index build`. After a large migration, refresh_index brings it up to date incrementally. If objects are missing from search results, check that the profile's schemas allowlist includes them.",
  },
  {
    id: "http-transport",
    category: "Transport",
    tags: ["http", "security"],
    title: "When to use HTTP instead of stdio",
    summary: "Only when the client cannot spawn a process. Off loopback, a token is mandatory.",
    body: "Desktop clients should spawn the binary over stdio: no port, no auth to misconfigure. Use --http for hosted agents or shared sidecars. Port defaults to 8899 and bind to 127.0.0.1; binding anywhere else without NEXQL_MCP_HTTP_TOKEN is refused at startup rather than warned about. Rate limiting defaults to 600 requests per 60 seconds per token, or per IP when there is no token.",
  },
  {
    id: "docker-networking",
    category: "Install",
    tags: ["docker"],
    title: "Reaching a host database from Docker",
    summary: "host.docker.internal, plus --add-host on Linux. And do not forget -i.",
    body: "Inside the container, localhost is the container. Use host.docker.internal instead, and on Linux add --add-host=host.docker.internal:host-gateway. The container also needs -i so stdin stays open — the server speaks MCP over stdio, and without it the process exits immediately. Images are published as ghcr.io/nexql-oss/mcp:<version>.",
  },
  {
    id: "glibc-linux",
    category: "Install",
    tags: ["linux"],
    title: "Prebuilt Linux binaries need glibc 2.35+",
    summary: "On older distributions, build with cargo or use Docker.",
    body: "A GLIBC version error from the installed binary means the prebuilt is too new for your distribution. `cargo install nexql-mcp` builds against whatever you have — it needs clang and libclang available, because pg_query generates its bindings at build time. The Docker image is the other option. Musl builds are not published yet.",
  },
  {
    id: "tui-setup",
    category: "Setup",
    tags: ["tui"],
    title: "The interactive setup TUI",
    summary: "nexql-mcp tui — edit profiles, test them, and wire up clients in one place.",
    body: "Keys: n new, e edit, d delete, t test, w wire, q quit. Wiring shows a diff and writes a timestamped backup before touching any client config, so it merges into an existing setup rather than overwriting it. `nexql-mcp init` with no client argument opens the same wizard.",
  },
  {
    id: "vscode-extension",
    category: "Integration",
    tags: ["vscode"],
    title: "Using it with the NexQL VS Code extension",
    summary: "Set postgresExplorer.mcp.enabled and the extension spawns this binary for you.",
    body: "It reuses the connections already configured in postgresExplorer.connections, so there is no second setup, and appears as a tool provider in Copilot Chat. Binary resolution order is the setting, then NEXQL_MCP_BIN, then the bundled copy, then PATH. The extension is entirely optional — the server is standalone.",
  },
  {
    id: "license-gpl",
    category: "Legal",
    tags: ["license"],
    title: "Licensing: GPL-3.0-only from v0.2.0",
    summary: "v0.1.6 and earlier were Apache-2.0, and that grant stands.",
    body: "Redistributing nexql-mcp, modified or not, carries the GPL source-release obligation. Using it as a tool — including from a proprietary editor extension that spawns it as a subprocess over stdio — does not, because separate programs communicating over a pipe are not one combined work. That subprocess boundary is deliberate on the NexQL extension's side.",
  },
];

export const kbCategories = [...new Set(kbArticles.map((a) => a.category))];
