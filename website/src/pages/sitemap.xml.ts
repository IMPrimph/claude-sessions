import { PAGES, SITE } from '../site'

export const GET = () => new Response(`<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${PAGES.map((page) => `  <url>
    <loc>${SITE}${page.path}</loc>
    <lastmod>${page.updated}</lastmod>
  </url>`).join("\n")}
</urlset>
`, { headers: { "Content-Type": "application/xml" } })
