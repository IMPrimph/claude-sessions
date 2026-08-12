#!/usr/bin/env python3
"""Export the user's prompt history (~/.claude/history.jsonl) into per-session
Markdown files — "your side" of every conversation going back ~8 months.

Unlike the full session transcripts in ~/.claude/projects (auto-deleted after the
30-day cleanup), history.jsonl only stores what *you* typed: there are no Claude
responses or tool calls here. Each output file is clearly labelled as such.

Grouping: by sessionId. Entries with no sessionId are collected into a single
per-project "_no-session.md" file so nothing is lost.

Usage:
    python3 export_history_prompts.py [output_dir]
Default output_dir: ~/Documents/claude-sessions-export-prompts
"""

import json
import sys
import os
from collections import defaultdict
from datetime import datetime, timezone

HISTORY_PATH = os.path.expanduser("~/.claude/history.jsonl")

# Bare-noise displays that shouldn't be picked as a session's title.
TITLE_SKIP = {"login", "continue", "clear", "compact"}


def load_entries(path):
    entries = []
    with open(path, "r", encoding="utf-8") as history_file:
        for line in history_file:
            line = line.strip()
            if not line:
                continue
            try:
                entries.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return entries


def iso_timestamp(milliseconds):
    return (
        datetime.fromtimestamp(milliseconds / 1000, tz=timezone.utc)
        .isoformat(timespec="milliseconds")
        .replace("+00:00", "Z")
    )


def date_only(milliseconds):
    return datetime.fromtimestamp(milliseconds / 1000, tz=timezone.utc).strftime(
        "%Y-%m-%d"
    )


def slugify(text, max_len):
    slug_chars = []
    last_hyphen = False
    for character in text:
        if character.isalnum() and character.isascii():
            slug_chars.append(character.lower())
            last_hyphen = False
        elif not last_hyphen:
            slug_chars.append("-")
            last_hyphen = True
    slug = "".join(slug_chars).strip("-")[:max_len].strip("-")
    return slug or "untitled"


def project_folder(project_path):
    if not project_path:
        return "_unknown-project"
    return slugify(project_path.rstrip("/").split("/")[-1], 80)


def is_titleable(display):
    stripped = display.strip()
    if not stripped or stripped in TITLE_SKIP:
        return False
    # A bare slash command like "/cost" or "/effort" is weak as a title.
    if stripped.startswith("/") and len(stripped.split()) == 1:
        return False
    return len(stripped) >= 4


def pick_title(session_entries):
    for entry in session_entries:
        display = entry.get("display", "")
        if is_titleable(display):
            collapsed = " ".join(display.split())
            return collapsed[:80].strip()
    # Fallback: first non-empty display, else session id.
    for entry in session_entries:
        display = entry.get("display", "").strip()
        if display:
            return " ".join(display.split())[:80]
    return session_entries[0].get("sessionId", "untitled")


def render_pasted(pasted_contents):
    """Render pastedContents (id → {id,type,content}) as labelled blocks."""
    if not pasted_contents:
        return ""
    blocks = []
    for key in sorted(pasted_contents, key=lambda value: str(value)):
        item = pasted_contents[key]
        if not isinstance(item, dict):
            continue
        content = item.get("content", "")
        if not str(content).strip():
            continue
        label = item.get("id", key)
        blocks.append(f"<details>\n<summary>Pasted text #{label}</summary>\n\n```\n{content}\n```\n\n</details>")
    return "\n\n".join(blocks)


def build_markdown(title, project_path, session_id, session_entries):
    session_entries = sorted(session_entries, key=lambda entry: entry.get("timestamp", 0))
    first_date = date_only(session_entries[0].get("timestamp", 0))
    last_date = date_only(session_entries[-1].get("timestamp", 0))
    prompt_count = len(session_entries)

    lines = [f"# {title}", ""]
    lines.append(
        "> **Prompts-only transcript** reconstructed from `~/.claude/history.jsonl`. "
        "Claude's responses and tool calls were not retained (the full session expired "
        "under Claude Code's 30-day cleanup)."
    )
    span = first_date if first_date == last_date else f"{first_date} → {last_date}"
    lines.append(
        f">\n> Project: `{project_path}` · Session: `{session_id}` · "
        f"{prompt_count} prompt(s) · {span}"
    )
    lines.append("")

    for entry in session_entries:
        display = entry.get("display", "")
        timestamp = entry.get("timestamp", 0)
        lines.append(f"## You — {iso_timestamp(timestamp)}")
        lines.append("")
        lines.append(display if display.strip() else "_(empty)_")
        lines.append("")
        pasted = render_pasted(entry.get("pastedContents"))
        if pasted:
            lines.append(pasted)
            lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def main():
    output_root = (
        sys.argv[1]
        if len(sys.argv) > 1
        else os.path.expanduser("~/Documents/claude-sessions-export-prompts")
    )

    if not os.path.exists(HISTORY_PATH):
        print(f"history.jsonl not found at {HISTORY_PATH}", file=sys.stderr)
        sys.exit(1)

    entries = load_entries(HISTORY_PATH)
    print(f"Loaded {len(entries)} history entries from {HISTORY_PATH}")

    sessions = defaultdict(list)
    no_session_by_project = defaultdict(list)
    for entry in entries:
        session_id = entry.get("sessionId")
        if session_id:
            sessions[session_id].append(entry)
        else:
            no_session_by_project[entry.get("project") or ""].append(entry)

    written = 0
    used_paths = set()

    def unique_path(folder, base_name):
        candidate = os.path.join(folder, f"{base_name}.md")
        suffix = 1
        while candidate in used_paths:
            candidate = os.path.join(folder, f"{base_name}-{suffix}.md")
            suffix += 1
        used_paths.add(candidate)
        return candidate

    # Per-session files
    for session_id, session_entries in sessions.items():
        # Project = most common project across the session's entries.
        project_counts = defaultdict(int)
        for entry in session_entries:
            project_counts[entry.get("project") or ""] += 1
        project_path = max(project_counts, key=project_counts.get)

        folder = os.path.join(output_root, project_folder(project_path))
        os.makedirs(folder, exist_ok=True)

        title = pick_title(session_entries)
        first_ms = min(entry.get("timestamp", 0) for entry in session_entries)
        base_name = f"{date_only(first_ms)}_{slugify(title, 60)}_{session_id[:8]}"
        save_path = unique_path(folder, base_name)

        with open(save_path, "w", encoding="utf-8") as out_file:
            out_file.write(build_markdown(title, project_path, session_id, session_entries))
        written += 1

    # One "_no-session" file per project for the stray entries
    for project_path, stray_entries in no_session_by_project.items():
        folder = os.path.join(output_root, project_folder(project_path))
        os.makedirs(folder, exist_ok=True)
        save_path = unique_path(folder, "_no-session")
        title = "Prompts without a session id"
        with open(save_path, "w", encoding="utf-8") as out_file:
            out_file.write(
                build_markdown(title, project_path, "(none)", stray_entries)
            )
        written += 1

    print(
        f"\nDone. Wrote {written} file(s) "
        f"({len(sessions)} session(s) + {len(no_session_by_project)} no-session bucket(s)) "
        f"into {output_root}"
    )


if __name__ == "__main__":
    main()
