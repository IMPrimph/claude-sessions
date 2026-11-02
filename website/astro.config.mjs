// @ts-check
import { defineConfig } from 'astro/config';

import tailwindcss from '@tailwindcss/vite';

// https://astro.build/config
export default defineConfig({
  // The site's address. Canonical URLs, the sitemap, robots.txt, llms.txt and structured data
  // all derive from this line: it's the only thing to change when the domain moves.
  trailingSlash: 'always',
  site: 'https://claude-sessions-blond.vercel.app',
  vite: {
    plugins: [tailwindcss()]
  }
});