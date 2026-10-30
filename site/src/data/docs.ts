/**
 * The human layer of the docs. `tools.json` is what the server advertises
 * (regenerate it with scripts/sync-tools.mjs); this file groups those tools
 * into lines and explains each one in plain words. The build fails if the two
 * disagree, so a new tool can't ship undocumented.
 */
import catalog from "./tools.json";

export type Mode = "read" | "write" | "admin";

export interface Param {
  name: string;
  type: string;
  required: boolean;
  description: string;
  enum?: string[];
}

export interface ServerTool {
  name: string;
  description: string;
  annotations: { readOnlyHint?: boolean; destructiveHint?: boolean; idempotentHint?: boolean };
  sets: string[];
  params: Param[];
}

interface ToolNotes {
  /** One plain sentence: what it does for you. */
  summary: string;
  /** Something a person might type that leads the assistant here. */
  ask: string;
  /** How it works, limits, and what to expect. Inline `code` is allowed. */
  how: string[];
  /** Writes nexql-mcp's own config or index, not the database. */
  local?: boolean;
}

export interface Line {
  id: string;
  name: string;
  color: string;
  blurb: string;
  tools: Record<string, ToolNotes>;
}

export const LINES: Line[] = [
  {
    id: "explore",
    name: "Explore the schema",
    color: "var(--hue-blue)",
    blurb: "Find the right tables before any SQL is written, so the assistant works from what exists, not from guesses.",
    tools: {
      orient: {
        summary: "Gives a one-call overview of the whole database.",
        ask: "What's in this database?",
        how: [
          "Reads the schema index, not the live catalog, so it is fast and costs no queries.",
          "Returns tables and views with their columns, primary key and row estimate, the foreign-key joins between them, and enum values.",
          "Past 40 tables it lists the rest by name only, to keep the answer small. Pass `focus` (e.g. `orders`) to zoom in.",
        ],
      },
      search_schema: {
        summary: "Finds tables, views and functions that match your words.",
        ask: "Where do we keep customer emails?",
        how: [
          "Ranks objects in the schema index by how well their names, columns and comments match the query (TF-IDF).",
          "With local embeddings switched on (`--embeddings local`), it also ranks by meaning and fuses both lists, so \"money we're owed\" can find `invoices`.",
          "The server's instructions tell the assistant to start here and to treat a table that isn't returned as one that doesn't exist.",
        ],
      },
      inspect_or_search: {
        summary: "Searches and describes the matches in one step.",
        ask: "Show me the tables about card payments and their columns.",
        how: [
          "Runs the same search as `search_schema`, then attaches columns, keys and row estimates for the top matches (3 by default).",
          "Saves a round trip when the assistant would otherwise call `search_schema` and then `describe_object` on each hit.",
        ],
      },
      describe_object: {
        summary: "Shows one table or view in full: columns, types, keys and indexes.",
        ask: "What columns does the orders table have?",
        how: [
          "Takes `schema.name`; a bare name works when only one schema has it.",
          "With `resolve_refs`, it adds enum values and a sample of the labels behind foreign-key IDs, so the assistant knows that `status_id = 3` means \"shipped\".",
        ],
      },
      get_join_path: {
        summary: "Works out how two tables connect.",
        ask: "How do order items link back to customers?",
        how: [
          "Walks the join graph in the schema index and returns the shortest chain of foreign keys between the two tables.",
          "The graph includes inferred joins (an `*_id` column that matches a primary key) where no foreign key was declared.",
          "Also returns a small Mermaid diagram of the path that clients can draw.",
        ],
      },
      sample_values: {
        summary: "Shows a handful of real values from one column.",
        ask: "What values does the status column actually hold?",
        how: [
          "Runs a small read-only SELECT on the column, so the assistant can match your wording to real values (`'shipped'`, not `'Shipped'`).",
          "Masking applies: sampling an email column returns `<redacted>`.",
        ],
      },
      get_ddl: {
        summary: "Returns the definition of a table, view, function or index.",
        ask: "Show me the SQL that defines the monthly_revenue view.",
        how: [
          "Views, materialized views, functions and indexes come back as their `CREATE` statement.",
          "Tables come back as structured DDL: columns, constraints and indexes.",
        ],
      },
      list_schemas: {
        summary: "Lists the schemas in the current database.",
        ask: "Which schemas are there?",
        how: ["System schemas (`pg_catalog`, `information_schema`, …) are left out, and so are any the profile's `schemas` / `deny_schemas` settings hide."],
      },
      list_objects: {
        summary: "Lists the tables and views in a schema.",
        ask: "List every table in the billing schema.",
        how: [
          "Filter by `kind` (table, view or materialized view).",
          "Partitions are grouped under their parent unless you ask for `include_partitions`.",
        ],
      },
      find_missing_fks: {
        summary: "Spots columns that look like foreign keys but aren't declared as one.",
        ask: "Are there relationships in this schema that aren't enforced?",
        how: [
          "Uses the joins the schema index inferred; without an index, it falls back to `*_id` columns that match a single-column primary key.",
          "It only suggests. Adding the constraint is up to you.",
        ],
      },
    },
  },
  {
    id: "query",
    name: "Ask with SQL",
    color: "var(--hue-indigo)",
    blurb: "Run read-only queries, get results in the shape you need, and understand why a query is slow.",
    tools: {
      run_select: {
        summary: "Runs a read-only query and returns the rows.",
        ask: "How much revenue did we make each month this year?",
        how: [
          "The SQL is parsed with Postgres's own parser (`pg_query`). Anything other than a single SELECT or WITH is refused before it reaches the database, and the connection itself is read-only.",
          "Returns 50 rows by default, up to the profile's `max_rows` (500 unless set). `has_more` says when there are more.",
          "Runs under a statement timeout (30 seconds unless the profile sets `statement_timeout_ms`). `timeout_ms` can lower it, never raise it.",
          "Columns that look like personal data or secrets come back as `<redacted>`, and tables in `deny_tables` are refused.",
          "Pass values through `params` (`$1`, `$2`, …) rather than pasting them into the SQL.",
        ],
      },
      explain_query: {
        summary: "Shows how Postgres plans to run a query, without running it.",
        ask: "Will this query use the index on created_at?",
        how: ["Runs plain `EXPLAIN`, so nothing executes. For timings, use `deep_plan_analysis`."],
      },
      export_query: {
        summary: "Runs a query and formats the result as CSV, JSON or INSERT statements.",
        ask: "Export last week's refunds as CSV.",
        how: [
          "Same read-only checks, row cap and masking as `run_select`.",
          "`sqlinsert` produces `INSERT` statements for a target table you name, handy for copying rows into a test database.",
        ],
      },
      deep_plan_analysis: {
        summary: "Runs a query with timing and explains where the time goes.",
        ask: "Why is this report query slow?",
        how: [
          "Runs `EXPLAIN ANALYZE` by default, so the query really executes. Pass `analyze: false` for estimates only.",
          "Parses the plan and grades findings by severity: bad row estimates, expensive scans, costly functions, CTEs and subqueries, with suggestions.",
        ],
      },
      auto_tune_query: {
        summary: "Suggests step-by-step fixes for a slow query.",
        ask: "Make this query faster.",
        how: [
          "Runs `EXPLAIN ANALYZE`, reads the statistics of the tables involved and looks for missing indexes.",
          "Returns recommendations. It never creates an index or rewrites anything itself.",
        ],
      },
    },
  },
  {
    id: "connect",
    name: "Connections",
    color: "var(--hue-purple)",
    blurb: "Add databases, move between them, and find which one holds the data, with passwords kept out of the chat.",
    tools: {
      add_database: {
        summary: "Connects another database without the password passing through the chat.",
        ask: "Also connect our billing database on db.internal, user billing_reader.",
        how: [
          "The assistant passes the host, port, database and user, but never a password. The tool refuses a password, and refuses a URL that contains one.",
          "nexql-mcp opens a one-time page on `127.0.0.1` with a random port and secret link. You check the details and type the password there.",
          "When you submit, it tests the connection, stores the password in your OS keyring, saves a read-only profile and makes it available straight away, with no restart.",
          "The page accepts 5 attempts and expires after 10 minutes. The assistant calls again with `setupId` to see whether you finished.",
          "If nexql-mcp runs on another machine, it gives you a `nexql-mcp profile add …` command to run in a terminal instead.",
        ],
        local: true,
      },
      list_connections: {
        summary: "Lists the saved connections.",
        ask: "Which databases can you reach?",
        how: ["Shows each profile's name, host, port and database. Passwords are never included."],
      },
      get_current_context: {
        summary: "Says which connection and database the session is using.",
        ask: "Which database are you looking at right now?",
        how: ["Returns the active profile, database and access mode."],
      },
      switch_connection: {
        summary: "Moves the session to another connection or database.",
        ask: "Switch to the analytics database.",
        how: [
          "Pass a profile name, a database, or both. Passing only `database` stays on the same server.",
          "Most query tools also accept `connectionId` / `database` for one call, without switching.",
        ],
      },
      list_databases: {
        summary: "Lists the databases on a server.",
        ask: "What other databases are on this server?",
        how: ["Uses the active connection unless you name another profile."],
      },
      search_all_databases: {
        summary: "Finds which database holds a table, across every connection.",
        ask: "Which of our databases has the subscriptions table?",
        how: [
          "Searches every saved profile and every database it can reach (20 pairs by default).",
          "Databases without a schema index are indexed on first search, so the first call can take longer.",
          "Each hit carries the `database` to pass to `describe_object` or `run_select`.",
        ],
      },
      resolve_target: {
        summary: "Picks the right connection from a hint like \"staging\" or a table name.",
        ask: "Check the orders table in staging.",
        how: [
          "Matches the hint against profile names, hosts, databases and indexed tables across all connections.",
          "If one match is clearly best it switches to it; if several fit equally it returns them and the assistant should ask you.",
        ],
      },
      setup_connection: {
        summary: "Detects or creates a connection from what's already on your machine.",
        ask: "Connect to the database this project uses.",
        how: [
          "Looks at environment variables such as `DATABASE_URL`, workspace files and saved settings.",
          "A password given here goes to the OS keyring, never to a file in plain text. Prefer `add_database`, which keeps the password out of the chat entirely.",
        ],
        local: true,
      },
      save_profile: {
        summary: "Saves or updates a connection profile.",
        ask: "Lower the row limit on the prod profile to 200.",
        how: [
          "Merges into an existing profile by default: fields you leave out are kept. `replace: true` overwrites it.",
          "Raising `access_mode` to write or admin needs `confirm_elevated_access: true`, so it can't happen by accident.",
          "Writes the config atomically, keeps a backup, and reloads the session.",
        ],
        local: true,
      },
      test_profile: {
        summary: "Checks that a connection works.",
        ask: "Can you still reach the reporting database?",
        how: ["Connects and returns the server version, whether the role is a superuser, and the round-trip time. Saves nothing."],
      },
      export_profile: {
        summary: "Produces a shareable config with every secret stripped.",
        ask: "Give me a config my team can use.",
        how: ["Outputs TOML for `.nexql/config.toml` in a project. Passwords and other credentials are removed."],
      },
      import_profile: {
        summary: "Imports a team config into your own.",
        ask: "Import the connections from this project's .nexql/config.toml.",
        how: ["Takes a path or TOML text. Each person still supplies their own password."],
        local: true,
      },
    },
  },
  {
    id: "health",
    name: "Health and performance",
    color: "var(--hue-magenta)",
    blurb: "See how the database is doing, what's slow, what's stuck, and which indexes help or hurt.",
    tools: {
      db_dashboard: {
        summary: "Gives a live snapshot of the database in one call.",
        ask: "How's the database doing right now?",
        how: [
          "Size and owner, connections by state, the largest tables, object counts, active queries and blocking locks.",
          "If one section fails (say, a missing permission), the others are still returned.",
        ],
      },
      db_health_check: {
        summary: "Checks for common problems and reports them.",
        ask: "Run a health check.",
        how: ["Cache hit ratio, tables with many dead rows that need vacuuming, connection use and blocking locks. Sections fail independently."],
      },
      table_stats: {
        summary: "Shows size and activity for one table.",
        ask: "How big is the events table and when was it last vacuumed?",
        how: ["Size, row count, scans, inserts/updates/deletes, dead rows, last vacuum and analyze, plus per-column statistics."],
      },
      index_usage: {
        summary: "Shows how often each index on a table is used.",
        ask: "Are the indexes on orders being used?",
        how: ["Scan counts, size, type and definition for every index on the table, from `pg_stat_user_indexes`."],
      },
      list_running_queries: {
        summary: "Lists the queries running right now.",
        ask: "What's running on the database?",
        how: ["Non-idle sessions with pid, user, state, wait event and how long each has been running."],
      },
      find_blocking_locks: {
        summary: "Finds which queries are stuck and who is blocking them.",
        ask: "Why is the checkout update hanging?",
        how: ["Pairs each waiting query with the pid and query holding the lock it needs. To stop one, see `terminate_query`."],
      },
      slow_queries: {
        summary: "Lists the slowest queries the database has seen.",
        ask: "What are our slowest queries?",
        how: [
          "Reads `pg_stat_statements`, ordered by mean execution time.",
          "That extension has to be installed; if it isn't, you get a hint on how to enable it instead of an error.",
        ],
      },
      suggest_indexes: {
        summary: "Suggests indexes that would likely help.",
        ask: "Which indexes are we missing?",
        how: [
          "Looks at tables read mostly by sequential scans, foreign-key columns with no index, and slow statements.",
          "Pass `sql` to base the suggestions on one query's plan. Nothing is created.",
        ],
      },
      find_unused_indexes: {
        summary: "Lists indexes that have never been used.",
        ask: "Which indexes could we drop?",
        how: [
          "Indexes with zero scans since statistics were last reset. Primary keys, unique and constraint-backed indexes are left out.",
          "Check how long statistics have been collecting before dropping anything.",
        ],
      },
      bloat_report: {
        summary: "Estimates which tables carry the most dead rows.",
        ask: "Which tables need a vacuum?",
        how: ["A quick estimate from the dead-row ratio in `pg_stat_user_tables` (tables with over 1,000 dead rows), not a measurement of on-disk bloat."],
      },
      server_settings: {
        summary: "Shows the server settings that matter most.",
        ask: "How much memory is Postgres allowed to use?",
        how: ["Memory, connections, timeouts, autovacuum and version, from `pg_settings`."],
      },
      list_extensions: {
        summary: "Lists installed extensions.",
        ask: "Is PostGIS installed?",
        how: ["Name, version and schema of each installed extension."],
      },
      list_roles: {
        summary: "Lists database roles, or one role's memberships and privileges.",
        ask: "What can the reporting role access?",
        how: ["Without `role`, lists every role and its attributes. With it, shows that role's memberships and table privileges."],
      },
    },
  },
  {
    id: "change",
    name: "Changes and migrations",
    color: "var(--hue-orange)",
    blurb: "Plan schema changes safely, and, only when you allow it, change data and run maintenance.",
    tools: {
      schema_diff: {
        summary: "Compares two schemas in the same database.",
        ask: "What's different between public and staging?",
        how: ["Returns table, column, constraint and index differences. Changes nothing."],
      },
      generate_migration: {
        summary: "Writes the SQL to turn one schema into another.",
        ask: "Write a migration that brings public in line with staging.",
        how: ["Builds on `schema_diff` and returns SQL text. It never runs it, and destructive drops come out commented out so you decide on them."],
      },
      check_ddl_safety: {
        summary: "Warns about schema changes that would lock or rewrite a table.",
        ask: "Is it safe to run this ALTER TABLE on production?",
        how: [
          "Reads the DDL without running it and scores the risk: non-concurrent index builds, column drops, table rewrites and other exclusive locks.",
          "Suggests the zero-downtime alternative where there is one. `CREATE DATABASE` / `DROP DATABASE` always rate CRITICAL.",
        ],
      },
      execute_sql: {
        summary: "Runs an INSERT, UPDATE or DELETE.",
        ask: "Mark order 1042 as refunded.",
        how: [
          "Needs write mode. DDL through this tool needs admin mode.",
          "Runs inside a transaction. Any error rolls everything back. `dry_run: true` runs it and rolls back, with before-and-after snapshots of the affected rows.",
          "`timeout_ms` may raise the time limit for a big batch, up to 10 minutes (`NEXQL_MCP_MAX_TIMEOUT_MS`).",
        ],
      },
      edit_row: {
        summary: "Inserts, updates or deletes one row by its primary key.",
        ask: "Change customer 88's plan to annual.",
        how: [
          "Needs write mode. You give the table, the values and the primary key, and the server builds the parameterized SQL.",
          "Supports `dry_run` with a before-and-after diff, like `execute_sql`.",
        ],
      },
      import_data: {
        summary: "Inserts many rows from a list.",
        ask: "Add these 30 products to the catalog.",
        how: ["Needs write mode. Takes a JSON array of rows and inserts them in batches. Column order comes from `columns` or from the first row's keys."],
      },
      apply_ddl: {
        summary: "Runs a schema change: CREATE, ALTER, DROP, TRUNCATE.",
        ask: "Add a deleted_at column to orders.",
        how: [
          "Needs admin mode. Runs inside a transaction, and supports `dry_run`.",
          "For a CRITICAL-risk statement such as `DROP TABLE`, nexql-mcp asks you directly (MCP elicitation) when your app supports it. Otherwise the call needs `confirm_destructive: true`.",
          "Works on objects inside the connected database. `CREATE DATABASE` and `DROP DATABASE` are refused in every mode.",
          "Marks the schema index stale, so the next search reflects the change after a refresh.",
        ],
      },
      create_index_concurrently: {
        summary: "Builds an index without blocking writes.",
        ask: "Add an index on orders(customer_id) without locking the table.",
        how: ["Needs admin mode. Accepts only `CREATE INDEX CONCURRENTLY`, which Postgres can't run inside a transaction, so this tool runs it on its own."],
      },
      run_maintenance: {
        summary: "Runs VACUUM, ANALYZE or REINDEX.",
        ask: "Vacuum and analyze the events table.",
        how: [
          "Needs admin mode. Runs outside a transaction, as Postgres requires.",
          "`full: true` runs `VACUUM FULL`, which rewrites the table and locks it while it runs.",
        ],
      },
      terminate_query: {
        summary: "Cancels or ends a running query by its pid.",
        ask: "Cancel the query that's blocking checkout.",
        how: [
          "Needs admin mode. Cancels the query by default; `force: true` ends the whole session.",
          "Refuses to touch superuser sessions or its own.",
        ],
      },
    },
  },
  {
    id: "nexql",
    name: "NexQL itself",
    color: "var(--hue-amber)",
    blurb: "Look after the schema index, check the setup, and load more tools when the assistant needs them.",
    tools: {
      run_doctor: {
        summary: "Checks the connection, permissions and index, and reports problems.",
        ask: "Is everything set up right?",
        how: ["Covers the connection, the role's permissions (including a warning if it's a superuser), session guards and index status. The same checks as `nexql-mcp doctor`."],
      },
      get_index_status: {
        summary: "Says when the schema index was built and whether it's out of date.",
        ask: "Is your view of the schema up to date?",
        how: ["Returns when it was built, its fingerprint and object counts, and whether the live schema has drifted since. Reports `missing` if no index exists yet."],
      },
      rebuild_index: {
        summary: "Rebuilds the schema index from scratch.",
        ask: "Re-read the schema; we just added tables.",
        how: [
          "`full` (default) reads structure and sample values; `shallow` reads structure only and is quicker on big schemas.",
          "Only one build runs per database at a time; a second request waits for the first.",
        ],
        local: true,
      },
      refresh_index: {
        summary: "Updates the schema index using the last build's settings.",
        ask: "Refresh the schema index.",
        how: ["Use after schema changes. `apply_ddl` marks the index stale so the assistant knows to refresh."],
        local: true,
      },
      discover_tools: {
        summary: "Finds and loads more tools on demand.",
        ask: "Do you have anything for lock problems?",
        how: [
          "Mostly for the `meta` tool set, which starts small to save context. The assistant searches by keyword (\"bloat\", \"locks\") or category and gets the matching tools.",
        ],
      },
    },
  },
];

/** Required access mode, from `ToolName::required_mode` in registry.rs. */
const WRITE = new Set(["execute_sql", "edit_row", "import_data"]);
const ADMIN = new Set(["apply_ddl", "create_index_concurrently", "run_maintenance", "terminate_query"]);

export interface DocTool extends ServerTool, ToolNotes {
  line: Line;
  mode: Mode;
}

const server = new Map((catalog.tools as ServerTool[]).map((t) => [t.name, t]));

export const TOOLS: DocTool[] = LINES.flatMap((line) =>
  Object.entries(line.tools).map(([name, notes]) => {
    const t = server.get(name);
    if (!t) throw new Error(`docs.ts documents "${name}", which the server doesn't list. Run scripts/sync-tools.mjs or remove it.`);
    return { ...t, ...notes, line, mode: ADMIN.has(name) ? "admin" : WRITE.has(name) ? "write" : "read" } as DocTool;
  }),
);

const documented = new Set(TOOLS.map((t) => t.name));
const missing = [...server.keys()].filter((n) => !documented.has(n));
if (missing.length) throw new Error(`Tools without docs in docs.ts: ${missing.join(", ")}`);

export const PROMPTS = catalog.prompts as { name: string; description: string; arguments: { name: string; description: string; required?: boolean }[] }[];

export const toolHref = (name: string) => `/docs/tools/${name.replaceAll("_", "-")}`;

/** Feature guides, in reading order. `slug` is the page under /docs/. */
export const GUIDES = [
  { slug: "how-it-works", title: "How a question travels", blurb: "What happens between your question and the answer." },
  { slug: "safety", title: "Access modes and guardrails", blurb: "Read, write and admin, and what's blocked in every mode." },
  { slug: "privacy", title: "Masking personal data", blurb: "Which columns come back as <redacted>, and how to tune it." },
  { slug: "connections", title: "Connections and passwords", blurb: "Profiles, the keyring, and adding databases from the chat." },
  { slug: "schema-index", title: "The schema index", blurb: "How NexQL knows your tables before it writes SQL." },
  { slug: "tool-sets", title: "Tool sets", blurb: "query, dba, meta, full: how many tools your app sees." },
  { slug: "prompts", title: "Prompts and resources", blurb: "Ready-made tasks your app can offer as slash commands." },
  { slug: "cli", title: "Command line", blurb: "Every nexql-mcp command, from setup to diff." },
  { slug: "configuration", title: "Configuration", blurb: "config.toml fields, environment variables, HTTP mode." },
] as const;
