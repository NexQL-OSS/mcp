// @ts-check
import { defineConfig } from "astro/config";
import preact from "@astrojs/preact";
import sitemap from "@astrojs/sitemap";

export default defineConfig({
  site: "https://nexql-mcp.astrx.dev",
  integrations: [preact(), sitemap()],
  build: {
    assets: "_astro",
  },
});
