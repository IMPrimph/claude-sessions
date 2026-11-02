import { SITE } from '../site'

// Everyone may read everything, including search and AI crawlers: a free app wants to be found.
// /demo/ stays crawlable, since its noindex tag only works if crawlers can read it.
export const GET = () => new Response(`User-agent: *
Allow: /

Sitemap: ${SITE}/sitemap.xml
`)
