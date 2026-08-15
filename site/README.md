# NexQL MCP site

Static documentation and landing page for [NexQL MCP](https://github.com/NexQL-OSS/mcp), intended for deployment at `nexql-mcp.astrx.dev`.

## Run locally

From this directory, use any static file server:

```bash
npx serve .
# or
python3 -m http.server 3000
```

No build step or runtime dependencies are required. The page uses the same visual vocabulary as the NexQL and NexQL Themes sites: Space Grotesk, Instrument Serif, JetBrains Mono, a warm paper surface, and a clay accent.

## Deploy

Configure the deployment root/project root as `mcp/site` and publish the directory as a static site. The page has no server-side routes and is suitable for Vercel, Netlify, GitHub Pages, or any static host.

## Content source

Commands and configuration examples are based on `../README.md`, `../docs/config.example.toml`, `../docs/clients/README.md`, and `../docs/tools/README.md`. Update those source docs and this page together when the MCP interface changes.
