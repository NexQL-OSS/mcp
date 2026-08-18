/** Shared types, palette, and helpers for StoryDemo + SetupDemo. */

export type Tone = 0 | 1 | 2;

export interface LineMark {
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

export interface TableCell {
  text: string;
  tone: Tone;
}

export type TableRowSpec = (string | [string, Tone])[];

export interface TableMark {
  kind: "table";
  title: string;
  sub: string;
  head: { t: string; al: "left" | "right" }[];
  rows: TableRowSpec[];
}

export interface HbarsMark {
  kind: "hbars";
  title: string;
  sub: string;
  max: number;
  items: [string, number, string, Tone][];
}

export interface ListMark {
  kind: "list";
  title: string;
  items: ["hot" | "ok" | "info", string, string?][];
}

export interface NoteMark {
  kind: "note";
  title: string;
  lines: ["step" | "warn" | "p", string, string?][];
}

export interface CodeBlockSpec {
  /** Mono label above the block, e.g. "Paste into your agent". */
  label: string;
  code: string;
  hint?: string;
}

export interface CodeMark {
  kind: "code";
  title: string;
  sub?: string;
  blocks: CodeBlockSpec[];
}

export type Mark = LineMark | TableMark | HbarsMark | ListMark | NoteMark | CodeMark;

export const CODE_PRE_STYLE =
  "margin:0;padding:12px 14px;border:1px solid #1E222A;border-radius:8px;background:#0A0B0D;font:400 11.5px/1.65 'JetBrains Mono',monospace;color:#D2A76A;white-space:pre-wrap;word-break:break-word";

export interface Scene {
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

export const AMBER = "#D2A76A";
export const MINT = "#77CBA4";
export const HOT = "#E0885B";
export const NEUT = "rgba(242,240,234,.19)";
export const BODY = "400 13px/1.5 'Space Grotesk',system-ui,sans-serif";
export const BODY_M = "400 12px/1.5 'JetBrains Mono',monospace";

export type ClientShape = "claude" | "vscode";

export interface NpxClient {
  key: string;
  label: string;
  file: string;
  shape: ClientShape;
}

export const NPX_CLIENTS: NpxClient[] = [
  { key: "claude-desktop", label: "Claude Desktop", file: "claude_desktop_config.json", shape: "claude" },
  { key: "claude-code", label: "Claude Code", file: ".mcp.json", shape: "claude" },
  { key: "cursor", label: "Cursor", file: ".cursor/mcp.json", shape: "claude" },
  { key: "vscode", label: "VS Code · GitHub Copilot", file: ".vscode/mcp.json", shape: "vscode" },
  { key: "antigravity", label: "Antigravity", file: "~/.gemini/antigravity-ide/mcp_config.json", shape: "claude" },
  { key: "windsurf", label: "Windsurf", file: "~/.codeium/windsurf/mcp_config.json", shape: "claude" },
];

export const PROFILE_PLACEHOLDER = "mydb";

export function npxSnippet(shape: ClientShape): string {
  const entry = `{
      "command": "npx",
      "args": ["-y", "nexql-mcp", "--profile", "${PROFILE_PLACEHOLDER}"]${shape === "vscode" ? ',\n      "type": "stdio"' : ""}
    }`;
  return shape === "vscode"
    ? `{\n  "servers": {\n    "nexql-mcp": ${entry}\n  }\n}`
    : `{\n  "mcpServers": {\n    "nexql-mcp": ${entry}\n  }\n}`;
}

export const GENERIC_SNIPPET = `{
  "command": "npx",
  "args": ["-y", "nexql-mcp", "--profile", "${PROFILE_PLACEHOLDER}"]
}`;

export const LINKS = [
  { name: "Repository", href: "https://github.com/NexQL-OSS/mcp", note: "Rust workspace · 6 crates · GPL-3.0-only" },
  { name: "Tool reference", href: "https://github.com/NexQL-OSS/mcp/blob/main/docs/REFERENCE.md", note: "All 54 tools, arguments and access modes" },
  { name: "Client setup", href: "https://github.com/NexQL-OSS/mcp/tree/main/docs/clients", note: "Config paths and paste blocks per client" },
  { name: "Quickstarts", href: "https://github.com/NexQL-OSS/mcp/tree/main/docs", note: "Developer · DBA · analyst · PM tracks" },
  { name: "MCP registry", href: "https://registry.modelcontextprotocol.io/servers/io.github.NexQL-OSS/nexql-mcp", note: "io.github.NexQL-OSS/nexql-mcp" },
];

export function linePath(vals: number[], ymax: number): string {
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

export const toneFill = (t: Tone) => (t === 1 ? HOT : t === 2 ? MINT : NEUT);
export const toneText = (t: Tone) => (t === 1 ? HOT : t === 2 ? MINT : "#8E949E");

export function tableCell(
  c: string | [string, Tone],
  head: { al: "left" | "right" } | undefined,
  colIndex: number,
): TableCell & { al: "left" | "right"; font: string } {
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

export interface TurnView {
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

export type TurnVariant = "story" | "setup";

export function buildTurn(
  scenes: Scene[],
  roleHue: (role: string) => string,
  n: number,
  live: boolean,
  t: number,
  cur: boolean,
  variant: TurnVariant = "story",
): TurnView {
  const sc = scenes[n];
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
        ? variant === "setup"
          ? "paste the prompt below"
          : "answered from the live database"
        : shown < sc.steps.length
          ? shown + " of " + sc.steps.length + (variant === "setup" ? " steps" : " checks")
          : !insightOn
            ? variant === "setup"
              ? "preparing the prompt"
              : "writing the answer"
            : variant === "setup"
              ? "copy the highlighted blocks"
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

export function prefersReducedMotion(): boolean {
  return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}
