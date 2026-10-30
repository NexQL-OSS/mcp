# NexQL MCP site

Astro static site for [NexQL MCP](https://github.com/NexQL-OSS/mcp), deployed at `nexql-mcp.astrx.dev`.

No client islands. The tour, the docs, and the replay pages are server-rendered HTML; small inline scripts handle the theme, the tour's choices, copy buttons, and scroll progress.

| Route | Source | What it shows |
|-------|--------|----------------|
| `/` | `src/pages/index.astro` | Guided tour: pick an app, install, connect, ask |
| `/install` | `src/pages/install.astro` | Claude Desktop download |
| `/story` | `src/pages/story.astro` | Replayed session (`ReplayPage` + `SceneFlow`) |
| `/setup` | `src/pages/setup.astro` | Agent prompts, same replay shell |
| `/docs` | `src/pages/docs/` | Guides and one page per tool |

`src/styles/theme.css` holds the palette. The header toggle follows the system theme until a visitor picks Dark or Light; the choice is stored in `localStorage` and applied before paint. `src/data/site.ts` holds `SITE_URL`, `SITE_VERSION`, `REPO_URL`, and `FACTS` for SEO meta and JSON-LD in `src/layouts/BaseLayout.astro`.

Tool pages are generated from `src/data/tools.json`. Regenerate that file with `scripts/sync-tools.mjs` when the server's tool list changes, and keep the plain-language notes in `src/data/docs.ts` in step with it (the build throws if they disagree).

## Develop

```bash
cd site
npm ci
npm run dev
```

## Build

```bash
npm run build        # output: dist/
npx astro check
```

Vercel: root `site`, framework Astro, output `dist`. `vercel.json` included.
