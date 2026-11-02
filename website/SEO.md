# Evertusk SEO

What's already in the site, and what can only happen once there's a domain. `npm run build && npm run check:seo` catches the mistakes that fail silently: broken links, wrong canonicals, a noindex on a real page, invalid structured data.

## When you buy the domain

`evertusk.com` is taken. Check `evertusk.app` and `evertusk.io` at a registrar: `.io` showed as unregistered on 2 November 2026 and `.app` couldn't be confirmed.

1. Change `site` in `astro.config.mjs`. Canonicals, the sitemap, robots.txt, llms.txt and structured data all follow it.
2. In Vercel, add the domain and redirect `claude-sessions-blond.vercel.app` to it with a 301, so the old address passes on what it has earned.
3. Update the repo's homepage on GitHub, and any link in `README.md` and `LAUNCH.md`.
4. Verify the domain in Google Search Console and Bing Webmaster Tools, and submit `/sitemap.xml` to both. Bing matters beyond Bing: it feeds Copilot.
5. Check the home page in Google's Rich Results Test and PageSpeed Insights, on the live URL.
6. After a month, read the real queries in Search Console and decide what to write next from those, not from guesses.

## Off-site, where AI answers get their sources

Google's guidance is that AI search is still SEO: crawlable pages, useful content, no special files. Third-party studies say AI answers lean on Reddit, YouTube and other independent mentions, so these should matter more than anything left on the site.

- Give the GitHub repo a description and topics. It currently has neither.
- Post the launch from `LAUNCH.md` on Hacker News and Reddit, once a stable link exists.
- List it on AlternativeTo, Product Hunt and awesome lists for Claude Code and Codex.
- Record a short walkthrough for YouTube. The old `demo.mp4` shows the previous UI.
- Offer a Homebrew cask, which several similar apps have, once the app is signed.

## Keeping it true

- These guides make claims about two tools that change often. Recheck them against the official docs every three months, and change `updated` in `src/site.ts` only when the content really changed.
- A comparison page would be worth having, since Agent Sessions, Poirot and Claude Code History Viewer already show up for these searches. Only write it after installing and trying each one, because a wrong claim about a competitor costs more than the page earns.

## Deliberately left out

| Thing | Why |
|---|---|
| FAQPage markup | Google removed FAQ rich results in 2026. The questions are on the page as text |
| Keyword meta tag, sitemap priority and changefreq | Google ignores them |
| ai-catalog.json, WebMCP, Markdown copies of pages | The site has no agent actions or forms to expose. Unproven for a page like this |
| Local, e-commerce and multi-language SEO | Don't apply to a free Mac app with one language |
