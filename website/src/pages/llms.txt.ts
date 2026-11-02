import { DOWNLOAD_MAC, GITHUB_URL, PAGES, SITE, VERSION } from '../site'

// Optional: Google says it ignores this file, and no major AI provider has confirmed using it.
// It costs nothing to keep, and it's a plain statement of facts for anything that does read it.
export const GET = () => new Response(`# Evertusk

> Evertusk is a free Mac app for finding past work across Claude Code and Codex sessions. It shows them in one searchable list, grouped by project folder, and keeps copies of Claude Code sessions that would otherwise be deleted after 30 days. Everything stays on the Mac.

## Facts

- Price: free. No account and no signup.
- Current version: ${VERSION}. Requires macOS 12 or later, on Apple Silicon or Intel.
- Tools covered: Claude Code and Codex.
- Search: across session names and every message, in all sessions.
- Session view: messages, thinking, tool calls and output, subagents, pasted images, plans, interrupts, answers to questions, files changed, tokens, and cost for Claude Code.
- Saving: full local copies of Claude Code sessions only. Codex messages can be bookmarked. Export to Markdown works for both.
- Privacy: reads local files only. Its only automatic network request is the update check.
- App signing: not signed by Apple yet, so the first launch needs \`xattr -cr /Applications/Evertusk.app\`.
- License: source-available under FSL-1.1-MIT: ${GITHUB_URL}
- Not affiliated with or endorsed by Anthropic or OpenAI.

## Pages

${PAGES.map((page) => `- [${page.title}](${SITE}${page.path}): ${page.summary}`).join("\n")}
- [Working demo with made-up sessions](${SITE}/demo/): the real app running in a browser.
- [Download for Mac](${DOWNLOAD_MAC})
- [Source code](${GITHUB_URL})
`)
