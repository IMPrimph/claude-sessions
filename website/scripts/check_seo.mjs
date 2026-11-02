// Checks the built site for the SEO mistakes that fail silently. Run after a build: npm run check:seo
import fs from 'node:fs'
import path from 'node:path'

const DIST = new URL('../dist/', import.meta.url).pathname
const problems = []
const read = (file) => fs.readFileSync(path.join(DIST, file), 'utf-8')
const metaContent = (html, name) => html.match(new RegExp(`<meta (?:name|property)="${name}" content="([^"]*)"`))?.[1]

const site = read('robots.txt').match(/Sitemap: (.*)\/sitemap\.xml/)?.[1]
if (!site) problems.push('robots.txt has no sitemap')
const listed = [...read('sitemap.xml').matchAll(/<loc>(.*?)<\/loc>/g)].map((match) => match[1].replace(site, ''))

for (const page of listed) {
  const file = path.join(page, 'index.html')
  if (!fs.existsSync(path.join(DIST, file))) { problems.push(`${page}: in the sitemap but not built`); continue }
  const html = read(file)
  const fail = (message) => problems.push(`${page}: ${message}`)
  const title = html.match(/<title>(.*?)<\/title>/)?.[1] ?? ''
  const description = metaContent(html, 'description') ?? ''
  if (title.length < 10 || title.length > 65) fail(`title is ${title.length} characters`)
  if (description.length < 70 || description.length > 165) fail(`description is ${description.length} characters`)
  if (!html.includes(`<link rel="canonical" href="${site}${page}"`)) fail('wrong canonical')
  if (html.includes('noindex')) fail('noindex on a page in the sitemap')
  if ((html.match(/<h1[ >]/g) ?? []).length !== 1) fail('needs exactly one h1')
  for (const block of html.matchAll(/<script type="application\/ld\+json">(.*?)<\/script>/gs)) {
    try { JSON.parse(block[1]) } catch { fail('invalid JSON-LD') }
  }
  for (const link of html.matchAll(/href="(\/[^"#]*)/g)) {
    const target = link[1]
    const exists = fs.existsSync(path.join(DIST, target)) && (!target.endsWith('/') || fs.existsSync(path.join(DIST, target, 'index.html')))
    if (!exists) fail(`broken link ${target}`)
    if (!path.extname(target) && !target.endsWith('/')) fail(`link without a trailing slash ${target}`)
  }
  for (const image of html.matchAll(/<img [^>]*>/g)) {
    if (!/alt="[^"]/.test(image[0]) || !/width=/.test(image[0])) fail('image without alt text or size')
  }
}
if (listed.some((page) => page.startsWith('/demo'))) problems.push('the demo is in the sitemap')
if (!read('demo/index.html').includes('noindex')) problems.push('the demo can be indexed')

if (problems.length) { console.error(problems.join('\n')); process.exit(1) }
console.log(`SEO checks passed for ${listed.length} pages.`)
