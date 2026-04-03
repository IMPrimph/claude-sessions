# Claude Sessions

A lightweight desktop app to browse your Claude Code session history across all projects.

![Claude Sessions](https://img.shields.io/badge/Tauri-2.10-blue) ![Svelte](https://img.shields.io/badge/Svelte-5-orange)

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

## Prerequisites

- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://rustup.rs/) (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

## Install

```bash
git clone <repo-url> claude-sessions
cd claude-sessions
./scripts/install.sh
```

The script will build the app and optionally install it to `/Applications` (macOS).

## Development

```bash
npm install
npx tauri dev
```

## How it works

Reads session data from `~/.claude/projects/` — the JSONL files that Claude Code writes locally. No data is sent anywhere; everything stays on your machine.
