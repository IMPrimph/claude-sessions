# Claude Sessions — Launch Kit

Brand: **amber "kept"** · Positioning: **"Keep your Claude Code history. Nothing expires."**

---

## 1. The one-liner (use everywhere)

> **Claude Code deletes your session history after 30 days. Claude Sessions keeps the ones you care about — searchable, local, forever.**

Short forms:
- **Never lose a session.**
- Your Claude Code memory, kept.
- The keeper for your Claude Code history.

---

## 2. The shareable hook (this is what spreads)

Lead with the *loss*, not the feature. Most people don't know the deletion is happening — that surprise is the share trigger.

> **Did you know? Claude Code quietly deletes your session history after 30 days.**
> Every transcript, every decision, every pasted screenshot — gone. No warning, no undo.
> I built a free, local app that keeps the sessions you care about before they expire. 👇

Use this verbatim as the opening of the HN comment, the tweet, and the Reddit body. It's a public-service framing, not a sales pitch — that's why it travels.

---

## 3. Show HN

**Title:**
`Show HN: Claude Sessions – Claude Code deletes your history after 30 days; keep it`

**Body:**
```
Claude Code stores every session as a local JSONL file, then deletes them after
~30 days (cleanupPeriodDays). Most people don't realize it until a session they
wanted is just... gone — transcript, tool calls, pasted images, all of it.

Claude Sessions is a small, local desktop app (Tauri + Rust + Svelte) that:

- Browses every session across every project, with search and keyboard nav
- Surfaces things the raw logs bury: mid-turn interrupts, the option you picked
  in an AskUserQuestion, published artifacts, and fork/branch lineage
- Lets you Save or bookmark a session, which copies it to a local folder so it
  survives the 30-day cleanup. Bookmarks to expired sessions still open.

Everything stays on your machine — no server, no account, nothing uploaded.
Free and open source. macOS build + `brew` coming.

I'd love feedback on what else you wish you could recover from your Claude Code
history.

Repo: https://github.com/IMPrimph/claude-sessions
```

Post Tue–Thu, ~8–10am ET. Reply to every comment in the first 2 hours.

---

## 4. Reddit (r/ClaudeAI, r/ClaudeCode, r/AnthropicAI)

**Title:**
`Claude Code deletes your sessions after 30 days — I built a free local app to keep the ones you care about`

**Body:** open with the hook (§2), then:
```
It reads straight from ~/.claude/projects (no cloud, no account). You get:

• All sessions, all projects, in one place — search + keyboard nav
• "Save" or bookmark a session → it's copied locally and never expires
• It surfaces stuff the logs hide: your mid-turn corrections, the answer you
  picked in a question prompt, published artifacts, and which session a fork
  branched from
• Dark / light / bright themes

Free + open source (Tauri/Rust/Svelte). Would love feedback.
[link] · [15s demo gif]
```
Read each sub's self-promo rules first; lead with value, link second.

---

## 5. X / Twitter thread

1/ Your Claude Code sessions are being deleted every 30 days. Transcripts, tool
calls, pasted screenshots — gone, no warning. 🧵

2/ I got tired of losing work I wanted to look back on, so I built **Claude
Sessions** — a free, local app that keeps the sessions you care about. [gif]

3/ Bookmark or Save a session and it's copied to your machine, so it survives the
cleanup. Even bookmarks to already-expired sessions still open.

4/ It also surfaces what the raw logs bury: mid-turn interrupts, the option you
picked in a question prompt, published artifacts, fork/branch lineage.

5/ 100% local. No server, no account, nothing uploaded. Free + open source.
Grab it 👉 [link] ⭐ if it saves you a session.

---

## 6. Demo GIF script (15–20s — the money moment)

The single most persuasive thing is showing a session **coming back from the dead**.

1. (2s) Open app → the amber icon, the grid of projects. Tagline visible.
2. (3s) Click a session → scroll the conversation, show a pasted screenshot + a
   code block rendering.
3. (3s) Hit **Save** in the header → the amber "Saved" state + the pin in the list.
4. (4s) Cut to Settings → Storage: "1 session · 17 MB · ~/Library/…". Real, local.
5. (4s) Punchline card: **"Claude Code deletes sessions after 30 days. This one won't."**

Keep it silent, captioned, 1200px wide, <5MB. This GIF goes above the fold on the
landing page and as the first reply on every launch post.

---

## 7. Distribution checklist

- [ ] **Homebrew cask** — `brew install --cask claude-sessions`. Biggest credibility + discovery win for a dev tool. Do this before the HN post.
- [ ] Landing page: hero = positioning + demo GIF, real screenshots (theming looks sharp now), one download button.
- [ ] GitHub repo: README opens with the hook + the GIF + a screenshot; add topics `claude-code`, `tauri`, `developer-tools`.
- [ ] Add a subtle "⭐ Star on GitHub" nudge in-app (Settings footer or the welcome modal).
- [ ] Tool directories: There's An AI For That, Awesome-Claude lists, Tauri "made with" showcase.

---

## 8. Positioning guardrails (voice)

- Lead with the **loss** ("deletes after 30 days"), resolve with the **keep**.
- Always say **local / private / free** — it's a real differentiator and a trust builder.
- Calm-archivist tone: you're the reliable keeper, not a hype machine.
- Don't imply Anthropic endorsement; "for Claude Code" is a descriptor, not a claim.
