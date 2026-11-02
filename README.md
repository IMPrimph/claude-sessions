# Evertusk

A lightweight Mac app to browse, search and read your Claude Code and Codex session history across all projects. Claude Code deletes sessions after 30 days by default; Evertusk can keep a copy of the ones you care about.

_Evertusk is an independent project. It is not affiliated with or endorsed by Anthropic or OpenAI._

## Features

- Browse Claude Code and Codex sessions in one list, grouped by project folder, and search them all, including saved sessions that have expired
- Full conversation view: the agent's text, thinking, plans, recaps and every tool call with its input, output and screenshots
- See what the raw logs bury: mid-turn interrupts, the options you picked, published artifacts, fork lineage, `!` shell commands, subagent reports, hook failures and API errors
- Open any subagent or workflow agent's own transcript
- Changes tab with the diffs from every edit, including edits made through Bash and by subagents
- Session stats: tokens, tools, models and hooks, plus cost for Claude Code sessions (Codex records none)
- Save a Claude Code session, or bookmark a message in it, to keep a full local copy past the 30-day cleanup. Saving is for Claude Code only; Codex messages can be bookmarked, but no copy is made
- Export to Markdown, whole or from any message onward
- Custom session names (from `/rename`), Cmd+F search, keyboard navigation, and dark, light and bright themes

## Install

### macOS (pre-built)

Download the latest `.dmg` from [Releases](../../releases), open it, and drag **Evertusk** to Applications.

If macOS shows "app is damaged", run this once:
```bash
xattr -cr /Applications/Evertusk.app
```

Or use the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/IMPrimph/claude-sessions/main/scripts/install.sh | bash
```

### Build from source

Requires [Node.js](https://nodejs.org/) (v18+) and [Rust](https://rustup.rs/).

```bash
git clone https://github.com/IMPrimph/claude-sessions.git
cd claude-sessions
./scripts/build.sh
```

## Development

```bash
npm install
npx tauri dev
npm run test:ui                        # UI regressions, in headless Chrome
python3 scripts/audit_transcripts.py   # what the app shows and exports vs your real transcripts
```

## How it works

Reads session data from `~/.claude/projects/` and `~/.codex/` — the files Claude Code and Codex write locally. No data is sent anywhere; everything stays on your machine.

## License

[FSL-1.1-MIT](LICENSE.md) (Functional Source License). You can read, run, and modify the code for any purpose except building a competing commercial product. Each release converts to the MIT license two years after it ships.
