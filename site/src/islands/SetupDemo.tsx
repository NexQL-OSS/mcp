import { FACTS } from "../data/site";
import { AMBER, type Scene } from "./story-shared";
import { NarrativePlayer } from "./NarrativePlayer";

const ROLE = "YOU";
const ROLE_HUE = AMBER;

/** Only used in "what gets written" illustrations — never in a PASTE INTO YOUR AGENT block. */
const DB_URL_EXAMPLE = "postgres://user:password@host:5432/dbname";
const PROFILE = "mydb";
const NPX = "npx -y nexql-mcp@latest";

const PROMPT_INSTALL = `Set up NexQL MCP — a standalone Postgres MCP server (https://github.com/NexQL-OSS/mcp) — for me. Follow these steps exactly, in order. Do not skip ahead or improvise extra steps.

1. Run this exact command in the terminal: ${NPX} setup
   Do not install it any other way — no "npm install -g", no cloning the repo, no building from source. That one command is the entire install.
2. It will print a local URL on 127.0.0.1 in the terminal output. Tell me that URL.
3. You most likely cannot see or click inside a web browser yourself — do not assume you can, and do not go quiet waiting for something to happen there. Pick one, based on what you actually have:
   - If I ask you to walk me through the wizard, guide me one tab at a time (Profiles, then Clients) — tell me exactly what to enter or click next, and wait for me to confirm each step before moving on. I'll do the clicking.
   - Otherwise, skip the browser entirely: ask me for my database connection string and do the whole setup yourself over the terminal (profile add, then init <client> --apply) — no browser needed either way.
4. Either path: if I have not already told you my database connection string, ask me for it now — do not guess one, and do not reuse an example from documentation.`;

const PROMPT_PROFILE = `Guide me through the NexQL setup wizard's Profiles tab to create a stored profile — tell me exactly what to enter and click, since you likely can't see the browser yourself. I'll do the clicking.

If you don't already have these from me, ask before telling me what to enter:
- What should the profile be called? (any short name is fine, e.g. "${PROFILE}")
- What is the full Postgres connection string? (e.g. postgres://user:password@host:5432/dbname)
- Access mode: leave it on read-only (the default) unless I explicitly ask for write or admin access.

Tell me to click Test Connection, and wait for me to confirm it passed before telling me to click Save. The password must land in the OS keyring — if I report it fell back to plaintext or a keyring error, stop and tell me what to do before continuing.`;

const PROMPT_WIRE = `Guide me through the NexQL setup wizard's Clients tab to wire up my client — tell me exactly what to click, since you likely can't see the browser yourself. I'll do the clicking.

Ask me which client I'm using if you don't already know (Claude Code, Claude Desktop, Cursor, VS Code, Windsurf, or another supported client), then tell me which one to select and which profile to wire it to.

Ask me to confirm the config diff looks right before I click Apply — the wizard backs up the existing file automatically. After I apply it, tell me which client setting to reload (e.g. Cursor: Settings → MCP → refresh) so the new server shows up.`;

const PROMPT_WIRE_APPLY = `${NPX} init <client> --apply --profile ${PROFILE}

Non-interactive alternative to the wizard's Clients tab — no browser needed. Replace <client> with whichever one I'm using: claude-code, claude-desktop, cursor, vscode-copilot, zed, windsurf, or another supported client — ask me if you don't already know. Replace "${PROFILE}" with whatever we actually named the profile in the previous step if it wasn't that. It writes the merged config file directly (with a backup of what was there) and prints the diff it applied.`;

const PROMPT_VERIFY = `Verify NexQL MCP is working. Run this exact command in the terminal, using whatever profile name we saved earlier (this assumes "${PROFILE}" — swap it if we called it something else):

${NPX} --profile ${PROFILE} doctor

Paste the full output here. If any check fails, tell me exactly what it reported and what to fix — do not proceed to ask database questions until every check passes.`;

const PROMPT_FIRST = `NexQL MCP should now be connected to my Postgres database. Use its MCP tools for this — not raw SQL through a shell, and not a database driver you write yourself:

1. orient — call this first, always. One-call schema bootstrap: tables, FK joins, enum-like columns.
2. search_schema — only if orient didn't already surface what I'm asking about.
3. get_join_path — only if the join I need isn't already in orient's digest.

Ask me what I actually want to know about the data before running anything — don't invent a question to demo the tools.`;

const STANDING_RULE = `Whenever you need to read, search, or modify data in this project's Postgres database, use the nexql-mcp MCP tools — do not open a raw psql/DB-driver connection or hand-write a connection string.

- Call \`orient\` first on any database you haven't explored yet in this session — one call gives you tables, FK joins, and enum-like columns instead of guessing.
- Use \`search_schema\` / \`describe_object\` before writing SQL — never assume a table or column exists.
- Prefer \`run_select\` for reads; it enforces read-only, row limits, and PII masking automatically.
- For writes use \`execute_sql\` / \`edit_row\`; for schema changes use \`apply_ddl\`. Both run inside a transaction and support \`dry_run: true\` to preview first.
- If a statement is flagged CRITICAL risk (e.g. DROP TABLE, TRUNCATE), stop and get my explicit confirmation before proceeding — never pass \`confirm_destructive: true\` on your own judgment.
- nexql-mcp is read-only by default — check \`get_current_context\` before assuming you can write.`;

const CURSOR_CONFIG = `{
  "mcpServers": {
    "nexql-mcp": {
      "command": "npx",
      "args": ["-y", "nexql-mcp", "--profile", "${PROFILE}"]
    }
  }
}`;

const SCENES: Scene[] = [
  {
    act: 0,
    role: ROLE,
    where: "Step 1",
    prompt: "I want NexQL MCP in Cursor — give me a prompt to paste.",
    steps: [
      ["1", "you paste the prompt into Cursor chat", "—"],
      ["2", `agent runs ${NPX} setup in the terminal`, "—"],
      ["3", "browser wizard opens — agent walks you through it", "local only"],
    ],
    insight:
      "NexQL won't install itself. Paste the prompt below — your agent runs the command and guides you through the wizard. Nothing leaves your machine.",
    figures: [
      ["1 prompt", "TO PASTE"],
      ["0", "MANUAL JSON"],
    ],
    mark: {
      kind: "code",
      title: "Start here",
      sub: "Copy → paste into Cursor, Claude Code, or any agent with terminal access",
      blocks: [
        {
          label: "PASTE INTO YOUR AGENT",
          code: PROMPT_INSTALL,
          hint: "The agent should run the npx command for you — you approve the terminal prompt when asked.",
        },
        {
          label: "OR RUN YOURSELF IN TERMINAL",
          code: `${NPX} setup`,
          hint: "Same result — opens the local setup page in your browser.",
        },
      ],
    },
    alts: [1, 2],
  },
  {
    act: 1,
    role: ROLE,
    where: "Step 2 · Profiles",
    prompt: "How do I save my database connection without putting URLs in every client?",
    steps: [
      ["1", "you paste the profile prompt", "—"],
      ["2", "agent fills the Profiles tab in the wizard", "—"],
      ["3", "connection test passes → saved to ~/.config/nexql-mcp/", "keyring"],
    ],
    insight:
      "Profiles are the single source of truth. Every client references --profile by name — switch databases by editing one file, not five MCP configs.",
    figures: [
      ["1 profile", "ALL CLIENTS SHARE"],
      ["read", "DEFAULT MODE"],
    ],
    mark: {
      kind: "code",
      title: "Add your database",
      sub: "Paste after the wizard opens",
      blocks: [
        {
          label: "PASTE INTO YOUR AGENT",
          code: PROMPT_PROFILE,
        },
        {
          label: "OR RUN YOURSELF IN TERMINAL",
          code: `${NPX} profile add ${PROFILE} --url ${DB_URL_EXAMPLE}`,
          hint: "Replace the URL with your own connection string. Equivalent to the wizard — then skip to Step 3.",
        },
      ],
    },
    alts: [2, 3],
  },
  {
    act: 2,
    role: ROLE,
    where: "Step 3 · Clients",
    prompt: "Wire Cursor — I want to see the config diff before anything is written.",
    steps: [
      ["1", "you paste the wire-up prompt", "—"],
      ["2", "agent selects Cursor in the Clients tab", "—"],
      ["3", "shows .cursor/mcp.json diff → Apply with backup", "—"],
    ],
    insight:
      "The wizard merges into your existing MCP config — it won't remove other servers. You see the exact diff before Apply. An agent with terminal access can skip the browser entirely with one non-interactive command.",
    figures: [
      ["1 file", ".CURSOR/MCP.JSON"],
      ["+6 lines", "TYPICAL DIFF"],
    ],
    mark: {
      kind: "code",
      title: "Wire Cursor",
      sub: "What gets written — profile name, not a connection string",
      blocks: [
        {
          label: "PASTE INTO YOUR AGENT",
          code: PROMPT_WIRE,
        },
        {
          label: "OR — NO BROWSER NEEDED",
          code: PROMPT_WIRE_APPLY,
          hint: "This is the recommended path when an agent is doing the setup on your behalf — non-interactive, writes the file directly.",
        },
        {
          label: "RESULTING CONFIG",
          code: CURSOR_CONFIG,
          hint: "Claude Desktop, VS Code, Windsurf work the same — tick them in the wizard Clients tab, or swap the client name in init --apply.",
        },
      ],
    },
    alts: [3, 4],
  },
  {
    act: 3,
    role: ROLE,
    where: "Step 4 · verify",
    prompt: "How do I know it's working before I ask real questions?",
    steps: [
      ["1", "you paste the verify prompt", "—"],
      ["2", "agent runs doctor against your profile", "14 checks"],
      ["3", "you reload Cursor MCP panel if all green", "—"],
    ],
    insight: "doctor opens a real connection and checks permissions + index state. If it passes, your agent will work too.",
    figures: [
      ["14", "HEALTH CHECKS"],
      ["0", "SHOULD FAIL"],
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
          label: "OR RUN YOURSELF IN TERMINAL",
          code: `${NPX} --profile ${PROFILE} doctor`,
          hint: "Also builds the schema index on first run if missing — makes search_schema fast.",
        },
      ],
    },
    alts: [4, 0],
  },
  {
    act: 4,
    role: ROLE,
    where: "Step 5 · first question",
    prompt: "What should I ask now that NexQL is connected?",
    steps: [
      ["orient", "one-call schema bootstrap: tables, FK joins, enums", "41ms"],
      ["search_schema", "only if orient didn't surface it", "34ms"],
      ["get_join_path", "only if the join isn't already in orient's digest", "12ms"],
    ],
    insight: `Start with orient, every time — it replaces the search_schema → describe_object → get_join_path chain with one call. All ${FACTS.toolCount} tools are available in read mode by default.`,
    figures: [
      ["1 call", "SCHEMA BOOTSTRAP"],
      [String(FACTS.toolCount), "TOOLS READY"],
    ],
    mark: {
      kind: "code",
      title: "First real question",
      sub: "Paste once NexQL shows as connected in your client",
      blocks: [
        {
          label: "PASTE INTO YOUR AGENT",
          code: PROMPT_FIRST,
          hint: "The agent uses NexQL's MCP tools — not a SQL socket — so it finds tables before writing queries.",
        },
      ],
    },
    alts: [5, 0],
  },
  {
    act: 5,
    role: ROLE,
    where: "Step 6 · make it permanent",
    prompt: "How do I make sure my agent always uses NexQL for database work — not just this once?",
    steps: [
      ["1", "you paste the rule into your agent's rules file", "—"],
      ["2", "agent adopts it as a standing instruction", "—"],
      ["3", "every future DB question goes through NexQL's tools", "—"],
    ],
    insight:
      "One paste, works everywhere. Claude Code reads CLAUDE.md, Cursor reads .cursorrules or .cursor/rules, and most other coding agents (Gemini CLI, Windsurf, Continue, …) accept the same block as a project instructions file — no per-agent rewriting.",
    figures: [
      ["1 paste", "WORKS IN ANY AGENT"],
      ["0", "RAW SQL SHORTCUTS"],
    ],
    mark: {
      kind: "code",
      title: "Make it permanent",
      sub: "Add once — every agent session picks it up automatically",
      blocks: [
        {
          label: "PASTE INTO CLAUDE.md / .cursorrules / AGENTS.md",
          code: STANDING_RULE,
          hint: "Works verbatim for Claude Code, Cursor, Windsurf, Gemini CLI, or any agent that reads a project rules file — nothing NexQL-specific to translate.",
        },
      ],
    },
    alts: [0, 1],
  },
];

const ACTS: [string, number][] = [
  ["GET", 0],
  ["PROFILE", 1],
  ["WIRE", 2],
  ["VERIFY", 3],
  ["LIVE", 4],
  ["RULE", 5],
];

interface SetupDemoProps {
  autoplay?: boolean;
  sceneSeconds?: number;
  startScene?: number;
  height?: string;
}

export function SetupDemo({ autoplay = true, sceneSeconds = 28, startScene = 1, height = "100dvh" }: SetupDemoProps) {
  return (
    <NarrativePlayer
      scenes={SCENES}
      acts={ACTS}
      roleHue={() => ROLE_HUE}
      agentLabel="YOUR AGENT"
      stepsLabel="THEN YOUR AGENT DOES"
      footerNote="copy prompts · paste into your agent"
      mintGlowFromScene={4}
      mode="setup"
      autoplay={autoplay}
      sceneSeconds={sceneSeconds}
      startScene={startScene}
      height={height}
    />
  );
}
