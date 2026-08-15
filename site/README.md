# NexQL MCP site

Astro + Preact static site for [NexQL MCP](https://github.com/NexQL-OSS/mcp), deployed at `nexql-mcp.astrx.dev`.

## Stack

- **Astro 7** — static SSG, SEO-friendly HTML
- **Preact islands** — copy buttons, KB search
- **Theme switcher** — same NexQL Themes CDN integration as [nexql.astrx.dev](https://nexql.astrx.dev)
- **@astrojs/sitemap** — auto sitemap for crawlers

## Structure

| Route | Source |
|-------|--------|
| `/` | Landing |
| `/install` | Installation paths |
| `/features` | Feature index |
| `/features/[slug]` | Per-feature docs (8 topics) |
| `/agents` | Agent workflows |
| `/docs` | Documentation hub |
| `/docs/configuration` | Profiles & config.toml |
| `/docs/tools` | Full MCP tool catalog |
| `/docs/commands` | CLI reference |
| `/docs/clients` | MCP client wiring |
| `/docs/transport` | Stdio vs HTTP |
| `/kb` | Searchable knowledge base |

Content data: `src/data/{tools,commands,features,kb,site}.ts`

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

## Visual parity

Matches NexQL / NexQL Themes sites: Space Grotesk, Instrument Serif, JetBrains Mono, theme picker loading palettes from `nexql-themes.astrx.dev`. Header links to NexQL and Themes.

## Sync with repo

When the MCP interface changes, update:

- `src/data/tools.ts` — tool catalog
- `src/data/commands.ts` — CLI from `crates/nexql-mcp/src/main.rs`
- `src/data/features.ts` — feature docs
- `src/data/kb.ts` — knowledge base articles
- `src/data/site.ts` — version string
