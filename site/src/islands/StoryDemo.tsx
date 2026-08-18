/**
 * The full-screen narrative demo: a week in the life of one online store's
 * database, told as a replayed agent conversation — notice the drop, narrow
 * it to a page, explain the plan, ship the safe fix, confirm the recovery.
 *
 * Ported from the Claude Design source (`NexQL MCP Story.dc.html`) into a
 * plain Preact island. Scene data lives here; playback chrome is in
 * NarrativePlayer.
 */

import { AMBER, MINT, type Scene } from "./story-shared";
import { NarrativePlayer } from "./NarrativePlayer";

const HOURLY = [3.9, 3.94, 3.88, 3.91, 3.86, 3.9, 3.72, 3.4, 3.05, 2.71, 2.66, 2.72, 3.1, 3.5, 3.78, 3.86, 3.9, 3.88];
const RECOVER = [2.66, 2.7, 2.68, 2.72, 2.69, 2.74, 3.1, 3.62, 3.94, 4.02, 4.06, 4.04, 4.08, 4.05, 4.07, 4.09, 4.06, 4.08];

const SCENES: Scene[] = [
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

const roleHue = (r: string) => (r === "HEAD OF ONLINE STORE" ? AMBER : r === "DATA ANALYST" ? "#93A9E6" : MINT);

interface StoryDemoProps {
  autoplay?: boolean;
  sceneSeconds?: number;
  startScene?: number;
  height?: string;
}

export function StoryDemo({ autoplay = true, sceneSeconds = 25, startScene = 1, height = "100dvh" }: StoryDemoProps) {
  return (
    <NarrativePlayer
      scenes={SCENES}
      acts={ACTS}
      roleHue={roleHue}
      agentLabel="CLAUDE + NEXQL"
      stepsLabel="CHECKED THE DATABASE"
      footerNote="one online store's database · read-only"
      mintGlowFromScene={5}
      mode="use-case"
      autoplay={autoplay}
      sceneSeconds={sceneSeconds}
      startScene={startScene}
      height={height}
    />
  );
}
