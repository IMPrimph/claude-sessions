#!/usr/bin/env python3
"""Writes src/demoData.json, the made-up sessions behind the website's clickable demo.

Everything here is invented. The sessions are written as real transcripts in each tool's own
format and read by the app's real parser (the dump_sessions example), so the demo shows what
the app would show. Never paste real transcripts in: this ships to every visitor.

Run: npm run demo:data
"""
import base64
import json
import os
import re
import subprocess
import tempfile
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
# Timestamps are written relative to this, then moved to the visitor's "now" by demoBackend.ts.
ANCHOR = datetime(2026, 11, 2, 10, 0, tzinfo=timezone.utc)
CODE = "/Users/alex/code"
CLAUDE_MODEL = "claude-opus-5-5"
CODEX_MODEL = "gpt-6.1-sol"
SCREENSHOT = base64.b64encode((REPO / "scripts/demo-fixtures/pasted-screenshot.png").read_bytes()).decode()


def stable_id(name):
    return str(uuid.uuid5(uuid.NAMESPACE_URL, name))


def jsonl(entries):
    # Both tools write compact JSON, and the app's fast paths look for it.
    return "".join(json.dumps(entry, separators=(",", ":")) + "\n" for entry in entries)


def iso(moment):
    return moment.strftime("%Y-%m-%dT%H:%M:%S.000Z")


class Claude:
    """One Claude Code session, written line by line as ~/.claude/projects stores it."""

    def __init__(self, name, project, branch, ago, title, forked_from=None):
        self.id, self.project, self.branch, self.clock = stable_id(name), project, branch, ANCHOR - ago
        self.lines, self.calls, self.parent, self.forked_from = [], 0, None, forked_from
        self.agents = []
        self.line({"type": "ai-title", "aiTitle": title, "sessionId": self.id})

    def line(self, entry):
        self.lines.append(entry)

    def stamp(self, seconds):
        self.clock += timedelta(seconds=seconds)
        return iso(self.clock)

    def entry(self, kind, message, seconds, **extra):
        uid = stable_id(f"{self.id}-{len(self.lines)}")
        entry = {"type": kind, "uuid": uid, "parentUuid": self.parent, "sessionId": self.id, "cwd": f"{CODE}/{self.project}",
                 "gitBranch": self.branch, "version": "3.1.0", "timestamp": self.stamp(seconds), "message": message, **extra}
        if self.forked_from:
            entry["forkedFrom"] = {"sessionId": self.forked_from, "messageUuid": uid}
        self.parent = uid
        self.line(entry)
        return self

    def you(self, text, image=False, seconds=40):
        content = [{"type": "text", "text": text}]
        if image:
            content.insert(0, {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": SCREENSHOT}})
            content[1]["text"] = "[Image #1] " + text
        return self.entry("user", {"role": "user", "content": content}, seconds)

    def says(self, text, seconds=14, thinking=None):
        content = [{"type": "text", "text": text}]
        if thinking:
            content.insert(0, {"type": "thinking", "thinking": thinking, "signature": "demo"})
        return self.assistant(content, seconds)

    def assistant(self, content, seconds):
        self.calls += 1
        usage = {"input_tokens": 900, "output_tokens": 240 + 60 * len(content), "cache_read_input_tokens": 18000 + 2500 * self.calls, "cache_creation_input_tokens": 400}
        return self.entry("assistant", {"role": "assistant", "id": f"msg_{self.id[:8]}_{self.calls}", "model": CLAUDE_MODEL, "content": content, "usage": usage}, seconds, requestId=f"req_{self.id[:8]}_{self.calls}")

    def tool(self, name, tool_input, result, seconds=9, is_error=False, **tool_use_result):
        self.calls += 1
        call_id = f"toolu_{self.id[:8]}_{self.calls}"
        self.assistant([{"type": "tool_use", "id": call_id, "name": name, "input": tool_input}], seconds)
        extra = {"toolUseResult": tool_use_result} if tool_use_result else {}
        return self.entry("user", {"role": "user", "content": [{"type": "tool_result", "tool_use_id": call_id, "content": result, "is_error": is_error}]}, 3, **extra)

    def read(self, path, body):
        return self.tool("Read", {"file_path": f"{CODE}/{self.project}/{path}"}, body)

    def bash(self, command, output, is_error=False):
        return self.tool("Bash", {"command": command, "description": ""}, output, is_error=is_error)

    def edit(self, path, old, new):
        return self.tool("Edit", {"file_path": f"{CODE}/{self.project}/{path}", "old_string": old, "new_string": new}, f"The file {path} has been updated.")

    def write(self, path, body):
        return self.tool("Write", {"file_path": f"{CODE}/{self.project}/{path}", "content": body}, f"File created at {path}.")

    def stopped(self):
        return self.entry("user", {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user]"}]}, 5)

    def asks(self, question, header, options, chosen, seconds=30):
        offered = [{"label": label, "description": detail} for label, detail in options]
        questions = [{"question": question, "header": header, "multiSelect": False, "options": offered}]
        return self.tool("AskUserQuestion", {"questions": questions}, f'User answered: "{chosen}"', seconds=seconds, questions=questions, answers={question: chosen})

    def agent(self, description, agent_type, prompt, findings, steps):
        agent_id = stable_id(f"{self.id}-agent-{description}")[:17].replace("-", "")
        self.tool("Agent", {"description": description, "subagent_type": agent_type, "prompt": prompt}, findings, seconds=70,
                  agentId=agent_id, status="completed", totalToolUseCount=len(steps), totalTokens=18400, totalDurationMs=64000, content=[{"type": "text", "text": findings}])
        child = Claude(f"{self.id}-child-{agent_id}", self.project, self.branch, ANCHOR - self.clock + timedelta(seconds=70), description)
        child.lines = []
        child.id = self.id
        child.you(prompt, seconds=0)
        for command, output in steps:
            child.bash(command, output)
        child.says(findings)
        for entry in child.lines:
            entry["isSidechain"] = True
            entry["agentId"] = agent_id
        self.agents.append((agent_id, agent_type, description, child.lines))
        return self

    def write_to(self, home):
        folder = home / ".claude/projects" / re.sub(r"[^A-Za-z0-9]", "-", f"{CODE}/{self.project}")
        folder.mkdir(parents=True, exist_ok=True)
        path = folder / f"{self.id}.jsonl"
        path.write_text(jsonl(self.lines))
        for agent_id, agent_type, description, lines in self.agents:
            agents = folder / self.id / "subagents"
            agents.mkdir(parents=True, exist_ok=True)
            (agents / f"agent-{agent_id}.jsonl").write_text(jsonl(lines))
            (agents / f"agent-{agent_id}.meta.json").write_text(json.dumps({"agentType": agent_type, "description": description}))
        return path


class Codex:
    """One Codex session, written as a rollout under ~/.codex/sessions."""

    def __init__(self, name, project, branch, ago):
        self.id, self.clock, self.calls, self.tokens = stable_id(name), ANCHOR - ago, 0, 16000
        self.started = self.clock
        self.lines = []
        self.line("session_meta", {"id": self.id, "cwd": f"{CODE}/{project}", "git": {"branch": branch}, "source": "cli", "cli_version": "0.80.0", "timestamp": iso(self.clock)}, 0)
        self.line("turn_context", {"cwd": f"{CODE}/{project}", "model": CODEX_MODEL}, 0)

    def line(self, kind, payload, seconds):
        self.clock += timedelta(seconds=seconds)
        self.lines.append({"timestamp": iso(self.clock), "type": kind, "payload": payload})
        return self

    def item(self, kind, content, seconds):
        self.calls += 1
        return self.line("event_msg", {"type": "item_completed", "item": {"type": kind, "id": f"item_{self.calls}", "content": content}}, seconds)

    def you(self, text, seconds=30):
        return self.item("UserMessage", [{"type": "text", "text": text}], seconds)

    def says(self, text, seconds=12):
        self.item("AgentMessage", [{"type": "Text", "text": text}], seconds)
        self.tokens += 3100
        return self.line("event_msg", {"type": "token_count", "info": {"total_token_usage": {"input_tokens": self.tokens, "cached_input_tokens": self.tokens // 2, "output_tokens": self.tokens // 12, "reasoning_output_tokens": 400, "total_tokens": self.tokens + self.tokens // 12}}}, 0)

    def shell(self, command, output):
        self.calls += 1
        call_id = f"call_{self.calls}"
        self.line("response_item", {"type": "function_call", "name": "exec_command", "call_id": call_id, "arguments": json.dumps({"cmd": command})}, 8)
        return self.line("response_item", {"type": "function_call_output", "call_id": call_id, "output": output}, 3)

    def patch(self, patch_text):
        self.calls += 1
        call_id = f"call_{self.calls}"
        self.line("response_item", {"type": "custom_tool_call", "name": "apply_patch", "call_id": call_id, "input": patch_text}, 10)
        return self.line("response_item", {"type": "custom_tool_call_output", "call_id": call_id, "output": "Done"}, 2)

    def plan(self, steps):
        self.calls += 1
        call_id = f"call_{self.calls}"
        self.line("response_item", {"type": "function_call", "name": "update_plan", "call_id": call_id, "arguments": json.dumps({"plan": [{"step": step, "status": status} for step, status in steps]})}, 5)
        return self.line("response_item", {"type": "function_call_output", "call_id": call_id, "output": "Plan updated"}, 1)

    def write_to(self, home):
        moment = self.started
        folder = home / ".codex/sessions" / moment.strftime("%Y/%m/%d")
        folder.mkdir(parents=True, exist_ok=True)
        path = folder / f"rollout-{moment.strftime('%Y-%m-%dT%H-%M-%S')}-{self.id}.jsonl"
        path.write_text(jsonl(self.lines))
        return path


WEBHOOK = """export async function handlePaymentWebhook(event: PaymentEvent) {
  const order = await orders.findByPaymentId(event.paymentId);
  await ledger.recordCharge(order.id, event.amount);
  await orders.markPaid(order.id);
}
"""
FIXED = """export async function handlePaymentWebhook(event: PaymentEvent) {
  const firstDelivery = await webhookEvents.claim(event.id);
  if (!firstDelivery) return;
  const order = await orders.findByPaymentId(event.paymentId);
  await ledger.recordCharge(order.id, event.amount);
  await orders.markPaid(order.id);
}
"""


def double_charge():
    session = Claude("double-charge", "checkout-api", "fix/double-charge", timedelta(days=1, hours=2), "Fix double charge on webhook retry")
    session.you("Customers are getting charged twice on some orders. This one came in from support. Can you find out why?", image=True)
    session.says("The screenshot shows the same payment recorded again 30 seconds later, on a webhook retry. I'll start with the handler.", thinking="Same amount, 30 seconds apart, on a retry. That points at a webhook handler that isn't idempotent.")
    session.read("src/webhooks/payment.ts", WEBHOOK)
    session.bash("grep -rn 'recordCharge' src/ | head", "src/webhooks/payment.ts:3:  await ledger.recordCharge(order.id, event.amount);\nsrc/ledger/index.ts:18:export async function recordCharge(orderId: string, amount: number) {")
    session.agent("Check how often retries double-charged", "Explore", "Look through the last 90 days of webhook logs in ./logs and count orders with more than one recordCharge for one payment.", "Found 37 orders in 90 days with a duplicate charge. Every one had a retry within 60 seconds of a slow database write.", [
        ("ls logs | wc -l", "90"),
        ("grep -h 'recordCharge' logs/* | sort | uniq -d | wc -l", "37"),
    ])
    session.says("Found it. Nothing stops a retried webhook from recording the charge again: when the handler takes too long, the provider sends the event again and `recordCharge` runs twice. That's 37 orders in the last 90 days.\n\nI'll wrap the charge and the status update in a transaction so a retry...")
    session.stopped()
    session.you("wait, a transaction won't help here. the retry arrives after the first one has committed", seconds=12)
    session.says("You're right. A transaction only protects a single delivery, and these are two separate deliveries. The fix has to recognise an event it has already handled.")
    session.asks("How should the handler recognise an event it has already processed?", "Dedupe", [
        ("Table of processed event IDs (Recommended)", "Insert the event ID first. A unique index rejects a repeat, even when two deliveries race."),
        ("Check the order's status", "Skip if the order is already paid. Simple, but two racing deliveries can both pass the check."),
        ("Redis lock", "Fast, but the record disappears when the key expires."),
    ], "Table of processed event IDs (Recommended)")
    session.write("migrations/0042_webhook_events.sql", "CREATE TABLE webhook_events (\n  id TEXT PRIMARY KEY,\n  received_at TIMESTAMPTZ NOT NULL DEFAULT now()\n);\n")
    session.edit("src/webhooks/payment.ts", "  const order = await orders.findByPaymentId(event.paymentId);", "  const firstDelivery = await webhookEvents.claim(event.id);\n  if (!firstDelivery) return;\n  const order = await orders.findByPaymentId(event.paymentId);")
    session.write("src/webhooks/payment.test.ts", "it('charges once when the same event arrives twice', async () => {\n  await handlePaymentWebhook(event);\n  await handlePaymentWebhook(event);\n  expect(await ledger.chargesFor(order.id)).toHaveLength(1);\n});\n")
    session.bash("npm test -- webhooks", "PASS src/webhooks/payment.test.ts\n  ✓ charges once when the same event arrives twice (41 ms)\n  ✓ still charges a new event (12 ms)\n\nTests: 2 passed, 2 total")
    session.you("nice. write up the refund list so support can work through it", seconds=60)
    session.tool("Artifact", {"file_path": "reports/refunds.html"}, "Published.", url="https://claude.ai/artifact/refund-list-demo", title="Orders to refund", path="reports/refunds.html")
    session.says("Published a page with the 37 orders, what each customer was overcharged, and $1,813 to refund in total.\n\n| What | Result |\n|---|---|\n| Cause | Retried webhooks recorded the charge again |\n| Fix | Each event ID is claimed once, enforced by a unique index |\n| Test | Same event twice charges once: passing |\n| Refunds | 37 orders, $1,813 |\n\nOne thing to watch: the migration has to go out before the code, or the handler will fail on the missing table.")
    return session


def idempotency_fork(parent):
    session = Claude("idempotency-fork", "checkout-api", "fix/double-charge-redis", timedelta(hours=18), "Try the same fix with a Redis lock", forked_from=parent.id)
    session.lines.extend(json.loads(json.dumps(entry)) for entry in parent.lines[1:9])
    for entry in session.lines[1:]:
        entry["forkedFrom"] = {"sessionId": parent.id, "messageUuid": entry["uuid"]}
    session.forked_from = None
    session.parent = session.lines[-1]["uuid"]
    session.clock = ANCHOR - timedelta(hours=18)
    session.you("before we commit to a table, how would this look with a redis lock instead?")
    session.says("It would be less code, but it's a weaker guarantee: once the key expires, a late retry gets through. Providers can retry for up to three days, so the key would have to live that long. I'd stay with the table.")
    return session


def retry_review():
    session = Codex("retry-review", "checkout-api", "fix/double-charge", timedelta(hours=20))
    session.you("Review the double-charge fix on this branch. Tell me anything that could still charge someone twice.")
    session.plan([("Read the diff", "completed"), ("Trace concurrent deliveries", "completed"), ("Run the tests", "completed")])
    session.shell("git diff main --stat", " migrations/0042_webhook_events.sql |  4 ++++\n src/webhooks/payment.test.ts       |  5 +++++\n src/webhooks/payment.ts            |  2 ++\n 3 files changed, 11 insertions(+)")
    session.shell("npm test -- webhooks", "Tests: 2 passed, 2 total")
    session.says("The core fix is sound: the unique index settles a race between two deliveries. There's one real gap. `claim` commits before the charge runs, so if the process dies between them, the retry sees the event as handled and the customer is never charged. Claim and charge should share a transaction.")
    session.you("Fix that")
    session.patch("*** Begin Patch\n*** Update File: src/webhooks/payment.ts\n@@\n-  const firstDelivery = await webhookEvents.claim(event.id);\n-  if (!firstDelivery) return;\n+  await db.transaction(async (transaction) => {\n+    const firstDelivery = await webhookEvents.claim(event.id, transaction);\n+    if (!firstDelivery) return;\n*** End Patch")
    session.shell("npm test -- webhooks", "Tests: 3 passed, 3 total")
    session.says("Done. The claim and the charge now commit together, and I added a test that crashes between them. All 3 tests pass.")
    return session


def pricing_page():
    session = Claude("pricing-page", "marketing-site", "main", timedelta(minutes=48), "Rebuild the pricing page")
    session.you("The pricing page looks dated and the annual toggle is broken on mobile. Rebuild it, keep the same three plans.")
    session.read("src/pages/pricing.astro", "<PricingTable plans={plans} />")
    session.says("The toggle breaks because it relies on hover, which phones don't have. I'll make it a real switch and stack the plans on small screens.")
    session.edit("src/components/PricingTable.astro", '<div class="toggle" onmouseover={showAnnual}>', '<button role="switch" aria-checked={annual} onclick={toggleAnnual}>')
    session.bash("npm run build", "Completed in 2.1s. 14 pages built.")
    session.you("make annual the default", seconds=25)
    session.edit("src/components/PricingTable.astro", "let annual = false;", "let annual = true;")
    session.says("Annual is now the default. The build passes and I checked the switch at phone width.")
    return session


def hero_jump():
    session = Codex("hero-jump", "marketing-site", "main", timedelta(days=3, hours=4))
    session.you("The hero image makes the page jump as it loads. Find out why.")
    session.shell("grep -n '<img' src/components/Hero.astro", '12:  <img src="/hero.webp" alt="" />')
    session.says("The image has no width or height, so the browser leaves no room for it until it loads. Adding its real size fixes the jump.")
    session.patch('*** Begin Patch\n*** Update File: src/components/Hero.astro\n@@\n-  <img src="/hero.webp" alt="" />\n+  <img src="/hero.webp" alt="" width="1600" height="900" />\n*** End Patch')
    session.says("Fixed. The layout shift score on the home page went from 0.21 to 0.01.")
    return session


def flaky_login():
    session = Claude("flaky-login", "mobile-app", "main", timedelta(days=6), "Why does login fail on slow networks")
    session.you("Login fails about one time in five on the train. Works fine at home.")
    session.read("src/auth/session.ts", "const response = await fetch(LOGIN_URL, { signal: AbortSignal.timeout(3000) });")
    session.says("There's a 3 second timeout on login, and a slow connection regularly takes longer than that. On top of that, a timeout shows up as \"wrong password\".")
    session.edit("src/auth/session.ts", "AbortSignal.timeout(3000)", "AbortSignal.timeout(15000)")
    session.says("I raised the timeout to 15 seconds and gave timeouts their own message, so people aren't told their password is wrong.")
    return session


def push_tokens():
    session = Codex("push-tokens", "mobile-app", "release/4.2", timedelta(days=9))
    session.you("Push notifications stopped arriving after the 4.2 update. Why?")
    session.shell("git log --oneline -5 -- src/notifications", "a41f09c Move token storage to secure store\n19be2d1 Bump notification library")
    session.says("Commit a41f09c moved where the device token is stored, but nothing copies the old value across, so upgraded phones have no token and never register again.")
    return session


def shell_aliases():
    session = Claude("shell-aliases", "dotfiles", "main", timedelta(days=12), "Clean up zsh startup time")
    session.you("My terminal takes 2 seconds to open. Fix it.")
    session.bash("zsh -i -c 'zprof' | head -4", "1)  nvm_load   1412.20ms  71%\n2)  compinit    388.02ms  19%")
    session.says("Loading nvm is 71% of it. I'll load it the first time you use node instead of at every startup.")
    session.edit(".zshrc", 'source "$NVM_DIR/nvm.sh"', 'nvm() { unset -f nvm; source "$NVM_DIR/nvm.sh"; nvm "$@"; }')
    session.says("Your terminal now opens in 0.3 seconds.")
    return session


def old_migration():
    """Saved in the demo, and gone from Claude Code's own history: what saving is for."""
    session = Claude("old-migration", "checkout-api", "main", timedelta(days=47), "Move invoices to the new schema without downtime")
    session.you("We need to split invoices into invoices and line_items without taking checkout down. How would you do it?")
    session.says("In four steps, each safe to roll back:\n\n1. Add the new table, and write to both.\n2. Copy old rows across in small batches.\n3. Compare the two for a week, then switch reads.\n4. Stop writing to the old columns and drop them.")
    session.bash("psql -c 'select count(*) from invoices'", " count\n--------\n 412907")
    session.says("At 412,907 rows, batches of 5,000 take about 20 minutes, and checkout won't notice. I've written the backfill script.")
    return session


def main():
    double = double_charge()
    sessions = [double, idempotency_fork(double), retry_review(), pricing_page(), hero_jump(), flaky_login(), push_tokens(), shell_aliases(), old_migration()]
    saved_only = sessions[-1]
    with tempfile.TemporaryDirectory() as temporary:
        home = Path(temporary).resolve()
        paths = [session.write_to(home) for session in sessions]
        dumps = home / "dumps"
        dumps.mkdir()
        subprocess.run(["cargo", "run", "--quiet", "--example", "dump_sessions", "--", str(dumps), *map(str, paths)],
                       cwd=REPO / "src-tauri", check=True, env={**os.environ, "HOME": str(home), "CODEX_HOME": str(home / ".codex"),
                            "CARGO_HOME": os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")), "RUSTUP_HOME": os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup"))})
        listed = json.loads((dumps / "_listed.json").read_text())
        projects = json.loads((dumps / "_projects.json").read_text())
        details = {path.as_posix(): json.loads((dumps / f"{path.stem}.json").read_text()) for path in paths}
        for session in listed:
            session["last_written"] = session["modified"]
        # These folders are made up too, so the check for moved folders must not flag them.
        for project in projects:
            project["folder_exists"] = True
        # The saved-only session isn't in Claude Code's history any more, so it counts for no project.
        saved_project = next(session["project_path"] for session in listed if session["session_id"] == saved_only.id)
        next(project for project in projects if project["project_path"] == saved_project)["session_count"] -= 1
        data = {"anchor": iso(ANCHOR), "sessions": listed, "projects": projects, "details": details, "saved_only": saved_only.id}
        text = json.dumps(data, separators=(",", ":")).replace(str(home), "/Users/alex")
    (REPO / "src/demoData.json").write_text(text + "\n")
    print(f"{len(listed)} sessions, {len(projects)} projects, {len(text) // 1024} KB")


if __name__ == "__main__":
    main()
