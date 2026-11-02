# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

A Tauri desktop app for macOS (12+), rendered in WKWebView. Windows open at 1200×800 with a minimum of 800×600. There are no phone or tablet targets.

## Users

Heavy multi-tool users of AI coding agents: dozens of project folders, hundreds of sessions, several tools (Claude Code and Codex today), long runs with subagents and workflows. They arrive mid-work, usually with a specific thing in mind, and want to get back to their editor quickly. Plain language keeps newcomers oriented, but the design is tuned for the heavy user first.

## Product Purpose

Find and understand past agent work. The main job is finding it: "where did I solve X?", across every tool and project. Understanding what a session did (changes, cost, failures, decisions) is one step away from any result. Success is reaching the right moment in the right session in seconds, and trusting that what's shown matches what really happened.

## Positioning

One local history across agent tools, faithful to the raw transcripts. An agent vendor is unlikely to show a competitor's sessions, and its own app reads as a chat history rather than a record of what a run did. Keeping sessions past a tool's cleanup is a supporting benefit, not the pitch: Claude Code's retention is a setting, and no retention limit was found for Codex.

## Operating Context

- Reads local files only: `~/.claude/projects` and `~/.codex`. Nothing is uploaded, and there is no server or account.
- One folder can hold sessions from several tools, and more tools may follow. A project is a folder path, not a tool.
- Used alongside an editor and terminal, in short visits, with sessions that may still be running.
- Sessions range from a few messages to thousands of tool calls, hundreds of agents and hundreds of MB. Some have expired and live only in the local archive.

## Capabilities and Constraints

- Current: browse and search sessions; full conversation with thinking, tool input and output, plans, recaps, questions, artifacts, shell commands, hooks and errors; subagent and workflow transcripts; file diffs; session stats; save/archive; message bookmarks; Markdown export; dark, light and bright themes; keyboard shortcuts; auto-update.
- Tool coverage differs. Cost exists only for Claude Code (from its own records). Codex thinking is often encrypted, and Codex records tokens but no cost. Saving a copy applies only to Claude Code sessions. Never imply a tool has data it doesn't.
- Transcript formats are undocumented and change often. `scripts/audit_transcripts.py` guards fidelity.
- Stack: Tauri 2, Rust, Svelte 5, TypeScript. Local-only network policy apart from the updater; a strict CSP.
- Undecided: PDF export, pricing, team features, app signing timing.

## Brand Commitments

The name Evertusk and the amber elephant mark stay. Always state that it isn't affiliated with or endorsed by Anthropic; apply the same care to OpenAI.

## Evidence on Hand

- 58 DMG downloads as of 2026-10-31 (`stats/downloads.jsonl`). No testimonials, customers or benchmarks exist; do not invent any.
- Real findings from the owner's own use: a session worth about $800 and a hook failing 4,024 times in one session, neither noticed before.
- Screens and demo video: `website/public/`.

## Product Principles

1. Faithful first: show what the transcript says, and label what's missing rather than guessing.
2. Find before browse: the fastest route to a past moment beats any hierarchy.
3. Signal over transcript: surface decisions, changes and failures; keep the raw record one step away.
4. Tools are facts, not worlds: a tool label describes a session, and a folder stays one project whichever tools touched it.
5. Local and honest: no account, no upload, no claims the data can't back.

## Accessibility & Inclusion

Full keyboard operation, visible focus, respect for reduced motion, and tool or status never conveyed by color alone. Required standard not yet set by the owner.
