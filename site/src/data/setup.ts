/**
 * /setup: prompts to paste into an AI coding agent so it installs NexQL,
 * saves the connection, wires the app and checks it works. The agent never
 * handles the database password: the user types it in their own terminal.
 *
 * Each scene replays a recorded dry run of PROMPT_INSTALL in Claude Code
 * against the ecommerce_core sample schema. Output is real, trimmed with "…"
 * and with npm's "npm notice" lines left out; profile add, init and doctor
 * show the 0.6.0 output, the rest is from 0.5.0.
 */

import { HUE, type Replay, type Scene } from "./scenes";

const PROFILE = "mydb";
const NPX = "npx -y nexql-mcp@latest";
const CLIENTS =
  "claude-code, claude-desktop, cursor, vscode-copilot, windsurf, zed, antigravity, gemini-cli, codex, copilot-cli, cline, lm-studio, kiro, kimi or qwen";

/** Repeated in every prompt that touches a connection: the agent never handles the password. */
const PASSWORD_RULE = `Never ask me for my database password, and never put one in a command, a file or this chat. If I paste one by accident, don't use it, and tell me to change it.`;

const PROFILE_COMMAND = `${NPX} profile add ${PROFILE} --host <host> --port <port> --dbname <database> --user <user> --access-mode read`;

const SSL_NOTE = "If it needs SSL, add --sslmode require to the end.";

const SAVED = `✓ Profile '${PROFILE}' saved`;

const INIT_STEP = `Run \`${NPX} --profile ${PROFILE} init <client> --apply\`, with <client> set to my app (${CLIENTS}). It prints a diff of the app's MCP config, then "✓ wrote <app> (<path>)", plus a backup path if the file already existed. Show me the diff, and the backup path if there is one.`;

const numbered = (steps: string[]) => steps.map((s, i) => `${i + 1}. ${s}`).join("\n");

const PROMPT_INSTALL = `Set up NexQL MCP (https://github.com/NexQL-OSS/mcp), a Postgres MCP server, for me. Follow these steps in order and don't improvise extra ones.

${PASSWORD_RULE}

${numbered([
  "Run `node --version`. If Node.js isn't installed, tell me to install it from nodejs.org and stop.",
  "Ask me for my database's host, port (5432 if I don't know), database name and user, whether it needs SSL (hosted databases usually do), and which app I'm using you in. Not the password.",
  `Write out this command filled in with my details, and ask me to run it in my own terminal window, not yours:
   ${PROFILE_COMMAND}
   ${SSL_NOTE}
   It asks me for the password without showing it, tests the connection and saves the password in my OS keyring. It needs a real terminal, so it can't work in yours. Wait until I tell you it printed "${SAVED}".`,
  INIT_STEP,
  `Run \`${NPX} --profile ${PROFILE} doctor\` and tell me what it reports.`,
  "Tell me how to restart or reload my app so it picks up NexQL.",
])}`;

const PROMPT_PROFILE = `Help me save my database connection for NexQL MCP. ${PASSWORD_RULE}

Ask me for the host, port, database name and user, and whether it needs SSL. Then write out this command filled in with my details for me to run in my own terminal window:

${PROFILE_COMMAND}

${SSL_NOTE}

Don't run it yourself: it asks for the password without showing it, and that only works in a real terminal. Wait for me to tell you it printed "${SAVED}". If it says the connection failed, help me check the host, port, user and SSL setting, not the password.`;

const PROMPT_WIRE = `Wire NexQL MCP into the app you're running in. Run:

${NPX} --profile ${PROFILE} init <client> --apply

with <client> set to my app (${CLIENTS}). Ask me if you're not sure which. Use the profile name we saved, if it isn't "${PROFILE}".

It merges NexQL into the app's existing MCP config, prints a diff of the change, then "✓ wrote <app> (<path>)", plus a backup path if the file already existed. Show me the diff, and the backup path if there is one.

Then tell me how to restart or reload the app so it loads NexQL.`;

const PROMPT_VERIFY = `Check that NexQL MCP works. Run this, using the profile name we saved:

${NPX} --profile ${PROFILE} doctor

Paste the full output here. If a check fails, tell me what it reported and how to fix it, and don't ask database questions until everything passes. A "no local index" warning is fine: the server builds the index the first time it starts. If it says "password authentication failed", ask me to run \`${NPX} profile set-password ${PROFILE}\` in my own terminal. ${PASSWORD_RULE}`;

const PROMPT_FIRST = `NexQL MCP should now be connected to my Postgres database. Use its MCP tools for this, not raw SQL through a shell or a database driver you write yourself:

1. orient: call this first. One call gives you the foreign-key joins and a digest of the tables, views and functions.
2. search_schema: only if orient didn't already surface what I'm asking about.
3. get_join_path: only if the join I need isn't already in orient's overview.

After orient, ask me what I actually want to know before running any queries. Don't invent a question to demo the tools.

If I want another database connected later, call add_database with its host, port, database and user. It opens a page on my computer where I type the password myself.`;

const STANDING_RULE = `Whenever you need to read, search, or modify data in this project's Postgres database, use the nexql-mcp MCP tools. Do not open a raw psql/DB-driver connection or hand-write a connection string.

- Call \`orient\` first on any database you haven't explored yet in this session. One call gives you the foreign-key joins and a digest of tables, views and functions instead of guessing.
- Use \`search_schema\` / \`describe_object\` before writing SQL. Never assume a table or column exists.
- Prefer \`run_select\` for reads; it enforces read-only, row limits and masking of personal data.
- For writes use \`execute_sql\` / \`edit_row\`; for schema changes use \`apply_ddl\`. Both run inside a transaction and support \`dry_run: true\` to preview first.
- If a statement is flagged CRITICAL risk (e.g. DROP TABLE, TRUNCATE), stop and get my explicit confirmation. Never pass \`confirm_destructive: true\` on your own judgment.
- nexql-mcp is read-only by default. Check \`get_current_context\` before assuming you can write.
- Never ask me for a database password, or put one in a command, a file or the chat. To connect another database, call \`add_database\`; I'll type the password on the page it opens.`;

/* ---------- From the dry run ---------- */

const RUN_NODE = `$ node --version
v26.12.1`;

const RUN_PROFILE = `$ ${NPX} profile add ${PROFILE} --host localhost --port 5432 --dbname ecommerce_core --user app_reader --access-mode read
password for '${PROFILE}' (hidden; leave empty if none):
✓ Connection verified: PostgreSQL 17.11 (Debian 17.11-1.pgdg13+2) on x86_64-pc-linux-gnu (latency: 84ms)
${SAVED} to ~/.config/nexql-mcp/config.toml`;

const RUN_INIT = `$ ${NPX} --profile ${PROFILE} init claude-code --apply
--- /dev/null
+++ ~/shop/.mcp.json
@@ -0,0 +1,13 @@
+{
+  "mcpServers": {
+    "nexql-mcp": {
+      "args": [
+        "-y",
+        "nexql-mcp@latest",
+        "--profile",
+        "${PROFILE}"
+      ],
+      "command": "npx"
+    }
+  }
+}
\\ No newline at end of file
✓ wrote Claude Code (~/shop/.mcp.json)`;

const RUN_DOCTOR = `$ ${NPX} --profile ${PROFILE} doctor
resolved source=Profile profile=Some("${PROFILE}")
connected: PostgreSQL 17.11 (Debian 17.11-1.pgdg13+2) on x86_64-pc-linux-gnu
WARNING: connected as a superuser. A prompt-injected query can read server files (pg_read_file) and control the database server. Create role \`nexql_readonly\` (GRANT pg_read_all_data) and reconnect as that role. Copy-paste SQL: \`nexql-mcp profile readonly-role\`.
-- Least-privilege login for nexql-mcp (PostgreSQL 14+).
-- Replace the password, then point the profile at this role instead of a superuser.
CREATE ROLE nexql_readonly WITH LOGIN PASSWORD 'replace-with-a-secret';
GRANT pg_read_all_data TO nexql_readonly;
access_mode=Read superuser=WARNING
session default_transaction_read_only=on statement_timeout=30s
grants: can SELECT information_schema.tables
masking: personal data on; 11 column(s) masked in results: billing.v_unpaid_invoices.customer_email, customers.addresses.street_address, …
warning: pg_stat_statements extension not installed (slow_queries / index tools degraded)
warning: no local index for conn=${PROFILE} db=ecommerce_core under ~/.local/share/nexql-mcp — run \`nexql-mcp index build\`
doctor: ok`;

const RUN_INDEX = `index: no schema index for ${PROFILE}/ecommerce_core — building automatically
index build: Started
…
index build: Finished { build_ms: 29 }
index built: conn=${PROFILE} db=ecommerce_core tables=24 views=6 functions=135 enums=0 shards=9 … (29ms, 11 queries)`;

const RUN_ORIENT = `[context connectionId=${PROFILE} database=ecommerce_core accessMode=read]
{
  "database": "ecommerce_core",
  "enums": {},
  "joins": [
    {
      "declared": true,
      "detection": "declared_fk",
      "disabled": false,
      "edge": "billing.invoices.order_id -> orders.orders.id",
      "via": "invoices_order_id_fkey"
    },
    {
      "declared": true,
      "detection": "declared_fk",
      "disabled": false,
      "edge": "billing.invoices.customer_id -> customers.users.id",
      "via": "invoices_customer_id_fkey"
    },
…`;

const scenes: Scene[] = [
  {
    chapter: "Get started",
    label: "Paste one prompt",
    role: "YOU PASTE",
    where: "",
    prompt:
      "Set up NexQL MCP (https://github.com/NexQL-OSS/mcp), a Postgres MCP server, for me. Follow these steps in order and don't improvise extra ones. …",
    steps: [
      ["1", "runs node --version", "v26.12.1"],
      ["2", "asks for host, port, database, user, SSL and your app", "not the password"],
    ],
    insight:
      "Node v26.12.1 is installed, so we can continue. Next I need your connection details (not the password): host, port (5432 if you're not sure), database name, user, whether it needs SSL (hosted databases usually do), and which app you're using me in.",
    figures: [
      ["1 prompt", "TO PASTE"],
      ["0", "PASSWORDS IN CHAT"],
    ],
    mark: {
      kind: "code",
      title: "Start here",
      sub: "Copy → paste into any AI coding agent with terminal access",
      blocks: [
        {
          label: "PASTE INTO YOUR AGENT",
          code: PROMPT_INSTALL,
          hint: "Covers every step on this page. The steps after this one show the rest of the same session, each with its own prompt if you'd rather go one at a time.",
        },
        {
          label: "WHAT YOUR AGENT RAN",
          code: RUN_NODE,
        },
        {
          label: "OR DO IT ALL IN YOUR BROWSER",
          code: `${NPX} setup`,
          hint: "Opens a page on your computer where you add the connection, type the password and pick your apps. No agent needed.",
        },
      ],
    },
  },
  {
    chapter: "Connect",
    label: "Save your connection",
    role: "YOU REPLY",
    where: "",
    prompt:
      "Host localhost, port 5432, database ecommerce_core, user app_reader. No SSL, it's a local Docker database. I'm using you in Claude Code, in this project folder.",
    steps: [
      ["1", "fills in the profile add command, but doesn't run it", "—"],
      ["2", "you run it in your own terminal and type the password", "hidden"],
      ["3", "it tests the connection and saves the profile", "84ms"],
    ],
    insight:
      "Thanks. It's local with no SSL, so it doesn't need --sslmode. Please run this in your own terminal window, not mine. It asks for your password without showing it, tests the connection, and saves the password in your OS keyring. Let me know when it prints \"✓ Profile 'mydb' saved\".",
    figures: [
      ["0", "PASSWORDS IN CHAT"],
      ["read", "ACCESS MODE"],
    ],
    mark: {
      kind: "code",
      title: "Save your connection",
      sub: "Your agent fills in the details; you run the command",
      blocks: [
        {
          label: "PASTE INTO YOUR AGENT",
          code: PROMPT_PROFILE,
        },
        {
          label: "IN YOUR OWN TERMINAL",
          code: RUN_PROFILE,
          hint: "Nothing shows while you type the password. The agent only hears back that it printed the saved line.",
        },
        {
          label: "OR RUN IT YOURSELF",
          code: PROFILE_COMMAND,
          hint: "Replace the <placeholders>. If the database needs SSL, add --sslmode require.",
        },
      ],
    },
  },
  {
    chapter: "Wire up",
    label: "Add NexQL to your app",
    role: "YOU REPLY",
    where: "",
    prompt: `OK, that worked: ${SAVED} to ~/.config/nexql-mcp/config.toml`,
    steps: [
      ["1", "runs init claude-code --apply", "✓ wrote"],
      ["2", "shows you the diff it printed", "new file, no backup"],
    ],
    insight:
      "The profile is saved and the connection test passed, so I've wired NexQL into Claude Code. This project had no .mcp.json before, so there's no backup. The entry starts NexQL through npx and names the profile only. No connection string or password.",
    figures: [
      ["1 file", "APP MCP CONFIG"],
      ["0", "SECRETS IN IT"],
    ],
    mark: {
      kind: "code",
      title: "Wire your app",
      sub: "What gets written: a profile name, not a connection string",
      blocks: [
        {
          label: "PASTE INTO YOUR AGENT",
          code: PROMPT_WIRE,
        },
        {
          label: "WHAT YOUR AGENT RAN",
          code: RUN_INIT,
          hint: "init merges NexQL into your app's existing MCP config and keeps the other servers, hiding their secrets in the diff. Run through npx, the entry starts npx; with NexQL installed, it starts that copy. If the file already existed, it also prints where it backed up the old one.",
        },
        {
          label: "OR PICK APPS IN THE BROWSER",
          code: `${NPX} setup`,
          hint: "Clients tab: tick your apps, check the diff, Apply. VS Code uses a servers block, and Windsurf and Antigravity have their own file paths; init and setup pick the right one for each app.",
        },
      ],
    },
  },
  {
    chapter: "Check",
    label: "Prove it works",
    role: "YOU REPLY",
    where: "",
    prompt: "Yes, continue.",
    steps: [
      ["1", "runs doctor against your profile", "0.5s"],
      ["2", "reads the report back to you", "doctor: ok"],
      ["3", "you quit with /exit, run claude again and approve nexql-mcp", "/mcp"],
    ],
    insight:
      "Doctor passed. It reaches PostgreSQL 17.11 with the mydb profile, and every session is read-only with a 30s statement timeout. It warns that you're connected as a superuser: NexQL still keeps the session read-only, but a dedicated read-only role is safer, and it printed the SQL for one. There's no local schema index yet; the server builds one the first time it starts. Claude Code reads .mcp.json only at startup, so quit with /exit, run claude again in this folder, approve nexql-mcp, then check /mcp.",
    figures: [
      ["ok", "DOCTOR"],
      ["read-only", "EVERY SESSION"],
      ["11", "COLUMNS MASKED"],
    ],
    mark: {
      kind: "code",
      title: "Prove it works",
      sub: "Run before your first database question",
      blocks: [
        {
          label: "PASTE INTO YOUR AGENT",
          code: PROMPT_VERIFY,
        },
        {
          label: "WHAT YOUR AGENT RAN",
          code: RUN_DOCTOR,
          hint: "This database logs in as a superuser, so doctor warns and prints SQL for a safer read-only role. It also lists the personal-data columns it masks in results.",
        },
        {
          label: "OR RUN IN YOUR OWN TERMINAL",
          code: `${NPX} --profile ${PROFILE} doctor`,
        },
      ],
    },
  },
  {
    chapter: "Use it",
    label: "Ask your first question",
    role: "YOU",
    where: "",
    prompt: "NexQL shows as connected. Start with orient and give me an overview of the database.",
    steps: [
      ["1", "the server starts and builds its schema index", "29ms"],
      ["orient", "one call: foreign-key joins, then a digest of tables, views and functions", "474 lines"],
    ],
    insight:
      "ecommerce_core is an e-commerce schema split into six areas: customers, catalog, orders, billing, inventory and core/audit. Orient found 26 foreign-key joins: orders link to customers, invoices link to orders, and transactions link to invoices and payment methods. It found no enum-like columns. There are also ready-made views, like billing.v_unpaid_invoices and inventory.v_low_stock_alerts. What would you like to know?",
    figures: [
      ["1 call", "SCHEMA OVERVIEW"],
      ["26", "JOINS FOUND"],
    ],
    mark: {
      kind: "code",
      title: "First real question",
      sub: "Paste once NexQL shows as connected in your app",
      blocks: [
        {
          label: "PASTE INTO YOUR AGENT",
          code: PROMPT_FIRST,
          hint: "The agent uses NexQL's MCP tools, not a raw SQL connection, so it finds tables before writing queries.",
        },
        {
          label: "SERVER LOG ON FIRST START",
          code: RUN_INDEX,
        },
        {
          label: "WHAT ORIENT RETURNED (FIRST LINES)",
          code: RUN_ORIENT,
          hint: 'The rest: 26 joins, then a digest of 24 tables, 4 views, 2 materialized views and 10 functions, and a note: "Showing 40 of 165 objects — pass `focus` to narrow the digest."',
        },
      ],
    },
  },
  {
    chapter: "Keep it",
    label: "Make it stick",
    role: "YOU",
    where: "",
    prompt: "How do I make sure my agent always uses NexQL for database work, not just this once?",
    steps: [
      ["1", "you paste the rule into your agent's rules file", "—"],
      ["2", "agent adopts it as a standing instruction", "—"],
      ["3", "every future database question goes through NexQL", "—"],
    ],
    insight:
      "One paste works everywhere. Claude Code reads CLAUDE.md, Cursor reads .cursor/rules, and most other agents accept the same block as an AGENTS.md or project instructions file.",
    figures: [
      ["1 paste", "WORKS IN ANY AGENT"],
      ["0", "PASSWORDS IN CHAT"],
    ],
    mark: {
      kind: "code",
      title: "Make it permanent",
      sub: "Add once; every agent session picks it up",
      blocks: [
        {
          label: "PASTE INTO YOUR AGENT RULES FILE",
          code: STANDING_RULE,
          hint: "Claude Code → CLAUDE.md; Cursor → .cursor/rules; many agents → AGENTS.md. Same block everywhere.",
        },
      ],
    },
  },
];

export const SETUP: Replay = {
  scenes,
  agentLabel: "Your agent",
  stepsLabel: "Then your agent does",
  roleHue: () => HUE.accent,
};
