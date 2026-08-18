import { FACTS } from "../data/site";
import { AMBER, type Scene } from "./story-shared";
import { NarrativePlayer } from "./NarrativePlayer";

const ROLE = "YOU";
const ROLE_HUE = AMBER;

const DB_URL = "postgres://dev@localhost:5432/appdb";
const PROFILE = "mydb";

const PROMPT_INSTALL = `Help me set up NexQL MCP — a Postgres MCP server (https://github.com/NexQL-OSS/mcp).

1. Run in the terminal: npx -y nexql-mcp setup
2. Tell me when the browser wizard opens at 127.0.0.1 and walk me through each tab.
3. Do not skip the connection test.

My database URL is ${DB_URL} — ask me if you need a different one.`;

const PROMPT_PROFILE = `In the NexQL setup wizard, Profiles tab — create a stored profile:

- Profile name: ${PROFILE}
- Database URL: ${DB_URL}
- Access mode: read-only (default)

Click Test Connection, wait for it to pass, then Save. Store the password in the OS keyring — not in the config file as plaintext.`;

const PROMPT_WIRE = `In the NexQL setup wizard, Clients tab — wire Cursor to profile "${PROFILE}":

- Command: npx
- Args: ["-y", "nexql-mcp", "--profile", "${PROFILE}"]
- Target file: .cursor/mcp.json

Show me the full diff before Apply. Back up the existing file. After writing, tell me to reload Cursor's MCP panel (Settings → MCP → refresh).`;

const PROMPT_VERIFY = `Verify NexQL MCP is working. Run in the terminal:

npx -y nexql-mcp --profile ${PROFILE} doctor

Paste the full output here. If any check fails, tell me exactly what to fix before I ask database questions.`;

const PROMPT_FIRST = `NexQL MCP should now be connected to my Postgres database. Use its MCP tools (not raw SQL unless needed):

1. list_schemas — what schemas exist?
2. search_schema with query "orders customer" — find the relevant tables
3. get_join_path from public.orders to public.customers

Summarize what you found in plain English.`;

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
      ["2", "agent runs npx -y nexql-mcp setup in the terminal", "—"],
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
          code: "npx -y nexql-mcp setup",
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
          code: `nexql-mcp profile add ${PROFILE} --url ${DB_URL}`,
          hint: "Equivalent to the wizard — then skip to Step 3.",
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
      "The wizard merges into your existing MCP config — it won't remove other servers. You see the exact diff before Apply.",
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
          label: "RESULTING CONFIG",
          code: CURSOR_CONFIG,
          hint: "Claude Desktop, VS Code, Windsurf work the same — tick them in the wizard Clients tab.",
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
          code: `npx -y nexql-mcp --profile ${PROFILE} doctor`,
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
      ["list_schemas", "agent lists schemas via MCP", "8ms"],
      ["search_schema", "finds orders + customers in the index", "34ms"],
      ["get_join_path", "walks the FK graph between them", "12ms"],
    ],
    insight: `Start with search_schema — it hits the offline index, not pg_catalog. All ${FACTS.toolCount} tools are available in read mode by default.`,
    figures: [
      ["34ms", "SCHEMA SEARCH"],
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
    alts: [0, 1],
  },
];

const ACTS: [string, number][] = [
  ["GET", 0],
  ["PROFILE", 1],
  ["WIRE", 2],
  ["VERIFY", 3],
  ["LIVE", 4],
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
