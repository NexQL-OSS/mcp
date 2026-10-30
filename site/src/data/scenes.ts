/**
 * Shared shape of the replayed conversations on /story and /setup. Each scene
 * is one question and the answer it got; SceneFlow renders them in order as
 * page sections and SceneMark draws each scene's chart, table or code.
 */

/** 0 = neutral, 1 = bad (hot), 2 = good (ok). */
export type Tone = 0 | 1 | 2;

export interface LineMark {
  kind: "line";
  title: string;
  sub: string;
  vals: number[];
  ymax: number;
  tone: "hot" | "ok";
  xa: string;
  xb: string;
  /** 0–1 across the x axis. */
  vline?: number;
  vlineLabel?: string;
}

export interface TableMark {
  kind: "table";
  title: string;
  sub: string;
  head: { t: string; al: "left" | "right" }[];
  rows: (string | [string, Tone])[][];
}

export interface HbarsMark {
  kind: "hbars";
  title: string;
  sub: string;
  max: number;
  items: [label: string, value: number, shown: string, tone: Tone][];
}

export interface ListMark {
  kind: "list";
  title: string;
  items: [kind: "hot" | "ok", text: string, detail?: string][];
}

export interface NoteMark {
  kind: "note";
  title: string;
  lines: [kind: "step" | "warn" | "p", text: string, detail?: string][];
}

export interface CodeMark {
  kind: "code";
  title: string;
  sub?: string;
  blocks: { label: string; code: string; hint?: string }[];
}

export type Mark = LineMark | TableMark | HbarsMark | ListMark | NoteMark | CodeMark;

export interface Scene {
  /** Chapter it belongs to, e.g. "Noticed". */
  chapter: string;
  /** Short name for the rail, e.g. "The drop". */
  label: string;
  role: string;
  where: string;
  prompt: string;
  /** [tool name or step number, what it did, timing or note ("—" for none)] */
  steps: [string, string, string][];
  insight: string;
  figures: [value: string, label: string][];
  mark: Mark;
}

export interface Replay {
  scenes: Scene[];
  agentLabel: string;
  stepsLabel: string;
  /** Colour of the asker's bubble, per role. */
  roleHue: (role: string) => string;
}

/** Brand hues from nexql.astrx.dev, as used on the docs' tool lines. */
export const HUE = {
  accent: "#e0a4cc",
  blue: "#3d6fe0",
  indigo: "#6c4cd6",
  hot: "#f26b3a",
  ok: "#4ade80",
};

/** Polyline points for a 600×140 chart. */
export function linePoints(vals: number[], ymax: number): string {
  const lo = Math.min(...vals) * 0.82;
  return vals
    .map((v, i) => {
      const x = (i / (vals.length - 1)) * 600;
      const y = 140 - Math.min(1, (v - lo) / (ymax - lo)) * 132;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}
