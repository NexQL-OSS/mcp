# NexQL MCP site

Astro + Preact static site for [NexQL MCP](https://github.com/NexQL-OSS/mcp), deployed at `nexql-mcp.astrx.dev`.

Two interactive pages, same narrative player:

| Route | Island | What it shows |
|-------|--------|---------------|
| `/` | `StoryDemo` | A replayed agent conversation — one production incident end to end (noticed → narrowed → explained → fixed → recovered) |
| `/setup` | `SetupDemo` | Install and setup walkthrough — wizard → profile → wire client → doctor → first question |

Both share `NarrativePlayer` + `story-shared.ts`. The use-case story was ported from the Claude Design source `NexQL MCP Story.dc.html`.

## Stack

- **Astro 7** — static SSG, SEO-friendly HTML
- **Preact island** (`client:load`) — the scene player, autoplay clock, and install modal
- **@astrojs/sitemap** — auto sitemap for crawlers

## Structure

| Route | Source |
|-------|--------|
| `/` | `src/pages/index.astro` → `StoryDemo` |
| `/setup` | `src/pages/setup.astro` → `SetupDemo` |

Scene scripts live in `StoryDemo.tsx` (seven scenes) and `SetupDemo.tsx` (five steps).
Install-modal data (`LINKS`, `NPX_CLIENTS`, snippets) lives in `story-shared.ts`.

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
update `LINKS`/`NPX_CLIENTS` in `src/islands/story-shared.ts`, setup scenes in
`SetupDemo.tsx`, and `FACTS`/`SITE_VERSION` in `src/data/site.ts`.
