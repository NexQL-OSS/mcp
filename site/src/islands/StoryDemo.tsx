import { useCallback, useEffect, useRef, useState } from "preact/hooks";

/**
 * The full-screen narrative demo: a week in the life of one online store's
 * database, told as a replayed agent conversation — notice the drop, narrow
 * it to a page, explain the plan, ship the safe fix, confirm the recovery.
 *
 * Ported from the Claude Design source (`NexQL MCP Story.dc.html`) into a
 * plain Preact island. The design's `sc-for`/`sc-if` template runtime and
 * `style-hover` attributes don't exist outside that editor, so loops became
 * `.map()`, conditionals became `&&`, and hover states moved into the
 * `.story-*` classes in `story.astro`. Everything else — palette, timing,
 * copy, the seven scenes — is unchanged.
 */

type Tone = 0 | 1 | 2;

interface LineMark {
  kind: "line";
  title: string;
  sub: string;
  vals: number[];
  ymax: number;
  tone: "hot" | "mint" | "amber";
  xa: string;
  xb: string;
  vline?: number;
  vlineLabel?: string;
}

interface TableCell {
  text: string;
  tone: Tone;
}
type TableRowSpec = (string | [string, Tone])[];

interface TableMark {
  kind: "table";
  title: string;
  sub: string;
  head: { t: string; al: "left" | "right" }[];
  rows: TableRowSpec[];
}

interface HbarsMark {
  kind: "hbars";
  title: string;
  sub: string;
  max: number;
  items: [string, number, string, Tone][];
}

interface ListMark {
  kind: "list";
  title: string;
  items: ["hot" | "ok" | "info", string, string?][];
}

interface NoteMark {
  kind: "note";
  title: string;
  lines: ["step" | "warn" | "p", string, string?][];
}

type Mark = LineMark | TableMark | HbarsMark | ListMark | NoteMark;

interface Scene {
  act: number;
  role: string;
  where: string;
  prompt: string;
  steps: [string, string, string][];
  insight: string;
  figures: [string, string][];
  mark: Mark;
  alts: [number, number];
}

const AMBER = "#D2A76A";
const MINT = "#77CBA4";
const HOT = "#E0885B";
const NEUT = "rgba(242,240,234,.19)";
const BODY = "400 13px/1.5 'Space Grotesk',system-ui,sans-serif";
const BODY_M = "400 12px/1.5 'JetBrains Mono',monospace";

const HOURLY = [3.9, 3.94, 3.88, 3.91, 3.86, 3.9, 3.72, 3.4, 3.05, 2.71, 2.66, 2.72, 3.1, 3.5, 3.78, 3.86, 3.9, 3.88];
const RECOVER = [2.66, 2.7, 2.68, 2.72, 2.69, 2.74, 3.1, 3.62, 3.94, 4.02, 4.06, 4.04, 4.08, 4.05, 4.07, 4.09, 4.06, 4.08];

const S: Scene[] = [
  {
    act: 0,
    role: "HEAD OF ONLINE STORE",
    where: "Mon 09:12",
    prompt: "Sales were down about 4% last week. Can you tell me why?",
    steps: [
      ["list_schemas", "looked at what this database holds", "11ms"],
      ["run_select", "counted orders for every hour of the week", "184ms"],
      ["run_select", "compared it with the week before", "96ms"],
    ],
    insight:
      "Mornings and afternoons are completely normal. The entire drop happens between 6pm and 10pm — the busiest stretch of the day, when about a third of all orders are placed.",
    figures: [
      ["6–10pm", "WHERE THE DROP IS"],
      ["−31%", "ORDERS IN THAT WINDOW"],
      ["$212K", "SALES MISSED"],
    ],
    mark: { kind: "line", title: "Orders per hour", sub: "averaged over last week", vals: HOURLY, ymax: 4.4, tone: "hot", xa: "6am", xb: "midnight" },
    alts: [3, 6],
  },
  {
    act: 1,
    role: "DATA ANALYST",
    where: "Mon 09:40",
    prompt: "Where in the checkout are people giving up?",
    steps: [
      ["search_schema", "found the tables behind checkout", "9ms"],
      ["get_join_path", "linked website visits to real orders", "17ms"],
      ["run_select", "measured how far each visit got", "240ms"],
    ],
    insight:
      "They get all the way to the payment page and then leave. Of every 100 evening shoppers who open a cart, 76 reach payment but only 57 finish — and the ones who quit had been watching a loading spinner for about nine seconds.",
    figures: [
      ["9 seconds", "AVERAGE WAIT BEFORE QUITTING"],
      ["19 in 100", "LOST ON THE PAYMENT PAGE"],
    ],
    mark: {
      kind: "table",
      title: "Evening checkout, step by step",
      sub: "out of 100 shoppers who open a cart",
      head: [
        { t: "STEP", al: "left" },
        { t: "GET THERE", al: "right" },
        { t: "LEAVE HERE", al: "right" },
      ],
      rows: [
        ["Opens the cart", "100", "—"],
        ["Enters address", "94", "6"],
        ["Reaches payment", "76", "18"],
        ["Completes the order", ["57", 1], ["19", 1]],
      ],
    },
    alts: [2, 0],
  },
  {
    act: 1,
    role: "DATA ANALYST",
    where: "Mon 09:55",
    prompt: "Is the whole site slow in the evening, or just that page?",
    steps: [
      ["db_health_check", "ran 14 general health checks", "140ms"],
      ["slow_queries", "ranked what takes the most time", "61ms"],
      ["table_stats", "checked size and activity per table", "29ms"],
    ],
    insight:
      "Just one thing. The database itself is in good shape — but a single request, the one that adds up the cart total, is using almost half of all the evening capacity. Nothing else has changed.",
    figures: [
      ["1", "SLOW REQUEST"],
      ["46%", "OF EVENING CAPACITY"],
    ],
    mark: {
      kind: "list",
      title: "What came back",
      items: [
        ["hot", "The cart total request is using 46% of the evening", "It adds up items and stock levels for the cart page."],
        ["ok", "Memory, connections and disk are all healthy", "Fourteen checks, no warnings outside this one request."],
        ["ok", "No other request is above 17%", "Payments, prices and logins are unchanged from last month."],
      ],
    },
    alts: [3, 1],
  },
  {
    act: 2,
    role: "ENGINEER",
    where: "Mon 10:20",
    prompt: "Why is that only slow after 6pm?",
    steps: [
      ["explain_query", "asked the database how it runs that request", "74ms"],
      ["table_stats", "looked at the stock table", "26ms"],
      ["sample_values", "sampled tonight’s stock rows", "19ms"],
    ],
    insight:
      "It is working from an eleven-day-old summary of the stock table. That summary says only about 40 rows will match, so the database checks them one at a time. After the evening restock there are 8,100 — and checking those one at a time is what takes nine seconds.",
    figures: [
      ["11 days", "SINCE THE SUMMARY UPDATED"],
      ["200×", "OFF IN ITS ESTIMATE"],
    ],
    mark: {
      kind: "table",
      title: "What it expected vs what is there",
      sub: "stock rows during the evening restock",
      head: [
        { t: "", al: "left" },
        { t: "EXPECTED", al: "right" },
        { t: "ACTUAL", al: "right" },
      ],
      rows: [
        ["Rows to check", "40", ["8,100", 1]],
        ["Time to answer", "30 ms", ["9,000 ms", 1]],
        ["Method chosen", "one at a time", ["needs bulk match", 1]],
      ],
    },
    alts: [4, 2],
  },
  {
    act: 3,
    role: "ENGINEER",
    where: "Mon 10:35",
    prompt: "What is the safest fix we can ship today?",
    steps: [
      ["check_ddl_safety", "tested the fix before running it", "34ms"],
      ["get_ddl", "checked how the table is built", "21ms"],
      ["auto_tune_query", "proposed a faster approach", "52ms"],
    ],
    insight:
      "Two small pieces of work, and one trap to avoid. Refreshing the summary is instant. The shortcut that keeps it fast has to be added in two steps — written the obvious way, in one step, it would freeze every payment for four to seven minutes.",
    figures: [
      ["0.2 sec", "PAUSE, DONE SAFELY"],
      ["4–7 min", "PAUSE, DONE THE OBVIOUS WAY"],
    ],
    mark: {
      kind: "note",
      title: "Plan for tonight",
      lines: [
        ["step", "Refresh the stock summary", "Three seconds, nothing goes offline."],
        ["step", "Add the shortcut in two steps, not one", "Payments pause for under a fifth of a second."],
        ["warn", "Do not run it as a single command", "That version locks payments for four to seven minutes — the table is split into nine parts and all of them freeze."],
        ["p", "Both steps can be undone in seconds if anything looks wrong.", ""],
      ],
    },
    alts: [5, 3],
  },
  {
    act: 3,
    role: "ENGINEER",
    where: "Mon 19:04",
    prompt: "It is live. Did it work?",
    steps: [
      ["explain_query", "confirmed the new approach is being used", "68ms"],
      ["slow_queries", "compared tonight with last night", "58ms"],
      ["db_health_check", "14 checks, no warnings", "132ms"],
    ],
    insight: "Yes. The cart total now comes back in 31 milliseconds instead of 812 — about 26 times faster — and no single request is taking more than 9% of the evening.",
    figures: [
      ["26× faster", "CART TOTAL"],
      ["0", "WARNINGS LEFT"],
    ],
    mark: {
      kind: "hbars",
      title: "Time to load a cart total",
      sub: "typical evening request",
      max: 812,
      items: [
        ["Last night", 812, "812 ms", 1],
        ["Tonight", 31, "31 ms", 2],
      ],
    },
    alts: [6, 4],
  },
  {
    act: 4,
    role: "HEAD OF ONLINE STORE",
    where: "Tue 09:10",
    prompt: "Are sales back to normal?",
    steps: [
      ["run_select", "counted last night’s orders per hour", "176ms"],
      ["run_select", "compared with the same night last week", "104ms"],
      ["run_select", "checked carts abandoned at payment", "88ms"],
    ],
    insight:
      "Better than normal. Last night’s evening orders came in slightly ahead of the daytime rate, and the payment page lost 2 shoppers in 100 instead of 19. That is $28.4K of sales recovered in a single night.",
    figures: [
      ["+4.1%", "ORDERS VS LAST WEEK"],
      ["$28.4K", "RECOVERED, FIRST NIGHT"],
      ["1 day", "QUESTION TO FIX"],
    ],
    mark: {
      kind: "line",
      title: "Orders per hour, evening",
      sub: "last night compared with the week before",
      vals: RECOVER,
      ymax: 4.4,
      tone: "mint",
      xa: "6pm last week",
      xb: "10pm last night",
      vline: 0.34,
      vlineLabel: "fix went live",
    },
    alts: [0, 3],
  },
];

const ACTS: [string, number][] = [
  ["NOTICED", 0],
  ["NARROWED", 1],
  ["EXPLAINED", 3],
  ["FIXED", 4],
  ["RECOVERED", 6],
];

/**
 * Client configs NexQL can write to directly (`nexql-mcp setup`'s
 * mergeable targets — crates/nexql-mcp/src/client_targets.rs). `shape`
 * mirrors that file's `ConfigShape`: which top-level key the server entry
 * lives under, and whether it needs VS Code's `"type": "stdio"`.
 */
type ClientShape = "claude" | "vscode";

interface NpxClient {
  key: string;
  label: string;
  file: string;
  shape: ClientShape;
}

const NPX_CLIENTS: NpxClient[] = [
  { key: "claude-desktop", label: "Claude Desktop", file: "claude_desktop_config.json", shape: "claude" },
  { key: "claude-code", label: "Claude Code", file: ".mcp.json", shape: "claude" },
  { key: "cursor", label: "Cursor", file: ".cursor/mcp.json", shape: "claude" },
  { key: "vscode", label: "VS Code · GitHub Copilot", file: ".vscode/mcp.json", shape: "vscode" },
  { key: "antigravity", label: "Antigravity", file: "~/.gemini/antigravity-ide/mcp_config.json", shape: "claude" },
  { key: "windsurf", label: "Windsurf", file: "~/.codeium/windsurf/mcp_config.json", shape: "claude" },
];

/** Placeholder profile name, not a connection string — `--profile <name>`
 * is how `LaunchConfig::command_and_args` (setup/wire.rs) points every
 * client at a stored profile instead of baking a connection string into
 * each client's config file. Create it once (`nexql-mcp setup`, or
 * `nexql-mcp profile add mydb --url postgres://…`); every client below just
 * references it by name, so swapping databases means editing one profile,
 * not N client configs. */
const PROFILE_PLACEHOLDER = "mydb";

/** The exact shape `client_targets::ConfigShape::entry()` writes. */
function npxSnippet(shape: ClientShape): string {
  const entry = `{
      "command": "npx",
      "args": ["-y", "nexql-mcp", "--profile", "${PROFILE_PLACEHOLDER}"]${shape === "vscode" ? ',\n      "type": "stdio"' : ""}
    }`;
  return shape === "vscode"
    ? `{\n  "servers": {\n    "nexql-mcp": ${entry}\n  }\n}`
    : `{\n  "mcpServers": {\n    "nexql-mcp": ${entry}\n  }\n}`;
}

/** Any other MCP-speaking client (Codex, opencode, Continue, …) — not one of
 * NexQL's built-in merge targets, but every MCP client takes the same
 * command/args pair in whatever config format it uses. */
const GENERIC_SNIPPET = `{
  "command": "npx",
  "args": ["-y", "nexql-mcp", "--profile", "${PROFILE_PLACEHOLDER}"]
}`;

const LINKS = [
  { name: "Repository", href: "https://github.com/NexQL-OSS/mcp", note: "Rust workspace · 6 crates · GPL-3.0-only" },
  { name: "Tool reference", href: "https://github.com/NexQL-OSS/mcp/blob/main/docs/REFERENCE.md", note: "All 54 tools, arguments and access modes" },
  { name: "Client setup", href: "https://github.com/NexQL-OSS/mcp/tree/main/docs/clients", note: "Config paths and paste blocks per client" },
  { name: "Quickstarts", href: "https://github.com/NexQL-OSS/mcp/tree/main/docs", note: "Developer · DBA · analyst · PM tracks" },
  { name: "MCP registry", href: "https://registry.modelcontextprotocol.io/servers/io.github.NexQL-OSS/nexql-mcp", note: "io.github.NexQL-OSS/nexql-mcp" },
];

function linePath(vals: number[], ymax: number): string {
  const n = vals.length;
  const lo = Math.min(...vals) * 0.82;
  return vals
    .map((v, i) => {
      const x = (i / (n - 1)) * 600;
      const y = 140 - Math.min(1, (v - lo) / (ymax - lo)) * 132;
      return x.toFixed(1) + "," + y.toFixed(1);
    })
    .join(" ");
}

const toneFill = (t: Tone) => (t === 1 ? HOT : t === 2 ? MINT : NEUT);
const toneText = (t: Tone) => (t === 1 ? HOT : t === 2 ? MINT : "#8E949E");
const roleHue = (r: string) => (r === "HEAD OF ONLINE STORE" ? AMBER : r === "DATA ANALYST" ? "#93A9E6" : MINT);

function tableCell(c: string | [string, Tone], head: { al: "left" | "right" } | undefined, colIndex: number): TableCell & { al: "left" | "right"; font: string } {
  const isArr = Array.isArray(c);
  const text = isArr ? c[0] : c;
  const tone: Tone = isArr ? c[1] : 0;
  return {
    text,
    tone,
    al: head?.al ?? "left",
    font: colIndex === 0 ? BODY : BODY_M,
  };
}

interface TurnView {
  op: string;
  role: string;
  where: string;
  roleColor: string;
  userLine: string;
  userFill: string;
  userText: string;
  caretOn: boolean;
  agentOn: boolean;
  bubbleW: string;
  phase: string;
  steps: { tool: string; arg: string; dot: string }[];
  insightOn: boolean;
  insight: string;
  markOn: boolean;
  hasMarkTitle: boolean;
  markTitle: string;
  markSub: string;
  hasFigures: boolean;
  figures: { v: string; l: string; color: string }[];
  mark: Mark;
  draw: number;
  optStart: number;
}

function buildTurn(n: number, live: boolean, t: number, cur: boolean): TurnView {
  const sc = S[n];
  const pl = sc.prompt.length;
  const typeMs = Math.min(2600, 300 + pl * 22);
  const toolsStart = typeMs + 260;
  const stepGap = 360;
  const insightStart = toolsStart + sc.steps.length * stepGap + 340;
  const markStart = insightStart + 800;
  const hue = roleHue(sc.role);

  const typedLen = live ? Math.round(Math.min(1, t / typeMs) * pl) : pl;
  const shown = live ? (t >= toolsStart ? Math.min(sc.steps.length, Math.floor((t - toolsStart) / stepGap) + 1) : 0) : sc.steps.length;
  const insightOn = !live || t >= insightStart;
  const markOn = !live || t >= markStart;
  const draw = live ? Math.min(1, Math.max(0, (t - markStart) / 900)) : 1;

  const steps = sc.steps.slice(0, shown).map((s) => {
    const done = !live || shown >= sc.steps.length ? t >= toolsStart + sc.steps.length * stepGap || !live : false;
    return { tool: s[0], arg: s[1], dot: done ? "#4A505A" : hue };
  });

  return {
    op: cur ? "1" : ".42",
    role: sc.role,
    where: sc.where,
    roleColor: hue,
    userLine: cur ? "#2E343D" : "#1E222A",
    userFill: cur ? "rgba(255,255,255,.045)" : "rgba(255,255,255,.022)",
    userText: sc.prompt.slice(0, typedLen),
    caretOn: live && typedLen < pl,
    agentOn: shown > 0 || !live,
    bubbleW: markOn ? "100%" : "auto",
    phase: !cur
      ? ""
      : !live
        ? "answered from the live database"
        : shown < sc.steps.length
          ? shown + " of " + sc.steps.length + " checks"
          : !insightOn
            ? "writing the answer"
            : "answered from the live database",
    steps,
    insightOn,
    insight: sc.insight,
    markOn,
    hasMarkTitle: !!sc.mark.title,
    markTitle: sc.mark.title || "",
    markSub: "sub" in sc.mark ? sc.mark.sub || "" : "",
    hasFigures: sc.figures.length > 0,
    figures: sc.figures.map((f, k) => ({ v: f[0], l: f[1], color: k === 0 ? hue : "#E4E1D9" })),
    mark: sc.mark,
    draw,
    optStart: markStart + 1000,
  };
}

interface StoryState {
  i: number;
  t: number;
  playing: boolean;
  installOpen: boolean;
  /** Selected tab in the install modal's npx client picker — a key from NPX_CLIENTS. */
  installClient: string;
  vw: number;
}

interface StoryDemoProps {
  autoplay?: boolean;
  sceneSeconds?: number;
  startScene?: number;
  /** Any CSS height value. Defaults to the full-viewport /story page size;
   *  pass a fixed value (e.g. "560px") to embed the player in a bounded box
   *  like the homepage hero. */
  height?: string;
}

function prefersReducedMotion(): boolean {
  return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function StoryDemo({ autoplay = true, sceneSeconds = 25, startScene = 1, height = "100dvh" }: StoryDemoProps) {
  const dur = Math.max(8, sceneSeconds) * 1000;
  const initialI = Math.max(1, Math.min(S.length, startScene)) - 1;

  // Reduced motion: don't force the typing/reveal animation or the
  // auto-advance clock on someone who asked the OS to turn that off. The
  // scene still renders fully typed/revealed (buildTurn's `live: false`
  // path) and stays put until they navigate manually.
  const [state, setState] = useState<StoryState>(() => ({
    i: initialI,
    t: 0,
    playing: autoplay && !prefersReducedMotion(),
    installOpen: false,
    installClient: NPX_CLIENTS[0].key,
    vw: typeof window !== "undefined" ? window.innerWidth : 1440,
  }));
  const stateRef = useRef(state);
  stateRef.current = state;
  const prevI = useRef(state.i);

  const scrollRef = useRef<HTMLDivElement>(null);

  const patch = useCallback((p: Partial<StoryState>) => {
    setState((s) => ({ ...s, ...p }));
  }, []);

  const targets = useCallback((i: number): number[] => {
    const sc = S[i];
    const out = [(i + 1) % S.length];
    for (const a of sc.alts) {
      if (a !== i && out.indexOf(a) < 0) out.push(a);
    }
    for (let k = 1; out.length < 3 && k < S.length; k++) {
      const n = (i + 1 + k) % S.length;
      if (n !== i && out.indexOf(n) < 0) out.push(n);
    }
    return out;
  }, []);

  const go = useCallback((n: number) => {
    const L = S.length;
    patch({ i: ((n % L) + L) % L, t: 0 });
  }, [patch]);

  const pinBottom = useCallback(() => {
    const put = () => {
      const el = scrollRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    };
    put();
    requestAnimationFrame(() => {
      put();
      requestAnimationFrame(put);
    });
  }, []);

  // Mount-once: resize, keyboard, and the 80ms scene clock.
  useEffect(() => {
    const onResize = () => patch({ vw: window.innerWidth });
    window.addEventListener("resize", onResize);

    const onKey = (e: KeyboardEvent) => {
      const s = stateRef.current;
      if (e.key === "ArrowRight") {
        e.preventDefault();
        go(s.i + 1);
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        go(s.i - 1);
      } else if (e.key === " ") {
        e.preventDefault();
        patch({ playing: !s.playing });
      } else if (e.key === "Escape") {
        patch({ installOpen: false });
      } else if (e.key === "1" || e.key === "2" || e.key === "3") {
        const opt = targets(s.i)[Number(e.key) - 1];
        if (opt !== undefined) go(opt);
      }
    };
    window.addEventListener("keydown", onKey);

    pinBottom();

    const timer = window.setInterval(() => {
      const s = stateRef.current;
      if (!s.playing || s.installOpen) return;
      if (s.t + 80 >= dur) {
        go(targets(s.i)[0]);
      } else {
        patch({ t: s.t + 80 });
      }
    }, 80);

    return () => {
      window.removeEventListener("resize", onResize);
      window.removeEventListener("keydown", onKey);
      window.clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- mount-once, reads latest state via stateRef
  }, []);

  // Keep the transcript pinned to the bottom while playing or on scene change.
  useEffect(() => {
    if (state.playing || prevI.current !== state.i) pinBottom();
    prevI.current = state.i;
  });

  const { i, t, playing, installOpen, installClient, vw } = state;
  const sc = S[i];
  const hue = roleHue(sc.role);
  const live = playing;

  const turns: TurnView[] = [];
  for (let n = Math.max(0, i - 2); n < i; n++) turns.push(buildTurn(n, false, 0, false));
  const cur = buildTurn(i, live, t, true);
  turns.push(cur);

  const tgs = targets(i);
  const optionsOn = !live || t >= cur.optStart;
  const options = tgs.map((n, k) => ({
    n,
    label: String(k + 1),
    text: S[n].prompt,
    isNext: k === 0,
    prog: ((t / dur) * 100).toFixed(1) + "%",
    color: k === 0 ? "#F2F0EA" : "rgba(242,240,234,.5)",
    numColor: k === 0 ? hue : "#4E545F",
    line: k === 0 ? "#3A404A" : "#1E222A",
    fill: k === 0 ? "rgba(255,255,255,.04)" : "transparent",
  }));

  const wide = vw >= 1000;
  const glow = i >= 5 ? "rgba(119,203,164,.07)" : "rgba(210,167,106,.07)";
  const counter = String(i + 1).padStart(2, "0") + " / " + String(S.length).padStart(2, "0");
  const progress = ((t / dur) * 100).toFixed(2) + "%";

  return (
    <div
      class="story-stage"
      style={`position:relative;width:100%;height:${height};min-height:${height};max-height:${height};overflow:hidden;background:#0A0B0D;color:#F2F0EA;display:grid;grid-template-columns:minmax(0,1fr);grid-template-rows:auto minmax(0,1fr) auto`}
    >
      <div aria-hidden="true" style={`position:absolute;inset:0;pointer-events:none;z-index:0;background:radial-gradient(66% 48% at 50% 8%, ${glow}, transparent 72%)`}></div>

      <header style="position:relative;z-index:5;display:flex;align-items:center;gap:clamp(10px,2vw,26px);padding:11px clamp(14px,2.6vw,34px);border-bottom:1px solid #15181D;background:rgba(10,11,13,.72);backdrop-filter:blur(14px)">
        <div style="display:flex;align-items:center;gap:9px;flex:none">
          <img src="/assets/NexQL.png" alt="NexQL" width={22} height={22} style="width:22px;height:22px;border-radius:5px;display:block" />
          <span style="font:600 14px/1 'Space Grotesk',system-ui,sans-serif;letter-spacing:-.01em">NexQL</span>
          <span style="padding:3px 6px;border-radius:4px;border:1px solid #2A2F37;font:500 8.5px/1 'JetBrains Mono',monospace;letter-spacing:.16em;color:#8B929E">MCP</span>
        </div>

        <div style="display:flex;align-items:center;gap:clamp(6px,1.1vw,14px);min-width:0;overflow:hidden;flex:1 1 auto">
          {ACTS.map((a, k) => {
            const on = sc.act === k;
            return (
              <button
                key={a[0]}
                type="button"
                onClick={() => go(a[1])}
                style="display:flex;align-items:center;gap:7px;padding:5px 2px;flex:none"
              >
                <span style={`width:${on ? "22px" : "9px"};height:2px;background:${on ? hue : "#2A2F37"};flex:none;transition:width .45s ease,background .45s ease`}></span>
                <span style={`font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.13em;color:${on ? "#F2F0EA" : "#4E545F"};white-space:nowrap;transition:color .45s ease`}>{a[0]}</span>
              </button>
            );
          })}
        </div>

        <nav style="display:flex;align-items:center;gap:clamp(8px,1.4vw,18px);flex:none">
          {wide && (
            <a class="story-navlink" href="https://github.com/NexQL-OSS/mcp/blob/main/docs/REFERENCE.md" target="_blank" rel="noreferrer" style="font:400 11px/1 'Space Grotesk',system-ui,sans-serif;color:#8B929E">
              Docs
            </a>
          )}
          {wide && (
            <a class="story-navlink" href="https://github.com/NexQL-OSS/mcp/tree/main/docs/clients" target="_blank" rel="noreferrer" style="font:400 11px/1 'Space Grotesk',system-ui,sans-serif;color:#8B929E">
              Clients
            </a>
          )}
          <a class="story-navlink" href="https://github.com/NexQL-OSS/mcp" target="_blank" rel="noreferrer" style="font:400 11px/1 'Space Grotesk',system-ui,sans-serif;color:#8B929E">
            GitHub
          </a>
          <button type="button" class="story-install-btn" onClick={() => patch({ installOpen: true })} style="padding:7px 12px;border-radius:7px;border:1px solid #33383F;font:500 11px/1 'Space Grotesk',system-ui,sans-serif;color:#F2F0EA">
            Install
          </button>
        </nav>
      </header>

      <main ref={scrollRef} style="position:relative;z-index:2;min-height:0;overflow-y:auto;overflow-x:hidden;padding:clamp(16px,3vh,34px) clamp(14px,3vw,40px) 8px">
        <div style="max-width:880px;margin:0 auto;display:flex;flex-direction:column;gap:clamp(18px,2.8vh,30px)">
          {turns.map((tn, idx) => (
            <div key={idx} style={`display:flex;flex-direction:column;gap:11px;opacity:${tn.op};transition:opacity .5s ease`}>
              <div style="display:flex;flex-direction:column;align-items:flex-end;gap:6px">
                <div style="display:flex;align-items:center;gap:8px">
                  <span style="font:400 9px/1 'JetBrains Mono',monospace;letter-spacing:.1em;color:#4E545F;white-space:nowrap">{tn.where}</span>
                  <span style={`font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:${tn.roleColor};white-space:nowrap`}>{tn.role}</span>
                  <span style={`width:6px;height:6px;border-radius:50%;background:${tn.roleColor};flex:none`}></span>
                </div>
                <div style={`max-width:min(58ch,88%);padding:12px 16px;border-radius:14px 14px 4px 14px;border:1px solid ${tn.userLine};background:${tn.userFill}`}>
                  <span style="font:400 clamp(15px,1.22vw,19px)/1.45 'Space Grotesk',system-ui,sans-serif;color:#F8F6F1;text-wrap:pretty">{tn.userText}</span>
                  {tn.caretOn && (
                    <span style={`display:inline-block;width:2px;height:1em;margin-left:4px;vertical-align:-.14em;background:${tn.roleColor};animation:stBlink 1s step-end infinite`}></span>
                  )}
                </div>
              </div>

              {tn.agentOn && (
                <div style="display:flex;flex-direction:column;align-items:flex-start;gap:6px">
                  <div style="display:flex;align-items:center;gap:8px">
                    <span style="width:6px;height:6px;border-radius:1px;background:#5A616C;flex:none"></span>
                    <span style="font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#8B929E">CLAUDE + NEXQL</span>
                    <span style="font:400 9px/1 'JetBrains Mono',monospace;color:#3E444E;white-space:nowrap">{tn.phase}</span>
                  </div>

                  <div style={`max-width:min(70ch,96%);width:${tn.bubbleW};padding:14px 16px;border-radius:14px 14px 14px 4px;border:1px solid #191D23;background:rgba(255,255,255,.028);display:flex;flex-direction:column;gap:13px`}>
                    <div style="display:flex;flex-direction:column;gap:6px">
                      <span style="font:500 8.5px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#4A505A">CHECKED THE DATABASE</span>
                      {tn.steps.map((s, sk) => (
                        <div key={sk} style="display:flex;align-items:center;gap:9px;animation:stFade .4s ease both">
                          <span style={`width:4px;height:4px;border-radius:50%;background:${s.dot};flex:none`}></span>
                          <span style="font:400 11.5px/1.4 'Space Grotesk',system-ui,sans-serif;color:#8E949E;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{s.arg}</span>
                          <span style="flex:1 1 auto;min-width:10px;height:1px;background:repeating-linear-gradient(90deg,#23272E 0 4px,transparent 4px 9px)"></span>
                          <span style="font:400 9px/1 'JetBrains Mono',monospace;color:#4A505A;flex:none">{s.tool}</span>
                        </div>
                      ))}
                    </div>

                    {tn.insightOn && (
                      <p style="margin:0;font:400 clamp(14.5px,1.12vw,17.5px)/1.62 'Space Grotesk',system-ui,sans-serif;color:rgba(242,240,234,.9);text-wrap:pretty;animation:stRise .5s ease both">{tn.insight}</p>
                    )}

                    {tn.markOn && (
                      <div style="display:flex;flex-direction:column;gap:12px;padding-top:12px;border-top:1px solid #191D23;animation:stFade .5s ease both">
                        {tn.hasFigures && (
                          <div style="display:flex;flex-wrap:wrap;gap:clamp(14px,2.4vw,34px)">
                            {tn.figures.map((f, fk) => (
                              <div key={fk} style="display:flex;flex-direction:column;gap:4px">
                                <span style={`font:400 clamp(17px,1.6vw,24px)/1 'Instrument Serif',serif;color:${f.color}`}>{f.v}</span>
                                <span style="font:400 8.5px/1 'JetBrains Mono',monospace;letter-spacing:.11em;color:#5A616C;white-space:nowrap">{f.l}</span>
                              </div>
                            ))}
                          </div>
                        )}

                        {tn.hasMarkTitle && (
                          <div style="display:flex;align-items:baseline;gap:9px">
                            <span style="font:500 8.5px/1 'JetBrains Mono',monospace;letter-spacing:.13em;color:#7E8694;white-space:nowrap">{tn.markTitle}</span>
                            <span style="font:400 8.5px/1 'JetBrains Mono',monospace;color:#4A505A;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{tn.markSub}</span>
                          </div>
                        )}

                        {tn.mark.kind === "line" && (
                          <div style="position:relative;height:clamp(76px,13vh,124px);margin-bottom:14px">
                            <span style="position:absolute;left:0;right:0;bottom:0;height:1px;background:#20242B"></span>
                            <span style="position:absolute;left:0;right:0;top:50%;height:1px;background:#161A1F"></span>
                            <svg viewBox="0 0 600 140" preserveAspectRatio="none" width="100%" height="100%" style="display:block;overflow:visible">
                              <polyline
                                points={linePath(tn.mark.vals, tn.mark.ymax)}
                                fill="none"
                                stroke={tn.mark.tone === "mint" ? MINT : tn.mark.tone === "hot" ? HOT : AMBER}
                                stroke-width="2"
                                stroke-linejoin="round"
                                stroke-dasharray="1900"
                                stroke-dashoffset={(1900 * (1 - tn.draw)).toFixed(0)}
                              ></polyline>
                            </svg>
                            {tn.mark.vline != null && (
                              <span style={`position:absolute;top:0;bottom:0;left:${(tn.mark.vline * 100).toFixed(1)}%;width:1px;background:#77CBA4;opacity:.45`}></span>
                            )}
                            {tn.mark.vline != null && (
                              <span style={`position:absolute;top:-3px;left:${(tn.mark.vline * 100).toFixed(1)}%;transform:translateX(6px);font:400 8.5px/1 'JetBrains Mono',monospace;color:#77CBA4;white-space:nowrap`}>
                                {tn.mark.vlineLabel}
                              </span>
                            )}
                            <span style="position:absolute;left:0;bottom:-15px;font:400 8.5px/1 'JetBrains Mono',monospace;color:#4A505A">{tn.mark.xa}</span>
                            <span style="position:absolute;right:0;bottom:-15px;font:400 8.5px/1 'JetBrains Mono',monospace;color:#4A505A">{tn.mark.xb}</span>
                          </div>
                        )}

                        {tn.mark.kind === "hbars" && (
                          <div style="display:flex;flex-direction:column;gap:10px">
                            {(() => {
                              const m = tn.mark as HbarsMark;
                              return m.items.map((b, bk) => {
                                const w = Math.max(1.2, (b[1] / m.max) * 100 * tn.draw).toFixed(1) + "%";
                                return (
                                  <div key={bk} style="display:flex;flex-direction:column;gap:5px;min-width:0">
                                    <div style="display:flex;align-items:baseline;gap:12px">
                                      <span style="font:400 12px/1.35 'Space Grotesk',system-ui,sans-serif;color:#B6B3AB;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0">{b[0]}</span>
                                      <span style="flex:1"></span>
                                      <span style={`font:400 11px/1 'JetBrains Mono',monospace;color:${toneText(b[3])};flex:none`}>{b[2]}</span>
                                    </div>
                                    <span style={`height:5px;border-radius:3px;width:${w};background:${toneFill(b[3])};transition:width .5s ease`}></span>
                                  </div>
                                );
                              });
                            })()}
                          </div>
                        )}

                        {tn.mark.kind === "table" && (
                          <div style="display:flex;flex-direction:column;gap:0">
                            <div style="display:grid;grid-template-columns:minmax(0,1.7fr) minmax(0,1fr) minmax(0,1fr);gap:10px;padding:0 0 7px;border-bottom:1px solid #23272E">
                              {tn.mark.head.map((h, hk) => (
                                <span key={hk} style={`font:500 8.5px/1.2 'JetBrains Mono',monospace;letter-spacing:.12em;color:#5A616C;text-align:${h.al}`}>{h.t}</span>
                              ))}
                            </div>
                            {tn.mark.rows.map((r, rk) => (
                              <div key={rk} style="display:grid;grid-template-columns:minmax(0,1.7fr) minmax(0,1fr) minmax(0,1fr);gap:10px;padding:8px 0;border-bottom:1px solid #15181D">
                                {r.map((c, ck) => {
                                  const cell = tableCell(c, (tn.mark as TableMark).head[ck], ck);
                                  return (
                                    <span
                                      key={ck}
                                      style={`font:${cell.font};color:${cell.tone ? toneText(cell.tone) : ck === 0 ? "#E4E1D9" : "#9C9A93"};text-align:${cell.al};min-width:0;overflow:hidden;text-overflow:ellipsis`}
                                    >
                                      {cell.text}
                                    </span>
                                  );
                                })}
                              </div>
                            ))}
                          </div>
                        )}

                        {tn.mark.kind === "list" && (
                          <div style="display:flex;flex-direction:column;gap:9px">
                            {tn.mark.items.map((it, ik) => {
                              const glyph = it[0] === "hot" ? "▲" : it[0] === "ok" ? "✓" : "·";
                              const color = it[0] === "hot" ? HOT : it[0] === "ok" ? MINT : "#8E949E";
                              return (
                                <div key={ik} style="display:flex;align-items:flex-start;gap:10px">
                                  <span style={`flex:none;margin-top:3px;font:500 10px/1.2 'JetBrains Mono',monospace;color:${color}`}>{glyph}</span>
                                  <div style="display:flex;flex-direction:column;gap:2px;min-width:0">
                                    <span style="font:400 13.5px/1.45 'Space Grotesk',system-ui,sans-serif;color:#E4E1D9;text-wrap:pretty">{it[1]}</span>
                                    {it[2] && <span style="font:400 11.5px/1.45 'Space Grotesk',system-ui,sans-serif;color:#6E7480;text-wrap:pretty">{it[2]}</span>}
                                  </div>
                                </div>
                              );
                            })}
                          </div>
                        )}

                        {tn.mark.kind === "note" && (
                          <div style="display:flex;flex-direction:column;gap:10px">
                            {tn.mark.lines.map((nl, nk) => {
                              const isStep = nl[0] === "step";
                              const isWarn = nl[0] === "warn";
                              const lead = isStep ? String(nk + 1) : isWarn ? "▲" : "";
                              const leadFont = isWarn ? "500 10px/1.6 'JetBrains Mono',monospace" : "400 11px/1.5 'JetBrains Mono',monospace";
                              const leadColor = isWarn ? HOT : "#5A616C";
                              const font = isWarn ? "500 13px/1.5 'Space Grotesk',system-ui,sans-serif" : "400 13.5px/1.5 'Space Grotesk',system-ui,sans-serif";
                              const color = isWarn ? HOT : "#E4E1D9";
                              const pad = isWarn ? "10px 12px" : "0";
                              const rule = isWarn ? `2px solid ${HOT}` : "0";
                              const fill = isWarn ? "rgba(224,136,91,.07)" : "transparent";
                              const radius = isWarn ? "3px 8px 8px 3px" : "0";
                              return (
                                <div key={nk} style={`display:flex;align-items:flex-start;gap:11px;padding:${pad};border-left:${rule};background:${fill};border-radius:${radius}`}>
                                  <span style={`flex:none;font:${leadFont};color:${leadColor};min-width:13px`}>{lead}</span>
                                  <div style="display:flex;flex-direction:column;gap:2px;min-width:0">
                                    <span style={`font:${font};color:${color};text-wrap:pretty`}>{nl[1]}</span>
                                    {nl[2] && <span style="font:400 11.5px/1.45 'Space Grotesk',system-ui,sans-serif;color:#6E7480;text-wrap:pretty">{nl[2]}</span>}
                                  </div>
                                </div>
                              );
                            })}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          ))}
          <div style="height:2px"></div>
        </div>
      </main>

      <footer style="position:relative;z-index:3;padding:clamp(8px,1.4vh,14px) clamp(14px,3vw,40px) clamp(12px,2.2vh,20px);border-top:1px solid #14171C;background:rgba(10,11,13,.86);backdrop-filter:blur(12px)">
        <div style="max-width:880px;margin:0 auto;display:flex;flex-direction:column;gap:clamp(8px,1.4vh,14px)">
          {optionsOn && (
            <div style="display:flex;flex-direction:column;align-items:flex-end;gap:5px;animation:stRise .4s ease both">
              {options.map((o) => (
                <button
                  key={o.n}
                  type="button"
                  class="story-option"
                  onClick={() => go(o.n)}
                  style={`position:relative;overflow:hidden;display:flex;align-items:center;gap:11px;max-width:min(58ch,94%);padding:8px 14px;border-radius:12px 12px 4px 12px;border:1px solid ${o.line};background:${o.fill};text-align:left`}
                >
                  <span style={`font:400 9px/1 'JetBrains Mono',monospace;color:${o.numColor};flex:none`}>{o.label}</span>
                  <span style={`font:400 clamp(12.5px,1.02vw,14.5px)/1.35 'Space Grotesk',system-ui,sans-serif;color:${o.color};min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap`}>{o.text}</span>
                  {o.isNext && <span style={`position:absolute;left:0;bottom:0;height:1px;width:${o.prog};background:${hue}`}></span>}
                </button>
              ))}
            </div>
          )}

          <div style="display:flex;align-items:center;gap:clamp(10px,1.6vw,20px);flex-wrap:wrap">
            <button type="button" onClick={() => patch({ playing: !playing })} style="font:400 9.5px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#8B929E;padding:0">
              {playing ? "❚❚ PAUSE" : "▶ PLAY"}
            </button>
            <span style="font:400 9.5px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#4E545F">{counter}</span>
            <span style="font:400 9.5px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#4E545F">one online store&rsquo;s database · read-only</span>
            <span style="flex:1 1 auto;min-width:30px;height:1px;background:#171A20;position:relative">
              <span style={`position:absolute;left:0;top:0;height:1px;width:${progress};background:${hue}`}></span>
            </span>
            <span style="font:400 9.5px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#4E545F">1 2 3 · ← → · SPACE</span>
          </div>
        </div>
      </footer>

      {installOpen && (
        <div
          style="position:absolute;inset:0;z-index:20;background:rgba(6,7,8,.86);backdrop-filter:blur(8px);display:flex;align-items:center;justify-content:center;padding:clamp(16px,4vw,50px);animation:stFade .25s ease both"
          onClick={() => patch({ installOpen: false })}
        >
          <div
            style="width:min(660px,100%);max-height:100%;overflow-y:auto;border:1px solid #22262D;border-radius:14px;background:#0D0F12;padding:clamp(18px,2.4vw,30px);display:flex;flex-direction:column;gap:18px"
            onClick={(e) => e.stopPropagation()}
          >
            <div style="display:flex;align-items:center;gap:12px">
              <span style="font:400 22px/1.1 'Instrument Serif',serif;color:#F8F6F1">Add NexQL to your agent</span>
              <button type="button" class="story-close" onClick={() => patch({ installOpen: false })} style="margin-left:auto;font:400 10px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#8B929E">
                CLOSE ✕
              </button>
            </div>

            {/* New: the setup wizard — one command, opens a local browser UI that
                picks clients, previews the config diff, and writes every file. */}
            <div style="display:flex;flex-direction:column;gap:8px;padding:14px 16px;border:1px solid rgba(210,167,106,.35);border-radius:10px;background:rgba(210,167,106,.06)">
              <div style="display:flex;align-items:baseline;gap:8px;flex-wrap:wrap">
                <span style="font:500 8.5px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#D2A76A">NEW</span>
                <span style="font:500 13px/1.3 'Space Grotesk',system-ui,sans-serif;color:#F8F6F1">Setup wizard — no manual JSON</span>
              </div>
              <pre style="margin:0;padding:11px 13px;border:1px solid #1E222A;border-radius:8px;background:#0A0B0D;font:400 11.5px/1.6 'JetBrains Mono',monospace;color:#D2A76A;white-space:pre-wrap;word-break:break-all">npx -y nexql-mcp setup</pre>
              <p style="margin:0;font:400 11.5px/1.5 'Space Grotesk',system-ui,sans-serif;color:#8E949E">
                Opens a local page in your browser: pick which clients to wire up, preview the exact config diff per file, then apply. Nothing leaves your machine.
              </p>
            </div>

            <div style="display:flex;flex-direction:column;gap:8px">
              <span style="font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#8B929E">RECOMMENDED — RUN VIA NPX, NO INSTALL</span>
              <p style="margin:0;font:400 11px/1.5 'Space Grotesk',system-ui,sans-serif;color:#6E7480">
                <code style="color:#8E949E">--profile mydb</code> points at a stored profile, not a hardcoded connection —
                create it in the setup wizard above, or <code style="color:#8E949E">nexql-mcp profile add mydb --url postgres://…</code>.
                Every client below just references it by name, so switching databases means editing one profile, not every client config.
              </p>
              <div style="display:flex;flex-wrap:wrap;gap:6px">
                {NPX_CLIENTS.map((c) => (
                  <button
                    key={c.key}
                    type="button"
                    class="story-tab"
                    aria-pressed={c.key === installClient ? "true" : "false"}
                    onClick={() => patch({ installClient: c.key })}
                    style={`padding:6px 11px;border-radius:7px;border:1px solid ${c.key === installClient ? "#D2A76A" : "#22262D"};background:${c.key === installClient ? "rgba(210,167,106,.1)" : "transparent"};font:400 11px/1 'Space Grotesk',system-ui,sans-serif;color:${c.key === installClient ? "#F2F0EA" : "#8B929E"}`}
                  >
                    {c.label}
                  </button>
                ))}
              </div>
              {(() => {
                const c = NPX_CLIENTS.find((x) => x.key === installClient) ?? NPX_CLIENTS[0];
                return (
                  <div style="display:flex;flex-direction:column;gap:6px">
                    <span style="font:400 10px/1.4 'JetBrains Mono',monospace;color:#5A616C">→ merge into {c.file}</span>
                    <pre style="margin:0;padding:11px 13px;border:1px solid #1E222A;border-radius:8px;background:#0A0B0D;font:400 11px/1.6 'JetBrains Mono',monospace;color:#D2A76A;white-space:pre-wrap;word-break:break-all">{npxSnippet(c.shape)}</pre>
                  </div>
                );
              })()}
              <details style="margin-top:2px">
                <summary style="cursor:pointer;font:400 10.5px/1.4 'JetBrains Mono',monospace;color:#5A616C">Codex, opencode, or any other MCP client</summary>
                <div style="display:flex;flex-direction:column;gap:6px;margin-top:8px">
                  <span style="font:400 11px/1.5 'Space Grotesk',system-ui,sans-serif;color:#8E949E">Not one of NexQL's built-in targets yet — every MCP client takes the same command/args pair in its own config format:</span>
                  <pre style="margin:0;padding:11px 13px;border:1px solid #1E222A;border-radius:8px;background:#0A0B0D;font:400 11px/1.6 'JetBrains Mono',monospace;color:#D2A76A;white-space:pre-wrap;word-break:break-all">{GENERIC_SNIPPET}</pre>
                </div>
              </details>
            </div>

            <div style="display:flex;flex-direction:column;gap:8px;padding-top:4px;border-top:1px solid #1A1E24">
              <span style="font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#8B929E">SECONDARY — INSTALL THE BINARY</span>
              <div style="display:flex;flex-direction:column;gap:6px">
                <span style="font:400 10px/1.4 'JetBrains Mono',monospace;color:#5A616C">macOS · Linux</span>
                <pre style="margin:0;padding:11px 13px;border:1px solid #1E222A;border-radius:8px;background:#0A0B0D;font:400 11px/1.6 'JetBrains Mono',monospace;color:#D2A76A;white-space:pre-wrap;word-break:break-all">curl -fsSL https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.sh | bash</pre>
              </div>
              <div style="display:flex;flex-direction:column;gap:6px">
                <span style="font:400 10px/1.4 'JetBrains Mono',monospace;color:#5A616C">Windows (PowerShell)</span>
                <pre style="margin:0;padding:11px 13px;border:1px solid #1E222A;border-radius:8px;background:#0A0B0D;font:400 11px/1.6 'JetBrains Mono',monospace;color:#D2A76A;white-space:pre-wrap;word-break:break-all">irm https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.ps1 | iex</pre>
              </div>
              <p style="margin:0;font:400 11px/1.5 'Space Grotesk',system-ui,sans-serif;color:#6E7480">
                Then <code style="color:#8E949E">nexql-mcp setup</code> for the wizard, or <code style="color:#8E949E">nexql-mcp init &lt;client&gt;</code> for a paste-ready snippet.
              </p>
            </div>

            <div style="display:flex;flex-direction:column;gap:4px;padding-top:4px;border-top:1px solid #1A1E24">
              <span style="font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#8B929E">FALLBACK — DIRECT DOWNLOAD</span>
              <p style="margin:0;font:400 11px/1.5 'Space Grotesk',system-ui,sans-serif;color:#6E7480">
                No network access for a script, or prefer to verify the binary yourself? Grab it straight from{" "}
                <a href="https://github.com/NexQL-OSS/mcp/releases/latest" target="_blank" rel="noreferrer" style="color:#D2A76A">GitHub Releases</a>{" "}
                — macOS, Linux, and Windows builds, no runtime required.
              </p>
            </div>

            <div style="display:flex;flex-direction:column;gap:7px;padding-top:4px;border-top:1px solid #1A1E24">
              {LINKS.map((l) => (
                <a key={l.name} href={l.href} target="_blank" rel="noreferrer" style="display:flex;align-items:baseline;gap:10px">
                  <span style="font:400 12.5px/1.4 'Space Grotesk',system-ui,sans-serif;color:#F2F0EA;white-space:nowrap">{l.name}</span>
                  <span style="font:400 10px/1.4 'JetBrains Mono',monospace;color:#5A616C;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{l.note}</span>
                </a>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
