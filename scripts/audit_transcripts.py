#!/usr/bin/env python3
"""Compare what Evertusk shows and exports against the raw Claude Code transcripts in
~/.claude/projects and Codex rollouts in ~/.codex. Run it when Claude Code's format may have changed: anything lost or
leaking shows up here long before a user notices it.

It recounts from the raw files rather than checking for presence: how many of each thing, in what
order, what the list and Stats say, which edits really happened, images present against images
shown, forks, and whether exports stay well formed.

Sessions written in the last two minutes are skipped and counted, because they change while being
compared. Copies are made in a temporary folder and deleted at the end; nothing leaves this machine.

Usage: python3 scripts/audit_transcripts.py    (exits 1 if it finds problems)
"""

import collections
import datetime
import glob
import json
import os
import re
import subprocess
import sys
import tempfile
import time

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PROJECTS_DIR = os.path.expanduser("~/.claude/projects")
QUIET_SECONDS = 120
# A session's date may sit this far from its last message before it counts as wrong.
DATE_TOLERANCE_SECONDS = 60
EDIT_TOOLS = {"Write", "Edit", "MultiEdit", "NotebookEdit"}
NOT_TOOL_PILLS = {"mcp__ccd_turn__update_plan", "mcp__ccd_turn__wrap_up", "mcp__ccd_session__mark_chapter", "TodoWrite"}
DROPPED_BLOCKS = re.compile(
    r"<(system-reminder|browser_instruction|fork-boilerplate|local-command-caveat|local-command-stdout|local-command-stderr)>.*?</\1>",
    re.S,
)
WRAPPERS = re.compile(r"</?pasted_content[^>]*>")
# Terminal colours, links and titles, and other control characters: the app strips them from
# output. A carriage return stays, because it separates words, as progress bars rely on.
ANSI = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\\\)|\x1b\[[0-9;?]*[A-Za-z]|[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")
LEAKED_TAG = re.compile(r"</?(pasted_content|bash-input|bash-stdout|bash-stderr|command-name|command-message|command-args|local-command-\w+|task-notification|agent-message|system-reminder|fork-boilerplate|browser_instruction|tools-used-before-summary)\b")
TRAILING_REMINDERS = re.compile(r"(\s*<system-reminder>(?:(?!<system-reminder>).)*?</system-reminder>)+\s*$", re.S)
MOJIBAKE = re.compile(r"â€.|Ã[©¨ ]|ð\x9f")

checked = collections.Counter()
problems = collections.Counter()
samples = collections.defaultdict(list)


def report(kind, sample=""):
    problems[kind] += 1
    if len(samples[kind]) < 4:
        samples[kind].append(normalize(str(sample))[:180])


def normalize(text):
    return " ".join(text.split())


def text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(block.get("text", "") for block in content if isinstance(block, dict) and block.get("type") == "text")
    return ""


def tool_output_text(content):
    parts = [content] if isinstance(content, str) else [part.get("text", "") for part in content or [] if isinstance(part, dict) and part.get("type") == "text"]
    return "\n".join(ANSI.sub("", TRAILING_REMINDERS.sub("", part)) for part in parts)


def export_form(text):
    """Text as an export can hold it: exports escape raw tags and stray fences and move headings down."""
    return normalize(re.sub(r"[\\\\#]", "", text))


def report_body(text):
    """A subagent's report without the frame Claude Code puts around it for the model."""
    if text.startswith("[Subagent hand-back]") and "The report follows:" in text:
        return text.split("The report follows:", 1)[1]
    return text


def codex_request(text):
    """The person's words in a prompt Codex wrapped with its list of attached files."""
    found = re.search(r"## My request(?: for Codex)?:", text) if text.startswith(("# Files", "# Applications", "## Referenced", "## My request")) else None
    return text[found.end():].strip() if found else text


def clean_user_text(text):
    return ANSI.sub("", WRAPPERS.sub("", DROPPED_BLOCKS.sub("", text)))


def has_tool_result(content):
    return isinstance(content, list) and any(isinstance(block, dict) and block.get("type") == "tool_result" for block in content)


def is_machine_prompt(text):
    return any(marker in text for marker in ("<bash-", "<task-notification", "<command-", "[Request interrupted"))


def entries_of(path):
    with open(path, errors="replace") as transcript:
        for line in transcript:
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            if isinstance(entry, dict):
                yield entry


def parse_time(timestamp):
    try:
        return datetime.datetime.fromisoformat(str(timestamp).replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def compare_dates(tool, listed, raw_first, raw_last):
    """The list's dates against the transcript's own first and last moments."""
    checked["list dates"] += 1
    for label, shown, raw in (("modified", listed.get("modified"), raw_last), ("created", listed.get("created"), raw_first)):
        shown_time, raw_time = parse_time(shown), parse_time(raw)
        if shown_time is None or raw_time is None:
            report(f"{tool}: list has no {label} date", listed["session_id"])
        elif abs(shown_time - raw_time) > DATE_TOLERANCE_SECONDS:
            report(f"{tool}: list {label} date is off", f"{listed['session_id']}: {shown} vs {raw}")


def compare_order(tool, what, raw, shown):
    """Counts first, then order: one thing lost or repeated shouldn't also read as a reorder."""
    checked[f"{tool} {what}"] += len(raw)
    raw_count, shown_count = collections.Counter(raw), collections.Counter(shown)
    lost, repeated = raw_count - shown_count, shown_count - raw_count
    for key, count in lost.items():
        report(f"{tool}: {what} lost", f"x{count} {key}")
    for key, count in repeated.items():
        report(f"{tool}: {what} shown too many times", f"x{count} {key}")
    if not lost and not repeated and raw != shown:
        first_difference = next(index for index, pair in enumerate(zip(raw, shown)) if pair[0] != pair[1])
        report(f"{tool}: {what} out of order", f"at {first_difference}: {raw[first_difference]}")


def fence_on_line(line):
    """CommonMark: up to three spaces, then three or more of one fence character."""
    stripped = line.lstrip(" ")
    if len(line) - len(stripped) > 3 or stripped[:1] not in ("`", "~"):
        return None
    length = len(stripped) - len(stripped.lstrip(stripped[0]))
    if length < 3:
        return None
    info = stripped[length:]
    return None if stripped[0] == "`" and "`" in info else (stripped[0], length, info)


def audit_export(tool, markdown, messages):
    """An export has to stay a readable document: every fence closed, every turn's heading still a
    heading, nothing swallowed by a stray fence or an unclosed fold, and the right agent named."""
    checked[f"{tool} exports"] += 1
    agent_label, other_label = ("Codex", "Claude") if tool == "codex" else ("Claude", "Codex")
    body = markdown.split("\n## Agent transcripts\n", 1)[0]
    headings = collections.Counter()
    open_fence = None
    folds = 0
    for line in body.split("\n"):
        fence = fence_on_line(line)
        if open_fence:
            if fence and fence[0] == open_fence[0] and fence[1] >= open_fence[1] and not fence[2].strip():
                open_fence = None
            continue
        if fence:
            open_fence = fence
            continue
        found = re.match(r"## (You \(mid-turn\)|You ran|You|Claude|Codex) — ", line)
        if found:
            headings[found.group(1)] += 1
        # Only the app's own: a message can mention the tag in passing.
        folds += (line == "<details>") - (line == "</details>")
    if open_fence:
        report(f"{tool}: export has a fence nothing closes")
    if folds:
        report(f"{tool}: export has unbalanced folds", folds)
    roles = collections.Counter(message["role"] for message in messages)
    you = headings["You"] + headings["You (mid-turn)"]
    if you != roles["user"]:
        report(f"{tool}: export lost prompt headings", f"{you} vs {roles['user']}")
    if headings[agent_label] != roles["assistant"]:
        report(f"{tool}: export lost agent headings", f"{headings[agent_label]} vs {roles['assistant']}")
    if headings[other_label]:
        report(f"{tool}: export names the wrong agent", headings[other_label])
    if headings["You ran"] != roles["shell"]:
        report(f"{tool}: export lost shell headings", f"{headings['You ran']} vs {roles['shell']}")


def audit_fork_marks(tool, messages, stats, is_fork, listed, local_commands=()):
    """A fork's copied history comes first, marked, with one divider before its own work."""
    inherited = [index for index, message in enumerate(messages) if message.get("inherited")]
    dividers = [index for index, message in enumerate(messages) if (message.get("event") or {}).get("kind") == "fork"]
    if not is_fork:
        if inherited or dividers:
            report(f"{tool}: not a fork, but shown as one", listed.get("session_id") if listed else "")
        return
    checked[f"{tool} forks"] += 1
    if listed and not listed.get("forked_from_session_id"):
        report(f"{tool}: fork not marked in the list", listed["session_id"])
    if len(dividers) > 1:
        report(f"{tool}: fork has several dividers", len(dividers))
    if inherited and not dividers and len(inherited) != len(messages):
        report(f"{tool}: fork has no divider", (listed or {}).get("session_id"))
    if dividers and inherited and max(inherited) > dividers[0]:
        report(f"{tool}: inherited message after the fork's divider", (listed or {}).get("session_id"))
    inherited_prompts = sum(1 for index in inherited if messages[index]["role"] == "user" and messages[index]["text"].split(" ")[0] not in local_commands)
    if inherited_prompts != stats["inherited_prompts"]:
        report(f"{tool}: inherited prompts differ", f"{stats['inherited_prompts']} vs {inherited_prompts} marked")


def audit_edit_entries(tool, files, raw_calls, rejected):
    """raw_calls: tool_use_id -> (path, entries expected). Anything shown that no call made is phantom."""
    shown_entries = collections.Counter()
    for change in files:
        stamps = [edit["timestamp"] for edit in change["edits"] if edit["timestamp"]]
        if stamps != sorted(stamps):
            report(f"{tool}: edits out of time order", change["path"])
        for edit in change["edits"]:
            if edit["action"] == "bash":
                continue
            checked[f"{tool} edit entries"] += 1
            call_id = edit["tool_use_id"]
            shown_entries[call_id] += 1
            if not edit["timestamp"]:
                report(f"{tool}: edit entry with no time", change["path"])
            if call_id not in raw_calls:
                report(f"{tool}: phantom edit", f"{change['path']} {call_id}")
            elif raw_calls[call_id][0] != change["path"]:
                report(f"{tool}: edit under the wrong file", f"{change['path']} vs {raw_calls[call_id][0]}")
            if edit["failed"] != (call_id in rejected):
                report(f"{tool}: rejected edit marked wrongly", f"{change['path']} {call_id} failed={edit['failed']}")
    for call_id, (path, expected) in raw_calls.items():
        if shown_entries[call_id] != expected:
            report(f"{tool}: edit count differs", f"{path}: {shown_entries[call_id]} vs {expected}")


def own_lines_stats(transcript, own_id):
    """Recount a Claude transcript's stats from its lines, without the app's fast paths."""
    tokens_by_request = {}
    tool_counts = collections.Counter()
    tool_ids = set()
    tally = {"thinking": 0, "active_ms": 0, "durations": 0, "untimed": False, "cost": collections.defaultdict(float), "cost_output": collections.defaultdict(float), "has_usage": False}
    hook_runs = collections.Counter()
    hook_failures = collections.Counter()
    stop_summary = [0, 0]
    for entry in entries_of(transcript):
        kind = entry.get("type")
        if kind == "cost-state":
            run = entry.get("startTime", 0)
            tally["cost"][run] = max(tally["cost"][run], entry.get("totalCostUSD") or 0)
            usage = entry.get("modelUsage")
            if isinstance(usage, dict):
                tally["has_usage"] = True
                tally["cost_output"][run] = max(tally["cost_output"][run], sum((model.get("outputTokens") or 0) for model in usage.values()))
            continue
        if entry.get("forkedFrom") or entry.get("sessionId", own_id) != own_id:
            continue
        if kind == "assistant":
            message = entry.get("message") or {}
            if entry.get("entrypoint", "cli") != "cli":
                tally["untimed"] = True
            if entry.get("requestId") and isinstance(message.get("usage"), dict):
                tokens_by_request[entry["requestId"]] = message["usage"]
            for block in message.get("content") if isinstance(message.get("content"), list) else []:
                if block.get("type") == "tool_use" and block.get("id") not in tool_ids:
                    tool_ids.add(block.get("id"))
                    tool_counts[block["name"]] += 1
                elif block.get("type") == "thinking" and (block.get("thinking") or "").strip():
                    tally["thinking"] += 1
        elif kind == "system" and entry.get("subtype") == "turn_duration":
            tally["durations"] += 1
            tally["active_ms"] += entry.get("durationMs") or 0
        elif kind == "system" and entry.get("subtype") == "stop_hook_summary":
            stop_summary[0] += entry.get("hookCount") or 0
            stop_summary[1] += len(entry.get("hookErrors") or [])
        elif kind == "attachment":
            attachment = entry.get("attachment") or {}
            attachment_type = str(attachment.get("type"))
            if "hook" in attachment_type:
                event = attachment.get("hookEvent") or "unknown"
                hook_runs[event] += 1
                if (attachment.get("exitCode") or 0) != 0 or any(word in attachment_type for word in ("error", "block", "cancel")):
                    hook_failures[event] += 1
    if stop_summary != [0, 0]:
        # Stop hooks are recorded twice: once in the summary, once each. One run, whichever is fuller.
        hook_runs["Stop"] = max(hook_runs["Stop"], stop_summary[0])
        hook_failures["Stop"] = max(hook_failures["Stop"], stop_summary[1])
    return tokens_by_request, tool_counts, tally, hook_runs, hook_failures


def audit_claude_stats(transcript, own_id, dump):
    stats = dump["stats"]
    tokens_by_request, tool_counts, tally, hook_runs, hook_failures = own_lines_stats(transcript, own_id)
    checked["stats recounted"] += 1
    token_fields = {"input_tokens": "input_tokens", "output_tokens": "output_tokens", "cache_read_tokens": "cache_read_input_tokens", "cache_creation_tokens": "cache_creation_input_tokens"}
    for shown, raw in token_fields.items():
        total = sum(usage.get(raw) or 0 for usage in tokens_by_request.values())
        if stats[shown] != total:
            report(f"claude: {shown} differ", f"{own_id}: {stats[shown]} vs {total}")
    if stats["assistant_count"] != len(tokens_by_request) and len(tokens_by_request) > stats["assistant_count"]:
        report("claude: API requests undercounted", f"{own_id}: {stats['assistant_count']} vs {len(tokens_by_request)}")
    shown_tools = {tool["name"]: tool["count"] for tool in stats["tool_counts"]}
    if shown_tools != dict(tool_counts):
        report("claude: tool counts differ", f"{own_id}: {sorted((shown_tools.items() ^ tool_counts.items()))[:4]}")
    if stats["thinking_block_count"] != tally["thinking"]:
        report("claude: thinking count differs", f"{own_id}: {stats['thinking_block_count']} vs {tally['thinking']}")
    if stats["active_ms"] != tally["active_ms"]:
        report("claude: active time differs", f"{own_id}: {stats['active_ms']} vs {tally['active_ms']}")
    if (tally["untimed"] or (tally["durations"] == 0 and tokens_by_request)) and tally["active_ms"] >= 0 and not stats["active_is_partial"]:
        report("claude: partial active time isn't flagged", own_id)
    if tally["cost"]:
        cost = sum(tally["cost"].values())
        if abs((stats.get("cost_usd") or 0) - cost) > 0.005:
            report("claude: cost differs", f"{own_id}: {stats.get('cost_usd')} vs {cost}")
        covered = sum(tally["cost_output"].values())
        output = sum(usage.get("output_tokens") or 0 for usage in tokens_by_request.values())
        if tally["has_usage"] and covered < output * 0.8 and not stats["cost_is_partial"]:
            report("claude: a cost covering part of the session isn't flagged", f"{own_id}: {covered} of {output}")
    shown_hooks = {hook["event"]: (hook["runs"], hook["failures"]) for hook in stats["hooks"]}
    raw_hooks = {event: (runs, hook_failures[event]) for event, runs in hook_runs.items()}
    if shown_hooks != raw_hooks:
        report("claude: hook counts differ", f"{own_id}: {sorted(shown_hooks.items() ^ raw_hooks.items())[:4]}")
    agent_output = 0
    own_folder = transcript[:-6] + "/"
    for agent in dump["subagents"]:
        if agent["jsonl_path"].startswith(own_folder):
            per_request = {}
            for entry in entries_of(agent["jsonl_path"]):
                usage = (entry.get("message") or {}).get("usage") if entry.get("type") == "assistant" else None
                if entry.get("requestId") and isinstance(usage, dict):
                    per_request[entry["requestId"]] = usage.get("output_tokens") or 0
            agent_output += sum(per_request.values())
    if stats["agent_output_tokens"] != agent_output:
        report("claude: agents' output tokens differ", f"{own_id}: {stats['agent_output_tokens']} vs {agent_output}")


def audit_claude_edits(transcript, dump):
    """Every edit shown must be a real call in the session or one of its agents, once, and only
    `assistant` lines count: API-record copies of the same call are what made phantoms before."""
    raw_calls = {}
    rejected = set()
    read_counts = collections.Counter()
    seen_reads = set()
    for path in [transcript, *(agent["jsonl_path"] for agent in dump["subagents"])]:
        if not os.path.exists(path):
            continue
        for entry in entries_of(path):
            content = (entry.get("message") or {}).get("content") if isinstance(entry.get("message"), dict) else None
            if not isinstance(content, list):
                continue
            if entry.get("type") == "assistant":
                for block in content:
                    if block.get("type") != "tool_use":
                        continue
                    tool_input = block.get("input") or {}
                    if block["name"] in EDIT_TOOLS:
                        target = tool_input.get("file_path") or tool_input.get("notebook_path")
                        if target:
                            expected = max(len(tool_input.get("edits") or []), 1) if block["name"] == "MultiEdit" else 1
                            raw_calls[block["id"]] = (target, expected)
                    elif block["name"] == "Read" and tool_input.get("file_path") and block["id"] not in seen_reads:
                        seen_reads.add(block["id"])
                        read_counts[tool_input["file_path"]] += 1
            elif entry.get("type") == "user":
                rejected.update(block.get("tool_use_id") for block in content if isinstance(block, dict) and block.get("type") == "tool_result" and block.get("is_error") is True)
    audit_edit_entries("claude", dump["files"], raw_calls, rejected)
    shown_reads = {change["path"]: change["read_count"] for change in dump["files"] if change["read_count"]}
    if shown_reads != dict(read_counts):
        report("claude: read counts differ", sorted((shown_reads.items() ^ read_counts.items()))[:3])


def audit_claude_recount(transcript, dump, markdown, listed):
    """Counts and order against the raw lines. Presence alone is what let doubled edits, dropped
    repeats of a prompt and wrong dates through before."""
    own_id = os.path.basename(transcript)[:-6]
    messages = dump["messages"]
    raw_blocks = []
    seen_blocks = set()
    prompts = []
    first_time = last_time = None
    is_fork = False
    pasted_images = 0
    tool_images = {}
    notifications = set()
    interrupts = 0
    interrupt_times = []
    local_commands = collections.Counter()
    after_local_notice = False
    seen_retries = set()
    retries = 0
    file_position = {}
    for line_number, entry in enumerate(entries_of(transcript)):
        kind = entry.get("type")
        file_position[entry.get("timestamp")] = line_number
        if kind in ("user", "assistant", "system", "attachment") and parse_time(entry.get("timestamp")) is not None:
            first_time = first_time or entry["timestamp"]
            last_time = entry["timestamp"]
        if kind in ("user", "assistant") and (entry.get("forkedFrom") or entry.get("sessionId", own_id) != own_id):
            is_fork = True
        if entry.get("isSidechain"):
            continue
        message = entry.get("message") if isinstance(entry.get("message"), dict) else {}
        content = message.get("content")
        if kind == "assistant" and isinstance(content, list) and message.get("model") != "<synthetic>":
            for block in content:
                block_type = block.get("type")
                if block_type == "tool_use" and block["name"] not in NOT_TOOL_PILLS:
                    key = ("tool", block["id"])
                elif block_type == "text" and block["text"].strip():
                    key = ("text", normalize(block["text"])[:200])
                elif block_type == "thinking" and block.get("thinking", "").strip():
                    key = ("thinking", normalize(block["thinking"])[:200])
                else:
                    continue
                # One request's blocks can be written on several lines, or written twice.
                unique = (message.get("id"), key)
                if unique not in seen_blocks:
                    seen_blocks.add(unique)
                    raw_blocks.append(key)
        elif kind == "user":
            if isinstance(content, list):
                for block in content:
                    if not isinstance(block, dict):
                        continue
                    if block.get("type") == "tool_result" and isinstance(block.get("content"), list):
                        images = sum(1 for part in block["content"] if isinstance(part, dict) and part.get("type") == "image")
                        tool_images[block.get("tool_use_id")] = tool_images.get(block.get("tool_use_id"), 0) + images
                    elif block.get("type") == "image" and not entry.get("isMeta"):
                        pasted_images += 1
            text = text_of(content)
            if entry.get("isMeta"):
                after_local_notice = "<local-command-caveat>" in text
            else:
                command = re.search(r"<command-name>(.*?)</command-name>", text)
                if after_local_notice and command:
                    local_commands[command.group(1).strip()] += 1
                elif text.strip() and entry.get("parentUuid") and not is_machine_prompt(text) and not has_tool_result(content):
                    retry_key = (entry["parentUuid"], normalize(clean_user_text(text)))
                    retries += retry_key in seen_retries
                    seen_retries.add(retry_key)
                after_local_notice = False
            if "[Request interrupted" in text:
                interrupts += 1
                interrupt_times.append(entry.get("timestamp"))
            found_task = re.search(r"<task-id>(.*?)</task-id>.*?<status>(.*?)</status>", text, re.S)
            if found_task and "<task-notification" in text:
                notifications.add(found_task.groups())
            if entry.get("isMeta") or entry.get("isCompactSummary") or (entry.get("origin") or {}).get("kind") == "peer":
                continue
            if text.strip() and not is_machine_prompt(text)  and not has_tool_result(content):
                prompts.append(normalize(clean_user_text(text))[:300])
        elif kind == "attachment":
            attachment = entry.get("attachment") or {}
            prompt = attachment.get("prompt")
            if attachment.get("type") == "queued_command" and isinstance(prompt, str) and not is_machine_prompt(prompt) and (attachment.get("origin") or {}).get("kind") != "peer":
                prompts.append(normalize(clean_user_text(prompt))[:300])
            if attachment.get("type") == "queued_command" and isinstance(prompt, list):
                pasted_images += sum(1 for block in prompt if isinstance(block, dict) and block.get("type") == "image")

    shown_blocks = []
    for message in messages:
        for block in message.get("blocks") or []:
            if block["kind"] == "tool":
                shown_blocks.append(("tool", block.get("tool_use_id")))
            elif block["kind"] in ("text", "thinking"):
                shown_blocks.append((block["kind"], normalize(block["text"])[:200]))
    compare_order("claude", "agent blocks", raw_blocks, shown_blocks)

    # Each prompt must appear at or after where the one before it did. A repeat right beside its
    # own copy is one prompt delivered twice (queued then typed, or a retry); one further off is real.
    bubbles = [normalize(ANSI.sub("", message["text"])) for message in messages if message["role"] == "user"]
    position = 0
    previous = None
    checked["claude prompts in order"] += len(prompts)
    for wanted in prompts:
        if not wanted:
            continue
        found = next((index for index in range(position, len(bubbles)) if wanted[:200] in bubbles[index]), None)
        if found is not None:
            position = found + 1
        elif wanted != previous:
            report("claude: prompt lost or out of order", wanted)
        previous = wanted

    if listed:
        compare_dates("claude", listed, first_time, last_time)
        if listed["conversation_count"] != dump["stats"]["user_prompt_count"]:
            report("claude: list and Stats count prompts differently", f"{own_id}: {listed['conversation_count']} vs {dump['stats']['user_prompt_count']}")
    else:
        report("claude: session isn't listed", own_id)
    # Local commands like /model show as a bubble but aren't prompts. A retry is counted once.
    own_bubbles = sum(1 for message in messages if message["role"] == "user" and not message.get("inherited") and message["text"].split(" ")[0] not in local_commands)
    if own_bubbles - retries > dump["stats"]["user_prompt_count"] or own_bubbles < dump["stats"]["user_prompt_count"]:
        report("claude: prompt count differs from bubbles", f"{own_id}: {dump['stats']['user_prompt_count']} vs {own_bubbles} less {retries} retries")
    if retries and own_bubbles > dump["stats"]["user_prompt_count"]:
        report("claude: a retried prompt shows twice", f"{own_id}: {own_bubbles - dump['stats']['user_prompt_count']}")

    audit_claude_stats(transcript, own_id, dump)
    audit_claude_edits(transcript, dump)
    audit_fork_marks("claude", messages, dump["stats"], is_fork, listed, local_commands)
    audit_export("claude", markdown, messages)

    shown_pasted = sum(len(message.get("images") or []) for message in messages)
    checked["claude pasted images"] += pasted_images
    if shown_pasted != pasted_images:
        report("claude: pasted images differ", f"{own_id}: {shown_pasted} shown of {pasted_images}")
    for tool_id, images in tool_images.items():
        checked["claude tool images"] += images
        shown = (dump["results"].get(tool_id) or {}).get("image_count", 0)
        if shown != images:
            report("claude: tool images differ", f"{tool_id}: {shown} shown of {images}")

    cards = sum(1 for message in messages if message["role"] == "agent-notification")
    checked["claude background runs"] += len(notifications)
    if cards < len(notifications):
        report("claude: background runs collapsed", f"{own_id}: {cards} cards for {len(notifications)} runs")
    checked["claude interrupts"] += interrupts
    # Each marker must be visible: its own "Interrupted" row, or the reply it cut off marked.
    # Two Esc presses on one reply rightly share that reply's mark.
    for marker_time in interrupt_times:
        earlier = [message for message in messages if (parse_time(message["timestamp"]) or 0) <= parse_time(marker_time) and message["role"] in ("assistant", "system-event", "shell", "user", "compaction")]
        recent_row = any(message["role"] == "system-event" and "nterrupt" in message["text"] for message in earlier[-6:])
        replies = [message for message in earlier if message["role"] == "assistant"]
        if not recent_row and not (replies and replies[-1].get("interrupted")):
            report("claude: interrupt with no mark", f"{own_id}: {marker_time}")

    # A queued prompt really was sent before the reply it's written after, so only a card out of
    # step with the file's own order counts.
    stamps = [message["timestamp"] for message in messages if parse_time(message.get("timestamp")) is not None and not message.get("inherited")]
    for earlier, later in zip(stamps, stamps[1:]):
        if parse_time(later) < parse_time(earlier) - DATE_TOLERANCE_SECONDS and file_position.get(later, 0) < file_position.get(earlier, 0):
            report("claude: time runs backwards", f"{own_id}: {earlier} then {later}")
            break


def audit_session(transcript, dump, markdown, listed):
    messages = dump["messages"]
    shown_text = normalize("\n".join(message["text"] for message in messages))
    exported = export_form(markdown)
    prose_blocks = {normalize(block["text"]) for message in messages for block in message.get("blocks", []) if block["kind"] in ("text", "thinking")}
    tool_ids = {block.get("tool_use_id") for message in messages for block in message.get("blocks", []) if block["kind"] == "tool"}
    raw_text = open(transcript, errors="replace").read()

    for message in messages:
        # The whole text, not its start: only a tag the raw transcript doesn't hold there as typed text.
        if message["role"] in ("user", "compaction") and any(tag.group(0) not in clean_user_text(raw_text) for tag in LEAKED_TAG.finditer(message["text"])):
            report("machine tag shown as text", message["text"][:300])
        for garbled in MOJIBAKE.findall(message["text"]):
            if garbled not in raw_text:
                report("garbled text the source doesn't have", message["text"])
        if message["role"] in ("user", "assistant") and not message["text"].strip() and not message.get("images"):
            report("empty bubble", message["role"])

    tool_names = {}
    edited_files = set()
    for entry in entries_of(transcript):
        if entry.get("isSidechain"):
            continue
        message = entry.get("message") if isinstance(entry.get("message"), dict) else {}
        content = message.get("content")
        kind = entry.get("type")
        if kind == "assistant" and isinstance(content, list):
            synthetic = message.get("model") == "<synthetic>"
            for block in content:
                block_type = block.get("type")
                if synthetic:
                    if block_type == "text" and block["text"].strip() != "No response requested." and normalize(block["text"]) not in shown_text:
                        report("synthetic message missing", block["text"])
                elif block_type == "text" and block["text"].strip():
                    checked["assistant text"] += 1
                    if normalize(block["text"]) not in prose_blocks:
                        report("assistant text missing", block["text"])
                    if export_form(block["text"])[:400] not in exported:
                        report("assistant text missing from export", block["text"])
                elif block_type == "thinking" and block.get("thinking", "").strip():
                    checked["thinking"] += 1
                    if normalize(block["thinking"]) not in prose_blocks:
                        report("thinking missing", block["thinking"])
                elif block_type == "tool_use":
                    checked["tool calls"] += 1
                    tool_names[block["id"]] = block["name"]
                    if block["name"] in ("Write", "Edit", "MultiEdit", "NotebookEdit"):
                        tool_input = block.get("input") or {}
                        edited_files.add(tool_input.get("file_path") or tool_input.get("notebook_path"))
                    if block["name"] not in NOT_TOOL_PILLS and block["id"] not in tool_ids:
                        report("tool call missing", block["name"])
        elif kind == "user":
            tool_use_result = entry.get("toolUseResult")
            has_results = isinstance(content, list) and any(isinstance(block, dict) and block.get("type") == "tool_result" for block in content)
            if has_results:
                for block in content:
                    if not (isinstance(block, dict) and block.get("type") == "tool_result"):
                        continue
                    checked["tool results"] += 1
                    wanted = normalize(tool_output_text(block.get("content")))[:300]
                    got = dump["results"].get(block["tool_use_id"])
                    if got is None:
                        report("tool result missing", tool_names.get(block["tool_use_id"]))
                    elif wanted and wanted not in normalize(got["content"]) and not shown_as_binary(tool_output_text(block.get("content")), got["content"]):
                        report("tool result differs", tool_names.get(block["tool_use_id"]))
                if isinstance(tool_use_result, dict):
                    for edited in (tool_use_result.get("bashEditDiff") or {}).get("files") or []:
                        edited_files.add(edited.get("filePath"))
                continue
            if entry.get("isCompactSummary"):
                continue
            origin = entry.get("origin") or {}
            if origin.get("kind") == "peer":
                checked["subagent reports"] += 1
                report_text = report_body(origin.get("handbackReportText") or origin.get("body") or "")
                if normalize(report_text)[:200] not in shown_text:
                    report("subagent report missing", report_text)
                continue
            if entry.get("isMeta"):
                continue
            text = text_of(content)
            if text.strip() and not is_machine_prompt(text):
                checked["prompts"] += 1
                wanted = normalize(clean_user_text(text))[:300]
                if wanted and wanted not in shown_text:
                    report("prompt missing", text)
                if wanted and export_form(wanted)[:200] not in exported:
                    report("prompt missing from export", text)
        elif kind == "attachment":
            attachment = entry.get("attachment") or {}
            prompt = attachment.get("prompt")
            if attachment.get("type") == "queued_command" and isinstance(prompt, str) and not is_machine_prompt(prompt) and (attachment.get("origin") or {}).get("kind") != "peer":
                checked["queued prompts"] += 1
                if normalize(clean_user_text(prompt))[:300] not in shown_text:
                    report("queued prompt missing", prompt)

    for questions in dump["questions"].values():
        checked["questions"] += len(questions)
        for question in questions:
            if export_form(question["question"])[:120] not in exported:
                report("question missing from export", question["question"])
    for artifact in dump["artifacts"].values():
        checked["artifacts"] += 1
        if artifact["url"] not in markdown:
            report("artifact missing from export", artifact["url"])

    shown_files = {change["path"] for change in dump["files"]}
    for edited in edited_files:
        checked["edited files"] += 1
        if edited and edited not in shown_files:
            report("edit missing from Files panel", edited)

    audit_claude_recount(transcript, dump, markdown, listed)

    listed_paths = {agent["jsonl_path"] for agent in dump["subagents"]}
    for own_agent in glob.glob(transcript[:-6] + "/subagents/**/agent-*.jsonl", recursive=True):
        checked["subagents"] += 1
        if own_agent not in listed_paths:
            report("subagent not listed", own_agent)
    for listed in dump["subagents"]:
        if not os.path.basename(listed["jsonl_path"]).startswith("agent-"):
            report("non-agent file listed as an agent", listed["jsonl_path"])

    for subagent in dump["subagent_transcripts"]:
        audit_subagent(subagent)


def codex_entries(path):
    """A rollout's parsed lines. Reading .zst needs Python 3.14+ or the zstd command."""
    if path.endswith(".zst"):
        try:
            from compression import zstd
            with zstd.open(path, "rt", encoding="utf-8") as compressed:
                raw_lines = compressed.readlines()
        except ImportError:
            raw_lines = subprocess.run(["zstd", "-dc", path], capture_output=True, text=True, check=True).stdout.splitlines()
    else:
        with open(path, encoding="utf-8") as plain:
            raw_lines = plain.readlines()
    entries = []
    for line in raw_lines:
        try:
            entries.append(json.loads(line))
        except json.JSONDecodeError:
            report("codex: unreadable line")
    return entries


def shown_as_binary(raw, shown):
    """Raw bytes in tool output (a PNG sent to stdout) are replaced by a note, by design. The
    note only counts if the raw text really is binary, using the app's own rule."""
    binary = "\0" in raw or raw.count("\ufffd") * 100 > max(len(raw), 800)
    return binary and shown.startswith("[Binary output of")


def codex_output_text(output):
    if isinstance(output, list):
        return "".join(part.get("text", "") for part in output if isinstance(part, dict))
    return output if isinstance(output, str) else json.dumps(output or "")


def audit_codex_recount(entries, dump, markdown, listed, meta, items):
    """Counts and order against the raw file, for the same things the Claude audit recounts."""
    session_id = meta.get("id")
    messages = dump["messages"]
    stats = dump["stats"]
    is_item = lambda entry: entry.get("type") == "event_msg" and entry["payload"].get("type") == "item_completed"

    # Answers appear in order. A fork's parent history and a retried line can repeat text, so
    # this is a subsequence walk rather than an exact match.
    shown_answers = [normalize(block["text"]) for message in messages for block in message.get("blocks") or [] if block.get("kind") == "text"]
    position = 0
    for item in items:
        if item.get("type") != "AgentMessage":
            continue
        text = normalize("\n".join(part.get("text", "") for part in item.get("content", [])))
        if not text:
            continue
        checked["codex answers in order"] += 1
        found = next((index for index in range(position, len(shown_answers)) if text in shown_answers[index]), None)
        if found is None:
            report("codex: answer lost or out of order", text[:120])
        else:
            position = found

    if listed:
        real_times = [entry.get("timestamp") for entry in entries if parse_time(entry.get("timestamp")) is not None]
        message_times = [entry["timestamp"] for entry in entries if is_item(entry) and entry["payload"]["item"].get("type") in ("AgentMessage", "UserMessage") and parse_time(entry.get("timestamp")) is not None]
        if real_times:
            compare_dates("codex", listed, real_times[0], message_times[-1] if message_times else listed.get("modified"))
        if listed["conversation_count"] != stats["user_prompt_count"]:
            report("codex: list and Stats count prompts differently", f"{session_id}: {listed['conversation_count']} vs {stats['user_prompt_count']}")
    own_bubbles = sum(1 for message in messages if message["role"] == "user" and not message.get("inherited"))
    if own_bubbles != stats["user_prompt_count"]:
        report("codex: prompt count differs from bubbles", f"{session_id}: {stats['user_prompt_count']} vs {own_bubbles}")
    audit_fork_marks("codex", messages, stats, bool(meta.get("forked_from_id")), listed)
    audit_export("codex", markdown, messages)

    # Every image a person attached is shown or explained.
    attached = sum(1 for item in items if item.get("type") == "UserMessage" for part in item.get("content", []) if part.get("type") in ("image", "local_image"))
    shown_images = sum(len(message.get("images") or []) for message in messages if message["role"] == "user")
    explained = sum(len(re.findall(r"\[Attached (?:image|file)[^\]]*\]", message["text"])) for message in messages if message["role"] == "user")
    checked["codex attached images"] += attached
    if shown_images + explained < attached:
        report("codex: attached images lost", f"{session_id}: {shown_images} shown, {explained} explained, of {attached}")
    if meta.get("forked_from_id") is None and shown_images > attached and attached:
        report("codex: more attached images shown than attached", f"{session_id}: {shown_images} of {attached}")

    # Pictures in tool output: the raw call's output against the count its pill offers.
    raw_output_images = collections.Counter()
    for entry in entries:
        payload = entry.get("payload") or {}
        if entry.get("type") == "response_item" and payload.get("type") in ("function_call_output", "custom_tool_call_output") and isinstance(payload.get("output"), list):
            raw_output_images[payload.get("call_id")] += sum(1 for part in payload["output"] if isinstance(part, dict) and "image" in str(part.get("type")))
    results = dump["results"]
    shown_output_images = sum(result.get("image_count", 0) for result in results.values())
    checked["codex tool images"] += sum(raw_output_images.values())
    if shown_output_images < sum(raw_output_images.values()):
        report("codex: tool images lost", f"{session_id}: {shown_output_images} offered of {sum(raw_output_images.values())}")

    # Edits: each shown entry comes from a call that exists, and a failed patch says so.
    # Changes also include what this session's helpers wrote.
    raw_ids = {item.get("id") for item in items}
    for source in [entries, *(codex_entries(agent["jsonl_path"]) for agent in dump["subagents"])]:
        raw_ids |= {(entry.get("payload") or {}).get("call_id") for entry in source if entry.get("type") == "response_item"}
        raw_ids |= {entry["payload"]["item"].get("id") for entry in source if is_item(entry)}
    seen_entries = collections.Counter()
    for change in dump["files"]:
        for edit in change["edits"]:
            checked["codex edit entries"] += 1
            if edit["tool_use_id"] not in raw_ids:
                report("codex: phantom edit", f"{change['path']} {edit['tool_use_id']}")
            seen_entries[(change["path"], edit["tool_use_id"], edit["action"], edit["old_string"], edit["new_string"])] += 1
            result = results.get(edit["tool_use_id"])
            if result is not None and edit["failed"] != result["is_error"]:
                report("codex: rejected edit marked wrongly", f"{change['path']} {edit['tool_use_id']}")
    for key, count in seen_entries.items():
        if count > 1:
            report("codex: edit shown twice", key[0])


def audit_codex_session(entries, dump, markdown, listed_sessions):
    """Start from the raw file and account for everything in it, so a record the reader
    doesn't know about shows up here rather than silently disappearing."""
    is_item = lambda entry: entry.get("type") == "event_msg" and entry["payload"].get("type") == "item_completed"
    meta = next((entry["payload"] for entry in entries if entry.get("type") == "session_meta"), {})
    items = [entry["payload"]["item"] for entry in entries if is_item(entry)]
    blocks = [block for message in dump["messages"] for block in message.get("blocks") or []]
    shown_tools = collections.Counter(block.get("tool_use_id") for block in blocks if block.get("kind") == "tool")
    shown_text = normalize("\n".join(block["text"] for block in blocks if block.get("kind") == "text"))
    shown_thinking = normalize("\n".join(block["text"] for block in blocks if block.get("kind") == "thinking"))
    shown_notes = normalize("\n".join(block.get("text", "") for block in blocks if block.get("kind") == "note"))
    user_messages = [normalize(ANSI.sub("", message["text"])) for message in dump["messages"] if message["role"] == "user"]
    shown_user = "\n".join(user_messages)
    item_ids = {item.get("id") for item in items}
    results = dump["results"]
    exported = export_form(markdown)

    is_subagent = isinstance(meta.get("source"), dict) and "subagent" in meta["source"]
    prompts = [normalize(" ".join(part.get("text", "") for part in item.get("content", []) if part.get("type") == "text")) for item in items if item.get("type") == "UserMessage"]
    real_prompts = [prompt for prompt in prompts if prompt and not prompt.startswith("<")]
    if real_prompts and not is_subagent and meta.get("id") not in listed_sessions:
        report("codex: a conversation with prompts isn't listed", real_prompts[0][:120])

    for item in items:
        kind = item.get("type")
        checked[f"codex {kind}"] += 1
        if kind == "AgentMessage":
            text = normalize("\n".join(part.get("text", "") for part in item.get("content", [])))
            if text and text not in shown_text:
                report("codex: agent message missing", text[:120])
            elif text and export_form(text)[:200] not in exported:
                report("codex: agent message missing from export", text[:120])
        elif kind == "UserMessage":
            text = normalize(ANSI.sub("", "\n".join(part.get("text", "") for part in item.get("content", []) if part.get("type") == "text")))
            if text and not text.startswith("<") and codex_request(text)[:200] not in shown_user:
                report("codex: prompt missing", text[:120])
        elif kind == "Plan":
            if normalize(item.get("text", "")) not in shown_notes:
                report("codex: proposed plan missing", item.get("id"))
        elif kind in ("CommandExecution", "FileChange", "McpToolCall", "WebSearch", "ImageView", "Extension", "DynamicToolCall"):
            if shown_tools[item.get("id")] != 1:
                report(f"codex: {kind} shown {shown_tools[item.get('id')]} times", str(item.get("id")))
            if kind == "CommandExecution":
                result = results.get(item.get("id")) or {}
                if (item.get("exit_code") or 0) != 0 and not result.get("is_error"):
                    report("codex: failed command not marked failed", str(item.get("id")))
                output = normalize(ANSI.sub("", item.get("aggregated_output") or item.get("stdout") or ""))
                if output and output[:200] not in normalize(result.get("content", "")) and not shown_as_binary(item.get("aggregated_output") or item.get("stdout") or "", result.get("content", "")):
                    report("codex: command output differs", str(item.get("id")))

    # Every raw call must be accounted for: shown once, or a script wrapper whose commands
    # appear as their own items.
    outputs = {}
    open_scripts = []
    scripts_with_items = set()
    last_closed_script = []
    for entry in entries:
        payload = entry.get("payload") or {}
        if is_item(entry) and payload["item"].get("type") in ("CommandExecution", "FileChange", "McpToolCall", "WebSearch", "ImageView", "Extension", "DynamicToolCall"):
            # An item can land just after its script's output, so the last one to close still claims it.
            scripts_with_items.update(open_scripts or last_closed_script)
        elif entry.get("type") == "response_item":
            if payload.get("type") in ("function_call", "custom_tool_call") and payload.get("name") in ("exec", "wait"):
                open_scripts.append(payload.get("call_id"))
            elif payload.get("type") in ("function_call_output", "custom_tool_call_output"):
                outputs[payload.get("call_id")] = payload.get("output")
                if payload.get("call_id") in open_scripts:
                    open_scripts.remove(payload.get("call_id"))
                    last_closed_script = [payload.get("call_id")]

    edited = set()
    cwd = meta.get("cwd", "")
    for entry in entries:
        payload = entry.get("payload") or {}
        if entry.get("type") == "response_item" and payload.get("type") == "reasoning":
            for part in payload.get("summary") or []:
                checked["codex reasoning summary"] += 1
                if normalize(part.get("text", "")) not in shown_thinking:
                    report("codex: reasoning summary missing", part.get("text", "")[:120])
        if entry.get("type") != "response_item" or payload.get("type") not in ("function_call", "custom_tool_call"):
            continue
        name, call_id = payload.get("name"), payload.get("call_id")
        checked["codex raw call"] += 1
        if name in ("update_plan", "request_user_input", "request_user_input_async"):
            continue
        if call_id in item_ids or call_id in scripts_with_items:
            if call_id not in scripts_with_items and name != "request_user_input_async" and shown_tools[call_id] != 1:
                report(f"codex: {name} with an item shown {shown_tools[call_id]} times", call_id)
            continue
        if shown_tools[call_id] != 1:
            report(f"codex: {name} shown {shown_tools[call_id]} times", call_id)
            continue
        result = results.get(call_id)
        if result is None:
            report(f"codex: {name} has no result", call_id)
            continue
        raw_output = codex_output_text(outputs.get(call_id))
        # A script's own output can mention exit codes of anything, so only a shell's counts.
        for marker in () if name in ("exec", "wait", "js") else ("Process exited with code ", "Exit code: ", '"exit_code":'):
            found = re.search(re.escape(marker) + r"\s*(-?\d+)", raw_output[:3000])
            if found:
                if (int(found.group(1)) != 0) != result["is_error"]:
                    report("codex: exit code and failure mark disagree", f"{name} {call_id} {found.group(1)}")
                break
        if raw_output.lstrip().startswith(("Command:", "Chunk ID:", "Exit code:", "Script ", "Wall time")):
            body = raw_output.split("\nOutput:", 1)[1] if "\nOutput:" in raw_output else raw_output.split("\n", 1)[-1]
            tail = normalize(ANSI.sub("", body))[-60:]
            if tail and tail not in normalize(ANSI.sub("", result["content"])) and not shown_as_binary(body, result["content"]):
                report("codex: tool output differs", f"{name} {call_id}")
        patch = payload.get("input") or ""
        if "*** Begin Patch" in patch and not result["is_error"]:
            for path in re.findall(r"^\*\*\* (?:Add|Update|Delete) File: (.+)$", patch, re.M):
                edited.add(path.strip())

    shown_files = [change["path"] for change in dump["files"]]
    for path in edited:
        if not any(file == path or file.endswith("/" + path) for file in shown_files):
            report("codex: a patched file is missing from Changes", path)
    # A fork's Changes hold its own work, so what it inherited isn't expected there.
    inherited_tools = {block.get("tool_use_id") for message in dump["messages"] if message.get("inherited") for block in message.get("blocks") or []}
    for item in items:
        if item.get("type") == "FileChange" and item.get("status") != "failed" and item.get("id") not in inherited_tools:
            for path in item.get("changes", {}):
                if path not in shown_files:
                    report("codex: edited file missing from Changes", path)

    usage = [entry["payload"]["info"]["total_token_usage"] for entry in entries if entry.get("type") == "event_msg" and entry["payload"].get("type") == "token_count" and (entry["payload"].get("info") or {}).get("total_token_usage")]
    stats = dump["stats"]
    # A fork's running total starts with its parent's, which isn't its own work.
    is_fork = bool(meta.get("forked_from_id"))
    if usage and (stats["output_tokens"] > usage[-1]["output_tokens"] if is_fork else stats["output_tokens"] != usage[-1]["output_tokens"]):
        report("codex: token count differs", f"{stats['output_tokens']} vs {usage[-1]['output_tokens']}")
    if stats.get("cost_usd") is not None:
        report("codex: a cost was claimed")
    audit_codex_recount(entries, dump, markdown, listed_sessions.get(meta.get("id")), meta, items)


def audit_subagent(subagent):
    shown_text = normalize("\n".join(message["text"] for message in subagent["messages"]))
    prose_blocks = {normalize(block["text"]) for message in subagent["messages"] for block in message.get("blocks", []) if block["kind"] in ("text", "thinking")}
    saw_brief = False
    for entry in entries_of(subagent["path"]):
        message = entry.get("message") if isinstance(entry.get("message"), dict) else {}
        content = message.get("content")
        if entry.get("type") == "assistant" and isinstance(content, list) and message.get("model") != "<synthetic>":
            for block in content:
                if block.get("type") == "text" and block["text"].strip():
                    checked["subagent text"] += 1
                    if normalize(block["text"]) not in prose_blocks:
                        report("subagent text missing", block["text"])
        elif entry.get("type") == "user" and not saw_brief:
            saw_brief = True
            brief = normalize(clean_user_text(text_of(content)))[:150]
            if brief and brief not in shown_text:
                report("subagent brief missing", brief)


def main():
    now = time.time()
    is_quiet = lambda path: now - os.path.getmtime(path) > QUIET_SECONDS
    all_claude = glob.glob(PROJECTS_DIR + "/*/*.jsonl")
    all_codex = [
        path
        for folder in ("sessions", "archived_sessions")
        for path in glob.glob(os.path.expanduser(f"~/.codex/{folder}/**/rollout-*"), recursive=True)
    ]
    transcripts = [path for path in all_claude if is_quiet(path)]
    codex_transcripts = [path for path in all_codex if is_quiet(path)]
    skipped = len(all_claude) + len(all_codex) - len(transcripts) - len(codex_transcripts)
    with tempfile.TemporaryDirectory() as output_dir:
        subprocess.run(
            ["cargo", "run", "--quiet", "--example", "dump_sessions", "--", output_dir, *transcripts, *codex_transcripts],
            cwd=os.path.join(REPO_ROOT, "src-tauri"),
            check=True,
        )
        with open(f"{output_dir}/_listed.json") as listing_file:
            listed_sessions = {session["session_id"]: session for session in json.load(listing_file)}
        for transcript in transcripts:
            session_id = os.path.basename(transcript)[:-6]
            with open(f"{output_dir}/{session_id}.json") as dump_file, open(f"{output_dir}/{session_id}.md") as markdown_file:
                audit_session(transcript, json.load(dump_file), markdown_file.read(), listed_sessions.get(session_id))
        for transcript in codex_transcripts:
            session_id = os.path.basename(transcript).removesuffix(".zst").removesuffix(".jsonl")
            dump_name = os.path.basename(transcript).removesuffix(".zst") if transcript.endswith(".zst") else session_id
            with open(f"{output_dir}/{dump_name}.json") as dump_file, open(f"{output_dir}/{dump_name}.md") as markdown_file:
                audit_codex_session(codex_entries(transcript), json.load(dump_file), markdown_file.read(), listed_sessions)

    print(f"Audited {len(transcripts)} Claude and {len(codex_transcripts)} Codex sessions ({skipped} skipped: written in the last {QUIET_SECONDS // 60} minutes):", ", ".join(f"{count} {kind}" for kind, count in checked.most_common()))
    if not problems:
        print("No problems found.")
        return 0
    print(f"{sum(problems.values())} problem(s):")
    for kind, count in problems.most_common():
        print(f"\n[{count}] {kind}")
        for sample in samples[kind]:
            print("   ·", sample)
    return 1


if __name__ == "__main__":
    sys.exit(main())
