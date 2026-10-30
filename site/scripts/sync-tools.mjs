#!/usr/bin/env node
// Regenerates src/data/tools.json from a nexql-mcp binary, so the docs list
// exactly what the server advertises: names, descriptions, parameters,
// annotations, which --tools sets include each tool, and the MCP prompts.
//
//   NEXQL_MCP_BIN=../target/release/nexql-mcp \
//   DATABASE_URL=postgres://user@localhost:5432/db \
//   node scripts/sync-tools.mjs
//
// The server needs a database it can reach to start. Any one will do; the
// tool list does not depend on its contents. Pass the password with
// PGPASSWORD rather than in the URL. Config, keyring and index writes go to a
// throwaway directory, never your real ~/.config/nexql-mcp.

import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const bin = process.env.NEXQL_MCP_BIN ?? "nexql-mcp";
const url = process.env.DATABASE_URL;
if (!url) {
  console.error("Set DATABASE_URL to a database nexql-mcp can connect to.");
  process.exit(1);
}

const scratch = mkdtempSync(join(tmpdir(), "nexql-sync-"));
const env = {
  PATH: process.env.PATH,
  HOME: scratch,
  XDG_CONFIG_HOME: join(scratch, "config"),
  NEXQL_MCP_INDEX_DIR: join(scratch, "index"),
  NEXQL_MCP_OPEN_BROWSER: "0",
  // Keeps the server away from the real OS keyring.
  DBUS_SESSION_BUS_ADDRESS: "unix:path=/nonexistent",
  ...(process.env.PGPASSWORD ? { PGPASSWORD: process.env.PGPASSWORD } : {}),
};

/** Runs one server, sends `requests` after initialize, returns results by id. */
function ask(args, requests) {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, [...args, url], { env, stdio: ["pipe", "pipe", "pipe"] });
    const results = new Map();
    let buf = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error(`timed out waiting for ${bin}\n${stderr}`));
    }, 30_000);
    child.stderr.on("data", (d) => (stderr += d));
    child.stdout.on("data", (d) => {
      buf += d;
      let nl;
      while ((nl = buf.indexOf("\n")) >= 0) {
        const line = buf.slice(0, nl).trim();
        buf = buf.slice(nl + 1);
        if (!line) continue;
        const msg = JSON.parse(line);
        if (msg.id === 0) {
          child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }) + "\n");
          requests.forEach((r, i) => child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: i + 1, ...r }) + "\n"));
        } else if (msg.id) {
          results.set(msg.id, msg.result ?? msg.error);
          if (results.size === requests.length) {
            clearTimeout(timer);
            child.kill();
            resolve(requests.map((_, i) => results.get(i + 1)));
          }
        }
      }
    });
    child.on("exit", (code) => {
      if (results.size < requests.length) {
        clearTimeout(timer);
        reject(new Error(`${bin} exited with ${code}\n${stderr}`));
      }
    });
    child.stdin.write(
      JSON.stringify({
        jsonrpc: "2.0",
        id: 0,
        method: "initialize",
        params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "site-sync", version: "0" } },
      }) + "\n",
    );
  });
}

const listTools = { method: "tools/list", params: {} };
// Admin mode so write and admin tools are listed too.
const base = ["--access-mode", "admin", "--i-know-what-im-doing"];

try {
  const [full, prompts] = await ask([...base, "--tools", "full"], [listTools, { method: "prompts/list" }]);
  const sets = {};
  for (const set of ["query", "dba", "meta"]) {
    const [res] = await ask([...base, "--tools", set], [listTools]);
    sets[set] = new Set(res.tools.map((t) => t.name));
  }

  const tools = full.tools.map((t) => {
    const schema = t.inputSchema ?? {};
    const required = new Set(schema.required ?? []);
    return {
      name: t.name,
      description: t.description,
      annotations: t.annotations ?? {},
      sets: ["query", "dba", "meta"].filter((s) => sets[s].has(t.name)),
      params: Object.entries(schema.properties ?? {}).map(([name, p]) => ({
        name,
        type: p.type ?? (p.enum ? "string" : "any"),
        required: required.has(name),
        description: p.description ?? "",
        ...(p.enum ? { enum: p.enum } : {}),
      })),
    };
  });

  const out = join(dirname(fileURLToPath(import.meta.url)), "../src/data/tools.json");
  writeFileSync(out, JSON.stringify({ tools, prompts: prompts.prompts ?? [] }, null, 2) + "\n");
  console.log(`wrote ${tools.length} tools and ${(prompts.prompts ?? []).length} prompts to ${out}`);
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
