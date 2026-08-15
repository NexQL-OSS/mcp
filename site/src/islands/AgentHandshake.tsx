import { useCallback, useEffect, useMemo, useRef, useState } from "preact/hooks";

/**
 * The landing-page demo: an agent asking a question in English, and the four
 * beats NexQL MCP answers it in — orient, search the index, resolve the join,
 * run the bounded read.
 *
 * Renders into the .nx-playground / .nx-pg-* chassis in styles/nx.css, the same
 * shape nexql.astrx.dev uses for its SQL-assistant panel, so the two sites read
 * as one product.
 *
 * The typewriter loop (and its prefers-reduced-motion early-out) follows
 * website/src/islands/AiTypewriter.tsx.
 */

interface Hit {
  object: string;
  detail: string;
  score: string;
}

interface Scenario {
  id: string;
  title: string;
  ask: string;
  /** Tool the agent reaches for, shown in the cell bar. */
  tool: string;
  /** Arguments, typed out character by character. */
  call: string;
  /** One-line narration above the call. */
  reply: string;
  hits: Hit[];
  resultLabel: string;
  columns: string[];
  rows: string[][];
  /** Access mode the call resolved under. */
  mode: string;
}

const SCENARIOS: Scenario[] = [
  {
    id: "revenue",
    title: "Find the right tables",
    ask: "Which tables do I need for revenue by customer?",
    tool: "search_schema",
    call: `{
  "query": "customer revenue orders",
  "limit": 4
}`,
    reply: "Searching the offline index — no catalog round-trip, no schema dump in context.",
    hits: [
      { object: "public.customers", detail: "id · name · created_at", score: "0.94" },
      { object: "public.orders", detail: "customer_id → customers.id", score: "0.91" },
      { object: "public.order_items", detail: "order_id · amount_cents", score: "0.77" },
      { object: "public.refunds", detail: "order_id · amount_cents", score: "0.61" },
    ],
    resultLabel: "4 objects · 38 ms · index build 2h ago",
    columns: ["object", "kind", "rows"],
    rows: [
      ["public.customers", "table", "48,201"],
      ["public.orders", "table", "1,284,910"],
      ["public.order_items", "table", "3,901,554"],
    ],
    mode: "read-only",
  },
  {
    id: "join",
    title: "Resolve the join",
    ask: "How do refunds connect to customers?",
    tool: "get_join_path",
    call: `{
  "from": "public.refunds",
  "to": "public.customers"
}`,
    reply: "Walking the join graph — catalog foreign keys first, inferred edges as fallback.",
    hits: [
      { object: "refunds.order_id", detail: "→ orders.id", score: "fk" },
      { object: "orders.customer_id", detail: "→ customers.id", score: "fk" },
    ],
    resultLabel: "2 hops · both edges from pg_constraint",
    columns: ["step", "join", "confidence"],
    rows: [
      ["1", "refunds → orders", "declared"],
      ["2", "orders → customers", "declared"],
    ],
    mode: "read-only",
  },
  {
    id: "select",
    title: "Run it, bounded",
    ask: "Top customers by refunded amount this quarter.",
    tool: "run_select",
    call: `{
  "sql": "SELECT c.name, SUM(r.amount_cents)/100.0 AS refunded
          FROM refunds r
          JOIN orders o ON o.id = r.order_id
          JOIN customers c ON c.id = o.customer_id
          WHERE r.created_at >= date_trunc('quarter', now())
          GROUP BY c.name ORDER BY refunded DESC",
  "max_rows": 50
}`,
    reply: "Parsed with pg_query before it runs. A write here would be refused, not truncated.",
    hits: [],
    resultLabel: "3 of 27 rows · 41 ms · capped at max_rows=50",
    columns: ["name", "refunded"],
    rows: [
      ["Northwind Traders", "12,480.00"],
      ["Contoso Ltd", "9,120.50"],
      ["Fabrikam Inc", "7,655.25"],
    ],
    mode: "read-only",
  },
];

const TYPE_MS = 14;

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

export function AgentHandshake() {
  const [index, setIndex] = useState(0);
  // Seeded with the finished text so the server-rendered HTML shows a complete
  // tool call. The typing effect clears it on mount and replays it; without JS
  // the panel still reads as a finished exchange rather than an empty box.
  const [typed, setTyped] = useState(SCENARIOS[0].call);
  const [done, setDone] = useState(true);
  /** Set once the visitor clicks a scenario — stops the auto-advance. */
  const [pinned, setPinned] = useState(false);

  const scenario = SCENARIOS[index];
  const reduce = useMemo(prefersReducedMotion, []);

  // Type the tool-call arguments out.
  useEffect(() => {
    setDone(false);

    if (reduce) {
      setTyped(scenario.call);
      setDone(true);
      return;
    }

    setTyped("");
    let i = 0;
    const id = window.setInterval(() => {
      i += 1;
      setTyped(scenario.call.slice(0, i));
      if (i >= scenario.call.length) {
        window.clearInterval(id);
        setDone(true);
      }
    }, TYPE_MS);

    return () => window.clearInterval(id);
  }, [scenario, reduce]);

  // Advance to the next scenario a beat after the results land.
  useEffect(() => {
    if (!done || pinned || reduce) return;
    const id = window.setTimeout(() => {
      setIndex((n) => (n + 1) % SCENARIOS.length);
    }, 4200);
    return () => window.clearTimeout(id);
  }, [done, pinned, reduce]);

  const select = useCallback((n: number) => {
    setPinned(true);
    setIndex(n);
  }, []);

  return (
    <div class="nx-playground">
      <div class="nx-playground-prompts">
        {SCENARIOS.map((s, i) => (
          <button
            key={s.id}
            type="button"
            class={i === index ? "nx-prompt is-active" : "nx-prompt"}
            aria-pressed={i === index ? "true" : "false"}
            onClick={() => select(i)}
          >
            <div class="nx-prompt-head">
              <span class="nx-prompt-num">0{i + 1}</span>
              <span class="nx-prompt-title">{s.title}</span>
            </div>
            <div class="nx-prompt-ask">“{s.ask}”</div>
          </button>
        ))}

        <div class="nx-prompt-note">
          <b>Read-only unless you say otherwise.</b> The server starts every pool connection with{" "}
          <code>default_transaction_read_only = ON</code>. Write and admin tools are listed to the
          client but refused at call time until <code>--access-mode</code> says so.
        </div>
      </div>

      <div class="nx-playground-panel">
        <div class="nx-pg-hdr">
          <img src="/assets/NexQL.png" alt="" width="20" height="20" />
          <span class="nx-pg-hdr-title">nexql-mcp</span>
          <span class="nx-chip">stdio · {scenario.mode}</span>
        </div>

        <div class="nx-pg-chat">
          <div class="nx-pg-user">{scenario.ask}</div>
          <div class="nx-pg-ai-row">
            <div class="nx-pg-ai-avatar" aria-hidden="true"></div>
            <div class="nx-pg-ai-body">
              <p class="nx-pg-reply">{scenario.reply}</p>
              <div class="nx-pg-cell">
                <div class="nx-pg-cell-bar">
                  <span class="nx-pg-run">▶</span>
                  <span class="mcp-demo-toolcall">{scenario.tool}</span>
                  <span class="nx-pg-run-state">{done ? "returned" : "calling…"}</span>
                </div>
                <pre
                  class={done ? "nx-pg-sql" : "nx-pg-sql mcp-demo-caret"}
                  aria-live="polite"
                >{typed}</pre>
              </div>
            </div>
          </div>
        </div>

        <div class="nx-pg-results">
          <div class="nx-pg-meta">{scenario.resultLabel}</div>

          {scenario.hits.length > 0 && (
            <div class="mcp-demo-hits">
              {scenario.hits.map((h) => (
                <div class="mcp-demo-hit" key={h.object}>
                  <b>{h.object}</b>
                  <span>{h.detail}</span>
                  <span class="mcp-demo-hit-score">{h.score}</span>
                </div>
              ))}
            </div>
          )}

          {scenario.hits.length === 0 && (
            <table class="nx-table">
              <thead>
                <tr>
                  {scenario.columns.map((c) => (
                    <th key={c}>{c}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {scenario.rows.map((row) => (
                  <tr key={row.join("|")}>
                    {row.map((cell) => (
                      <td key={cell}>{cell}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </div>
  );
}
