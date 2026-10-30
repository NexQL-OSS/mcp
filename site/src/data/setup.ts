/**
 * /setup: prompts to paste into an AI coding agent so it installs NexQL,
 * saves the connection, wires the app and checks it works. The agent never
 * handles the database password: the user types it in their own terminal.
 */

import { FACTS } from "./site";
import { HUE, type Replay, type Scene } from "./scenes";

const PROFILE = "mydb";
const NPX = "npx -y nexql-mcp@latest";
const CLIENTS = "claude-code, claude-desktop, cursor, vscode-copilot, windsurf, zed, antigravity, gemini-cli, codex, copilot-cli, cline, lm-studio, kiro, kimi or qwen";

/** Repeated in every prompt that touches a connection: the agent never handles the password. */
const PASSWORD_RULE = `Never ask me for my database password, and never put one in a command, a file or this chat. If I paste one by accident, don't use it, and tell me to change it.`;

const PROFILE_COMMAND = `${NPX} profile add ${PROFILE} --host <host> --port <port> --dbname <database> --user <user> --access-mode read`;

const PROMPT_INSTALL = `Set up NexQL MCP (https://github.com/NexQL-OSS/mcp), a Postgres MCP server, for me. Follow these steps in order and don't improvise extra ones.

${PASSWORD_RULE}

1. Run \`node --version\`. If Node.js isn't installed, tell me to install it from nodejs.org and stop.
2. Ask me for my database's host, port (5432 if I don't know), database name and user, whether it needs SSL (hosted databases usually do), and which app I'm using you in. Not the password.
3. Write out this command filled in with my details, adding \`--sslmode require\` if it needs SSL, and ask me to run it in my own terminal window, not yours:
   ${PROFILE_COMMAND}
   It asks me for the password without showing it and saves it in my OS keyring. It needs a real terminal, so it can't work in yours. Wait until I tell you it printed "Profile '${PROFILE}' saved".
4. Run \`${NPX} --profile ${PROFILE} init <client> --apply\`, with <client> set to my app (${CLIENTS}). Show me the diff it printed and where it put the backup.
5. Run \`${NPX} --profile ${PROFILE} doctor\` and tell me what it reports.
6. Tell me how to restart or reload my app so it picks up NexQL.`;

const PROMPT_PROFILE = `Help me save my database connection for NexQL MCP. ${PASSWORD_RULE}

Ask me for the host, port, database name and user, and whether it needs SSL. Then write out this command filled in with my details (add \`--sslmode require\` if it needs SSL) for me to run in my own terminal window:

${PROFILE_COMMAND}

Don't run it yourself: it asks for the password without showing it, and that only works in a real terminal. Wait for me to tell you it printed "Profile '${PROFILE}' saved". If it says the connection failed, help me check the host, port, user and SSL setting, not the password.`;

const PROMPT_WIRE = `Wire NexQL MCP into the app you're running in. Run:

${NPX} --profile ${PROFILE} init <client> --apply

with <client> set to my app (${CLIENTS}). Ask me if you're not sure which. Use the profile name we saved, if it isn't "${PROFILE}".

It merges NexQL into the app's existing MCP config and keeps a backup. Show me the diff it printed and the backup path. Then tell me how to restart or reload the app so it loads NexQL.`;

const PROMPT_VERIFY = `Check that NexQL MCP works. Run this, using the profile name we saved:

${NPX} --profile ${PROFILE} doctor

Paste the full output here. If a check fails, tell me what it reported and how to fix it, and don't ask database questions until everything passes. If it says "password authentication failed", ask me to run \`${NPX} profile set-password ${PROFILE}\` in my own terminal. ${PASSWORD_RULE}`;

const PROMPT_FIRST = `NexQL MCP should now be connected to my Postgres database. Use its MCP tools for this, not raw SQL through a shell or a database driver you write yourself:

1. orient: call this first. One call gives you the tables, foreign-key joins and enum-like columns.
2. search_schema: only if orient didn't already surface what I'm asking about.
3. get_join_path: only if the join I need isn't already in orient's overview.

Ask me what I actually want to know before running anything. Don't invent a question to demo the tools.

If I want another database connected later, call add_database with its host, port, database and user. It opens a page on my computer where I type the password myself.`;

const STANDING_RULE = `Whenever you need to read, search, or modify data in this project's Postgres database, use the nexql-mcp MCP tools. Do not open a raw psql/DB-driver connection or hand-write a connection string.

- Call \`orient\` first on any database you haven't explored yet in this session. One call gives you tables, foreign-key joins and enum-like columns instead of guessing.
- Use \`search_schema\` / \`describe_object\` before writing SQL. Never assume a table or column exists.
- Prefer \`run_select\` for reads; it enforces read-only, row limits and masking of personal data.
- For writes use \`execute_sql\` / \`edit_row\`; for schema changes use \`apply_ddl\`. Both run inside a transaction and support \`dry_run: true\` to preview first.
- If a statement is flagged CRITICAL risk (e.g. DROP TABLE, TRUNCATE), stop and get my explicit confirmation. Never pass \`confirm_destructive: true\` on your own judgment.
- nexql-mcp is read-only by default. Check \`get_current_context\` before assuming you can write.
- Never ask me for a database password, or put one in a command, a file or the chat. To connect another database, call \`add_database\`; I'll type the password on the page it opens.`;

const EXAMPLE_MCP_CONFIG = `{
  "mcpServers": {
    "nexql-mcp": {
      "command": "npx",
      "args": ["-y", "nexql-mcp@latest", "--profile", "${PROFILE}"]
    }
  }
}`;

const scenes: Scene[] = [
  {
    chapter: "Get started",
    label: "One prompt does it all",
    role: "YOU",
    where: "",
    prompt: "I want NexQL MCP in my coding agent. Give me a prompt to paste.",
    steps: [
      ["1", "you paste the prompt into your agent", "—"],
      ["2", "agent asks for host, database and user, never the password", "—"],
      ["3", "you type the password once, in your own terminal", "keyring"],
    ],
    insight:
      "Your agent does the typing, except for one thing: the password. You enter that once in your own terminal, hidden, and it goes straight to your OS keyring. It never passes through the chat.",
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
          hint: "Covers every step on this page. The ones after it break the same setup into parts, if you'd rather go one at a time.",
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
    role: "YOU",
    where: "",
    prompt: "How do I add my database without giving you the password?",
    steps: [
      ["1", "agent asks for host, port, database and user", "—"],
      ["2", "you run profile add in your own terminal", "hidden prompt"],
      ["3", "connection tested, password saved to your OS keyring", "keyring"],
    ],
    insight:
      "The password is the one thing your agent never handles. Chats are stored by your AI provider, so you type it in your own terminal instead, where it's hidden and goes straight into Keychain, Credential Manager or Secret Service.",
    figures: [
      ["0", "PASSWORDS IN CHAT"],
      ["read", "DEFAULT MODE"],
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
          label: "OR RUN IN YOUR OWN TERMINAL",
          code: PROFILE_COMMAND,
          hint: "Replace the <placeholders>. Add --sslmode require for hosted databases. It asks for the password without showing it.",
        },
      ],
    },
  },
  {
    chapter: "Wire up",
    label: "Add NexQL to your app",
    role: "YOU",
    where: "",
    prompt: "Wire NexQL into my app. I want to see what changes before anything is written.",
    steps: [
      ["1", "you paste the wire-up prompt", "—"],
      ["2", `agent runs init <client> --apply`, "—"],
      ["3", "shows the config diff, keeps a backup", "—"],
    ],
    insight:
      "init merges NexQL into your app's existing MCP config without removing other servers, and backs up the old file. The config names your saved profile, never a connection string or password.",
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
          label: "OR PICK APPS IN THE BROWSER",
          code: `${NPX} setup`,
          hint: "Clients tab: tick your apps, check the diff, Apply.",
        },
        {
          label: "WHAT IT WRITES (CLAUDE-STYLE CONFIG)",
          code: EXAMPLE_MCP_CONFIG,
          hint: "VS Code uses a servers block, Codex uses TOML, and most apps have their own file paths. init picks the right one for the app you name.",
        },
      ],
    },
  },
  {
    chapter: "Check",
    label: "Prove it works",
    role: "YOU",
    where: "",
    prompt: "How do I know it's working before I ask real questions?",
    steps: [
      ["1", "you paste the check prompt", "—"],
      ["2", "agent runs doctor against your profile", "—"],
      ["3", "restart your app once everything passes", "—"],
    ],
    insight:
      "doctor opens a real connection and checks the role's permissions, read-only mode and the schema index. If it passes, your app will work too.",
    figures: [
      ["1", "COMMAND"],
      ["read-only", "CONFIRMED"],
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
          label: "OR RUN IN YOUR OWN TERMINAL",
          code: `${NPX} --profile ${PROFILE} doctor`,
          hint: "Warns you if the connection logs in as a superuser, and prints SQL for a safer read-only role.",
        },
      ],
    },
  },
  {
    chapter: "Use it",
    label: "Ask your first question",
    role: "YOU",
    where: "",
    prompt: "What should I ask now that NexQL is connected?",
    steps: [
      ["orient", "one-call overview: tables, joins, enums", "41ms"],
      ["search_schema", "only if orient didn't surface it", "34ms"],
      ["get_join_path", "only if the join isn't already in orient's overview", "12ms"],
    ],
    insight:
      "Start with orient: one call instead of a chain of searches. Everything is read-only by default, so your agent can look and query but not change anything. More databases later? Ask, and add_database opens a page for the password.",
    figures: [
      ["1 call", "SCHEMA OVERVIEW"],
      [String(FACTS.toolCount), "TOOLS IN TOTAL"],
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
