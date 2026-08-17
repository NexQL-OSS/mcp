# NexQL MCP site

Astro + Preact static site for [NexQL MCP](https://github.com/NexQL-OSS/mcp), deployed at `nexql-mcp.astrx.dev`.

A single page: `StoryDemo`, a replayed agent conversation walking through one
production incident end to end (noticed → narrowed → explained → fixed →
recovered) using NexQL MCP's read-only tools. Ported from the Claude Design
source `NexQL MCP Story.dc.html`.

## Stack

- **Astro 7** — static SSG, SEO-friendly HTML
- **Preact island** (`client:load`) — the scene player, autoplay clock, and install modal
- **@astrojs/sitemap** — auto sitemap for crawlers

## Structure

| Route | Source |
|-------|--------|
| `/` | `src/pages/index.astro` → `StoryDemo` |

Scene data (the seven-scene script, tool calls, chart data) lives inline in
`src/islands/StoryDemo.tsx`. Repo/tool-reference links shown in the Install
modal live in the same file (`LINKS`, `INSTALLS`).

`src/data/site.ts` holds the handful of constants (`SITE_URL`, `REPO_URL`,
`FACTS`, …) used for SEO meta and JSON-LD in `src/layouts/BaseLayout.astro`.

## Develop

```bash
cd mcp/site
npm install
npm run dev
```

## Build & deploy

```bash
npm run build   # output: dist/
```

Vercel: root `mcp/site`, framework Astro, output `dist`. `vercel.json` included.

## Sync with repo

When the MCP interface changes (tool count, install commands, doc links),
update the `S`, `INSTALLS`, and `LINKS` constants in
`src/islands/StoryDemo.tsx`, and `FACTS`/`SITE_VERSION` in `src/data/site.ts`.
