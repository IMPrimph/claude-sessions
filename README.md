# Evertusk

A lightweight Mac app to browse, search, and keep your Claude Code session history across all projects. Claude Code deletes sessions after 30 days; Evertusk keeps the ones you care about.

_Evertusk is an independent project for Claude Code users. It is not affiliated with or endorsed by Anthropic._

## Features

- Browse all Claude Code sessions across every project folder
- Project grid with session counts and last active time
- Session list with date grouping, search, and sort
- Full conversation view with user/assistant messages
- Compaction summaries shown as collapsible sections
- Custom session names (from `/rename`) preserved
- Search within messages (Cmd+F)
- Keyboard navigation (arrow keys)
- Token counts loaded in the background
- Handles 500k+ token sessions without lag

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
```

## How it works

Reads session data from `~/.claude/projects/` — the JSONL files that Claude Code writes locally. No data is sent anywhere; everything stays on your machine.

## License

[FSL-1.1-MIT](LICENSE.md) (Functional Source License). You can read, run, and modify the code for any purpose except building a competing commercial product. Each release converts to the MIT license two years after it ships.
