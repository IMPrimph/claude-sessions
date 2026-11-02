# Evertusk website

The landing page: one Astro page (`src/pages/index.astro`) styled with Tailwind, deployed to Vercel on every push to `main`.

```sh
npm install
npm run dev     # http://localhost:4321
npm run build
```

The install count is read at build time from `../stats/downloads.jsonl`. See `../RELEASING.md` for what to change on a release.

`public/demo` is the working demo in the page, built from the app with `npm run build:demo` in the repo root. Its sessions are made up: see `scripts/build_demo_data.py`.

The site's address lives in one place, `site` in `astro.config.mjs`. See `SEO.md` for what to do when the domain changes, and run `npm run check:seo` after a build.
