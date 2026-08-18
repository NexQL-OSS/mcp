import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import {
  AMBER,
  CODE_PRE_STYLE,
  GENERIC_SNIPPET,
  HOT,
  LINKS,
  MINT,
  NPX_CLIENTS,
  type CodeMark,
  type Mark,
  type Scene,
  buildTurn,
  linePath,
  npxSnippet,
  prefersReducedMotion,
  tableCell,
  toneFill,
  toneText,
} from "./story-shared";

export type NarrativeMode = "use-case" | "setup";

export interface NarrativeConfig {
  scenes: Scene[];
  acts: [string, number][];
  roleHue: (role: string) => string;
  agentLabel: string;
  stepsLabel: string;
  footerNote: string;
  /** Scene index at which the radial glow shifts to mint (recovery / success). */
  mintGlowFromScene: number;
  mode: NarrativeMode;
}

interface PlayerState {
  i: number;
  t: number;
  playing: boolean;
  installOpen: boolean;
  installClient: string;
  vw: number;
}

export interface NarrativePlayerProps extends NarrativeConfig {
  autoplay?: boolean;
  sceneSeconds?: number;
  startScene?: number;
  height?: string;
}

function CopyCodeBlock({ label, code, hint }: { label: string; code: string; hint?: string }) {
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      /* clipboard denied — selection still works */
    }
  };

  return (
    <div style="display:flex;flex-direction:column;gap:6px;min-width:0">
      <div style="display:flex;align-items:center;gap:10px">
        <span style="font:500 8.5px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#D2A76A">{label}</span>
        <button
          type="button"
          onClick={() => void copy()}
          class="story-copy-btn"
          style="margin-left:auto;padding:4px 8px;border-radius:5px;border:1px solid #2A2F37;font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.08em;color:#8B929E;flex:none"
        >
          {copied ? "COPIED ✓" : "COPY"}
        </button>
      </div>
      <pre style={CODE_PRE_STYLE}>{code}</pre>
      {hint && <span style="font:400 11px/1.5 'Space Grotesk',system-ui,sans-serif;color:#6E7480;text-wrap:pretty">{hint}</span>}
    </div>
  );
}

function renderMark(tn: ReturnType<typeof buildTurn>) {
  const mark = tn.mark as Mark;
  return (
    <>
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

      {mark.kind === "line" && (
        <div style="position:relative;height:clamp(76px,13vh,124px);margin-bottom:14px">
          <span style="position:absolute;left:0;right:0;bottom:0;height:1px;background:#20242B"></span>
          <span style="position:absolute;left:0;right:0;top:50%;height:1px;background:#161A1F"></span>
          <svg viewBox="0 0 600 140" preserveAspectRatio="none" width="100%" height="100%" style="display:block;overflow:visible">
            <polyline
              points={linePath(mark.vals, mark.ymax)}
              fill="none"
              stroke={mark.tone === "mint" ? MINT : mark.tone === "hot" ? HOT : AMBER}
              stroke-width="2"
              stroke-linejoin="round"
              stroke-dasharray="1900"
              stroke-dashoffset={(1900 * (1 - tn.draw)).toFixed(0)}
            ></polyline>
          </svg>
          {mark.vline != null && (
            <span style={`position:absolute;top:0;bottom:0;left:${(mark.vline * 100).toFixed(1)}%;width:1px;background:#77CBA4;opacity:.45`}></span>
          )}
          {mark.vline != null && (
            <span style={`position:absolute;top:-3px;left:${(mark.vline * 100).toFixed(1)}%;transform:translateX(6px);font:400 8.5px/1 'JetBrains Mono',monospace;color:#77CBA4;white-space:nowrap`}>
              {mark.vlineLabel}
            </span>
          )}
          <span style="position:absolute;left:0;bottom:-15px;font:400 8.5px/1 'JetBrains Mono',monospace;color:#4A505A">{mark.xa}</span>
          <span style="position:absolute;right:0;bottom:-15px;font:400 8.5px/1 'JetBrains Mono',monospace;color:#4A505A">{mark.xb}</span>
        </div>
      )}

      {mark.kind === "hbars" && (
        <div style="display:flex;flex-direction:column;gap:10px">
          {mark.items.map((b, bk) => {
            const w = Math.max(1.2, (b[1] / mark.max) * 100 * tn.draw).toFixed(1) + "%";
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
          })}
        </div>
      )}

      {mark.kind === "table" && (
        <div style="display:flex;flex-direction:column;gap:0">
          <div style="display:grid;grid-template-columns:minmax(0,1.7fr) minmax(0,1fr) minmax(0,1fr);gap:10px;padding:0 0 7px;border-bottom:1px solid #23272E">
            {mark.head.map((h, hk) => (
              <span key={hk} style={`font:500 8.5px/1.2 'JetBrains Mono',monospace;letter-spacing:.12em;color:#5A616C;text-align:${h.al}`}>{h.t}</span>
            ))}
          </div>
          {mark.rows.map((r, rk) => (
            <div key={rk} style="display:grid;grid-template-columns:minmax(0,1.7fr) minmax(0,1fr) minmax(0,1fr);gap:10px;padding:8px 0;border-bottom:1px solid #15181D">
              {r.map((c, ck) => {
                const cell = tableCell(c, mark.head[ck], ck);
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

      {mark.kind === "list" && (
        <div style="display:flex;flex-direction:column;gap:9px">
          {mark.items.map((it, ik) => {
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

      {mark.kind === "note" && (
        <div style="display:flex;flex-direction:column;gap:10px">
          {mark.lines.map((nl, nk) => {
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

      {mark.kind === "code" && (
        <div style="display:flex;flex-direction:column;gap:14px">
          {(mark as CodeMark).blocks.map((b, bk) => (
            <CopyCodeBlock key={bk} label={b.label} code={b.code} hint={b.hint} />
          ))}
        </div>
      )}
    </>
  );
}

export function NarrativePlayer({
  scenes,
  acts,
  roleHue,
  agentLabel,
  stepsLabel,
  footerNote,
  mintGlowFromScene,
  mode,
  autoplay = true,
  sceneSeconds = 25,
  startScene = 1,
  height = "100dvh",
}: NarrativePlayerProps) {
  const dur = Math.max(8, sceneSeconds) * 1000;
  const initialI = Math.max(1, Math.min(scenes.length, startScene)) - 1;

  const [state, setState] = useState<PlayerState>(() => ({
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

  const patch = useCallback((p: Partial<PlayerState>) => {
    setState((s) => ({ ...s, ...p }));
  }, []);

  const targets = useCallback(
    (i: number): number[] => {
      const sc = scenes[i];
      const out = [(i + 1) % scenes.length];
      for (const a of sc.alts) {
        if (a !== i && out.indexOf(a) < 0) out.push(a);
      }
      for (let k = 1; out.length < 3 && k < scenes.length; k++) {
        const n = (i + 1 + k) % scenes.length;
        if (n !== i && out.indexOf(n) < 0) out.push(n);
      }
      return out;
    },
    [scenes],
  );

  const go = useCallback(
    (n: number) => {
      const L = scenes.length;
      patch({ i: ((n % L) + L) % L, t: 0 });
    },
    [patch, scenes.length],
  );

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

  useEffect(() => {
    if (state.playing || prevI.current !== state.i) pinBottom();
    prevI.current = state.i;
  });

  const { i, t, playing, installOpen, installClient, vw } = state;
  const sc = scenes[i];
  const hue = roleHue(sc.role);
  const live = playing;

  const turnVariant = mode === "setup" ? "setup" : "story";
  const bubbleMax = mode === "setup" ? "min(92ch,96%)" : "min(70ch,96%)";

  const turns = [];
  for (let n = Math.max(0, i - 2); n < i; n++) turns.push(buildTurn(scenes, roleHue, n, false, 0, false, turnVariant));
  const cur = buildTurn(scenes, roleHue, i, live, t, true, turnVariant);
  turns.push(cur);

  const tgs = targets(i);
  const optionsOn = !live || t >= cur.optStart;
  const options = tgs.map((n, k) => ({
    n,
    label: String(k + 1),
    text: scenes[n].prompt,
    isNext: k === 0,
    prog: ((t / dur) * 100).toFixed(1) + "%",
    color: k === 0 ? "#F2F0EA" : "rgba(242,240,234,.5)",
    numColor: k === 0 ? hue : "#4E545F",
    line: k === 0 ? "#3A404A" : "#1E222A",
    fill: k === 0 ? "rgba(255,255,255,.04)" : "transparent",
  }));

  const wide = vw >= 1000;
  const glow = i >= mintGlowFromScene ? "rgba(119,203,164,.07)" : "rgba(210,167,106,.07)";
  const counter = String(i + 1).padStart(2, "0") + " / " + String(scenes.length).padStart(2, "0");
  const progress = ((t / dur) * 100).toFixed(2) + "%";
  const useCaseActive = mode === "use-case";
  const setupActive = mode === "setup";
  const navStyle = (active: boolean) =>
    `padding:6px 10px;border-radius:7px;border:1px solid ${active ? "#D2A76A" : "#22262D"};background:${active ? "rgba(210,167,106,.1)" : "transparent"};font:500 10px/1 'JetBrains Mono',monospace;letter-spacing:.1em;color:${active ? "#F2F0EA" : "#8B929E"}`;

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
          {acts.map((a, k) => {
            const on = sc.act === k;
            return (
              <button key={a[0]} type="button" onClick={() => go(a[1])} style="display:flex;align-items:center;gap:7px;padding:5px 2px;flex:none">
                <span style={`width:${on ? "22px" : "9px"};height:2px;background:${on ? hue : "#2A2F37"};flex:none;transition:width .45s ease,background .45s ease`}></span>
                <span style={`font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.13em;color:${on ? "#F2F0EA" : "#4E545F"};white-space:nowrap;transition:color .45s ease`}>{a[0]}</span>
              </button>
            );
          })}
        </div>

        <nav style="display:flex;align-items:center;gap:clamp(8px,1.4vw,18px);flex:none">
          <a class="story-navlink" href="/" style={navStyle(useCaseActive)}>
            Use case
          </a>
          <a class="story-navlink" href="/setup" style={navStyle(setupActive)}>
            Setup
          </a>
          {wide && (
            <a class="story-navlink" href="https://github.com/NexQL-OSS/mcp/blob/main/docs/REFERENCE.md" target="_blank" rel="noreferrer" style="font:400 11px/1 'Space Grotesk',system-ui,sans-serif;color:#8B929E">
              Docs
            </a>
          )}
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
                    <span style="font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#8B929E">{agentLabel}</span>
                    <span style="font:400 9px/1 'JetBrains Mono',monospace;color:#3E444E;white-space:nowrap">{tn.phase}</span>
                  </div>

                  <div style={`max-width:${bubbleMax};width:${tn.bubbleW};padding:14px 16px;border-radius:14px 14px 14px 4px;border:1px solid #191D23;background:rgba(255,255,255,.028);display:flex;flex-direction:column;gap:13px`}>
                    <div style="display:flex;flex-direction:column;gap:6px">
                      <span style="font:500 8.5px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#4A505A">{stepsLabel}</span>
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
                        {renderMark(tn)}
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
            <span style="font:400 9.5px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#4E545F">{footerNote}</span>
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

            <div style="display:flex;flex-direction:column;gap:8px;padding:14px 16px;border:1px solid rgba(210,167,106,.35);border-radius:10px;background:rgba(210,167,106,.06)">
              <div style="display:flex;align-items:baseline;gap:8px;flex-wrap:wrap">
                <span style="font:500 8.5px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#D2A76A">RECOMMENDED</span>
                <span style="font:500 13px/1.3 'Space Grotesk',system-ui,sans-serif;color:#F8F6F1">Setup wizard — no manual JSON</span>
              </div>
              <pre style="margin:0;padding:11px 13px;border:1px solid #1E222A;border-radius:8px;background:#0A0B0D;font:400 11.5px/1.6 'JetBrains Mono',monospace;color:#D2A76A;white-space:pre-wrap;word-break:break-all">npx -y nexql-mcp setup</pre>
              <p style="margin:0;font:400 11.5px/1.5 'Space Grotesk',system-ui,sans-serif;color:#8E949E">
                Opens a local page in your browser: pick which clients to wire up, preview the exact config diff per file, then apply. Nothing leaves your machine.
              </p>
            </div>

            <div style="display:flex;flex-direction:column;gap:8px">
              <span style="font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#8B929E">RUN VIA NPX, NO INSTALL</span>
              <p style="margin:0;font:400 11px/1.5 'Space Grotesk',system-ui,sans-serif;color:#6E7480">
                <code style="color:#8E949E">--profile mydb</code> points at a stored profile, not a hardcoded connection —
                create it in the setup wizard above, or <code style="color:#8E949E">nexql-mcp profile add mydb --url postgres://…</code>.
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
                  <span style="font:400 11px/1.5 'Space Grotesk',system-ui,sans-serif;color:#8E949E">Every MCP client takes the same command/args pair in its own config format:</span>
                  <pre style="margin:0;padding:11px 13px;border:1px solid #1E222A;border-radius:8px;background:#0A0B0D;font:400 11px/1.6 'JetBrains Mono',monospace;color:#D2A76A;white-space:pre-wrap;word-break:break-all">{GENERIC_SNIPPET}</pre>
                </div>
              </details>
            </div>

            <div style="display:flex;flex-direction:column;gap:8px;padding-top:4px;border-top:1px solid #1A1E24">
              <span style="font:500 9px/1 'JetBrains Mono',monospace;letter-spacing:.14em;color:#8B929E">INSTALL THE BINARY</span>
              <div style="display:flex;flex-direction:column;gap:6px">
                <span style="font:400 10px/1.4 'JetBrains Mono',monospace;color:#5A616C">macOS · Linux</span>
                <pre style="margin:0;padding:11px 13px;border:1px solid #1E222A;border-radius:8px;background:#0A0B0D;font:400 11px/1.6 'JetBrains Mono',monospace;color:#D2A76A;white-space:pre-wrap;word-break:break-all">curl -fsSL https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.sh | bash</pre>
              </div>
              <div style="display:flex;flex-direction:column;gap:6px">
                <span style="font:400 10px/1.4 'JetBrains Mono',monospace;color:#5A616C">Windows (PowerShell)</span>
                <pre style="margin:0;padding:11px 13px;border:1px solid #1E222A;border-radius:8px;background:#0A0B0D;font:400 11px/1.6 'JetBrains Mono',monospace;color:#D2A76A;white-space:pre-wrap;word-break:break-all">irm https://raw.githubusercontent.com/NexQL-OSS/mcp/main/scripts/install.ps1 | iex</pre>
              </div>
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
