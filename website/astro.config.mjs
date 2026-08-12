// @ts-check
import { defineConfig } from 'astro/config';

import tailwindcss from '@tailwindcss/vite';

// https://astro.build/config
export default defineConfig({
  // Canonical production origin — used by Astro for absolute URL resolution and
  // must match the SITE constant in src/pages/index.astro + public/sitemap.xml.
  site: 'https://claude-sessions-blond.vercel.app',
  vite: {
    plugins: [tailwindcss()]
  }
});