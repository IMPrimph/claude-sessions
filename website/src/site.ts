import fs from 'node:fs'

// The domain lives in astro.config.mjs (`site`). Everything else here and in the pages derives from it.
export const SITE = import.meta.env.SITE
export const VERSION = "1.3.0"
export const REPO = "IMPrimph/claude-sessions"
export const GITHUB_URL = `https://github.com/${REPO}`
export const DOWNLOAD_MAC = `${GITHUB_URL}/releases/download/v${VERSION}/Evertusk_${VERSION}_universal.dmg`
export const OG_IMAGE = `${SITE}/og-image.png`
export const AUTHOR = { "@type": "Person", name: "IMPrimph", url: "https://github.com/IMPrimph" }

// Real install count, read from the daily download snapshots at build time.
let installs = 58
try {
  const snapshots = fs.readFileSync(`${process.cwd()}/../stats/downloads.jsonl`, 'utf-8').trim().split('\n')
  installs = JSON.parse(snapshots[snapshots.length - 1]).total_installs
} catch {}
export const INSTALLS = installs

// Every page that belongs in search. `updated` is the last real change to its content: update it
// only when that changes, because search engines stop trusting dates that move without a reason.
export const PAGES = [
  { path: "/", title: "Evertusk", updated: "2026-11-02", summary: "What the app does, a working demo, and answers to common questions." },
  { path: "/claude-code-session-history/", title: "Where Claude Code stores your session history, and when it deletes it", updated: "2026-11-02", summary: "The files on disk, the 30-day cleanup, how to change it, and how to read old sessions." },
  { path: "/codex-session-history/", title: "Where Codex stores your session history", updated: "2026-11-02", summary: "The rollout files on disk, why most are compressed, and how to read and search them." },
  { path: "/privacy/", title: "Privacy", updated: "2026-11-02", summary: "What the app reads and writes, what it sends over the network, and what this website collects." },
]
