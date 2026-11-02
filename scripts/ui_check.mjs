// Regression checks for UI behavior that unit tests can't see: drives the built frontend in
// headless Chrome with synthetic data and a mocked backend. No server is started and no real
// transcripts are read. Run with `npm run test:ui`; exits 1 if a check fails.
//
// Set CHROME_PATH if Chrome isn't at its default macOS location.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";

const DIST = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "dist");
const CHROME = process.env.CHROME_PATH ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const MIME_TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css" };
const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

let failures = 0;
function check(name, passed, detail = "") {
  console.log(`${passed ? "ok  " : "FAIL"} ${name}${!passed && detail ? ` — ${detail}` : ""}`);
  if (!passed) failures += 1;
}

// The fields a Rust struct sends to the frontend: `required` always, `optional` only when set.
function serializedFields(source, declaration) {
  const body = source.match(new RegExp(`${declaration} \\{([\\s\\S]*?)\\n\\s*\\}`))?.[1];
  if (!body) throw new Error(`${declaration} not found in commands.rs`);
  const fields = { required: [], optional: [] };
  let skippable = false;
  for (const line of body.split("\n").map((entry) => entry.trim())) {
    if (line.startsWith("#[serde") && line.includes("skip_serializing_if")) skippable = true;
    const field = line.match(/^(?:pub )?(\w+): /);
    if (field) {
      fields[skippable ? "optional" : "required"].push(field[1]);
      skippable = false;
    }
  }
  return fields;
}

// Runs in the page, standing in for the Rust backend.
function installMockBackend(options) {
  const PIXEL = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
  const project = { tools: ["claude", "codex"], folder_exists: true, project_path: "/work/app", project_name: "app", short_path: "~/app", session_count: 2, last_active: "now", last_active_ms: 1 };
  const isCodex = (jsonlPath) => jsonlPath.includes("rollout-");
  const makeSession = (id, title, jsonlPath, modified = "2026-01-01T01:00:00Z", lastWritten = modified) => ({
    session_id: id, tool: isCodex(jsonlPath) ? "codex" : "claude", archived_in_tool: false, summary: null, custom_title: title, ai_title: null, first_prompt: null, project_path: "/work/app", project_name: "app",
    created: "2026-01-01T00:00:00Z", modified, last_written: lastWritten, conversation_count: 1,
    git_branch: null, forked_from_session_id: null, jsonl_path: jsonlPath,
  });
  // The same text searchable_text() builds in commands.rs, which is what session search matches on.
  const searchableText = (blocks) => blocks.map((block) => {
    if (block.kind === "tool") return `${block.name} ${block.summary}`;
    return block.text;
  }).join("\n\n");
  const assistantTurn = (timestamp, blocks) => ({ role: "assistant", timestamp, text: searchableText(blocks), blocks });
  const toolBlock = (name, summary, id, extra = {}) => ({ kind: "tool", name, summary, input: "", tool_use_id: id, ...extra });
  const liveSession = makeSession("live", "Live session", "/p/live.jsonl");
  const expiredSession = makeSession("expired", "Expired session", "/archive/expired/expired.jsonl");
  const codexSession = makeSession("codex", "Codex session", "/codex/rollout-codex.jsonl");
  const movedProject = { ...project, tools: ["codex"], folder_exists: false, project_path: "/gone/old", project_name: "old", session_count: 0 };
  const codexConversation = [
    { role: "user", text: "needle at the start. " + "A long pasted brief. ".repeat(200), timestamp: "2026-01-01T00:00:01Z" },
    assistantTurn("2026-01-01T00:00:02Z", [
      toolBlock("Shell", "git status", "shell_ok"),
      toolBlock("Shell", "cargo test", "shell_bad"),
      toolBlock("Edit", "src/main.rs", "edit"),
      { kind: "text", text: "All done." },
    ]),
  ];
  const conversation = [
    { role: "user", text: 'say "hello" to <Entity>', timestamp: "2026-01-01T00:00:01Z" },
    assistantTurn("2026-01-01T00:00:02Z", [
      { kind: "thinking", text: "private reasoning" },
      { kind: "text", text: 'She said "hi" & left.\n\n1. first\n2. second\n\nSee <Entity> and [the file](apps/main.ts) and [docs](https://example.com/docs).\n\n```\nx = a * b * c\n\n- item\n```' },
      toolBlock("Agent", "helper", "toolu_agent", { agent_id: "helper" }),
    ]),
    { role: "compaction", text: "See https://example.com/summary\n\n```\ncompacted code\n```", timestamp: "2026-01-01T00:00:03Z", compaction_detail: "auto · 10.0k → 1.0k tokens" },
  ];
  const agentConversation = [
    { role: "user", text: "Do the task", timestamp: "2026-01-01T00:00:01Z" },
    assistantTurn("2026-01-01T00:00:02Z", [toolBlock("Bash", "screenshot", "toolu_shot")]),
  ];
  // Long enough to need scrolling, with "needle" in every place a match can hide: a clamped
  // prompt, a fold, thinking inside the fold, plain text and the far end of the session.
  const filler = "Nothing to find in this paragraph, it only takes up room. ".repeat(12);
  const searchConversation = [
    { role: "user", text: `${"A long brief. ".repeat(100)} ends with needle`, timestamp: "2026-01-02T00:00:01Z" },
    assistantTurn("2026-01-02T00:00:02Z", [{ kind: "thinking", text: "a needle thought" }, toolBlock("Bash", "grep needle", "needle_tool"), { kind: "text", text: "one needle then another needle" }]),
    ...[...Array(30).keys()].map((index) => assistantTurn(`2026-01-02T00:01:${String(index).padStart(2, "0")}Z`, [{ kind: "text", text: filler }])),
    assistantTurn("2026-01-02T00:02:00Z", [toolBlock("Bash", "far away failure", "far_bad"), { kind: "text", text: "the last needle" }]),
  ];
  const wideHeader = [...Array(12).keys()].map((index) => `A rather long column heading ${index}`);
  const markdownConversation = [
    { role: "user", text: "Show me some formatting", timestamp: "2026-01-03T00:00:01Z" },
    assistantTurn("2026-01-03T00:00:02Z", [
      toolBlock("AskUserQuestion", "Which one?", "question"),
      { kind: "text", text: [
        "sanitize-marker\n\n1. first\n2. second",
        `| ${wideHeader.join(" | ")} |\n|${" --- |".repeat(12)}\n| ${wideHeader.map(() => "cell").join(" | ")} |`,
        `A long link https://example.com/${"long-path/".repeat(40)} and code \`${"unbroken".repeat(40)}\``,
        '<img src=x onerror="window.__backend.attacked = true"> <script>window.__backend.attacked = true</script> [click](javascript:window.__backend.attacked=true)',
      ].join("\n\n") },
    ]),
  ];
  markdownConversation.push(
    { role: "system-event", text: "A quiet hook ran", timestamp: "2026-01-03T00:00:03Z", event: { kind: "hook", is_error: false } },
    { role: "system-event", text: "A hook broke", timestamp: "2026-01-03T00:00:04Z", event: { kind: "hook", is_error: true } },
  );
  const answeredQuestion = { question: "Which one?", header: "Pick", multi_select: false, notes: "My own note", options: [
    { label: "The chosen answer", description: "picked", chosen: true, custom: false },
    { label: "The other answer", description: "left", chosen: false, custom: false },
  ] };
  const searchSession = makeSession("search", "Search session", "/p/search.jsonl");
  const markdownSession = makeSession("markdown", "Markdown session", "/p/markdown.jsonl");
  const twoDaysAgo = new Date(Date.now() - 2 * 86400000).toISOString();
  const hourAgo = new Date(Date.now() - 3600000).toISOString();
  // Tools append bookkeeping long after the last message: the row dates from the message, the lamp from the file.
  const stillWritten = makeSession("written", "Still being written", "/p/written.jsonl", twoDaysAgo, new Date().toISOString());
  const longQuiet = makeSession("quiet", "Long quiet", "/p/quiet.jsonl", hourAgo);
  const extraSessions = { search: [searchSession, markdownSession], times: [stillWritten, longQuiet], duplicates: [makeSession("live", "Live session", "/p/live.jsonl")] };
  const stats = (jsonlPath) => ({
    input_tokens: 1, output_tokens: 1, cache_read_tokens: 0, cache_creation_tokens: 0, assistant_count: 1, user_prompt_count: isCodex(jsonlPath) ? 7 : 1, thinking_block_count: 1, models: [], tool_counts: [],
    hooks: jsonlPath === markdownSession.jsonl_path ? [{ event: "Stop", runs: 4, failures: 3 }] : [], active_ms: 0, active_is_partial: false, cost_is_partial: false, agent_output_tokens: 0, agent_input_tokens: 0, inherited_prompts: 0,
  });
  const subagent = { agent_id: "helper", agent_type: "general", description: "helper", jsonl_path: "/p/live/subagents/agent-helper.jsonl", tool_use_id: "toolu_agent", workflow_run: null };
  const searchHit = (session, sessionName, matched) => ({ session_id: session.session_id, tool: session.tool, project_name: "app", project_path: "/work/app", session_name: sessionName, matched_text: matched, match_source: "message", timestamp: null, jsonl_path: session.jsonl_path });
  const fileEdit = { timestamp: null, action: "edit", old_string: "before", new_string: "after", tool_use_id: "edit", replace_all: false, failed: false };
  const fileChange = { path: "/work/app/a.rs", display_path: "a.rs", edits: [fileEdit], edit_count: 1, read_count: 0 };
  const toolResult = { content: "boom", is_error: true, persisted_path: null, image_count: 0 };
  window.__backend = {
    calls: [], messageDelay: 0, scanDelay: options.scanDelay ?? 400, changesDelay: 0, archiveFails: false, delays: {}, lastWritten: {}, attacked: false, sanitized: [], intervals: [],
    shapes: { ProjectInfo: project, SessionInfo: liveSession, ConversationMessage: conversation[1], "Tool": toolBlock("Bash", "x", "y"), SessionStats: stats("/p/live.jsonl"), HookStat: { event: "Stop", runs: 1, failures: 0 }, SubagentInfo: subagent, GlobalSearchResult: searchHit(liveSession, "x", "y"), ToolResultPayload: toolResult, FileChange: fileChange, FileEditEntry: fileEdit, AnsweredQuestion: answeredQuestion, AnsweredOption: answeredQuestion.options[0] },
  };
  // DOMPurify parses what it's given, so this shows whether rendered text really went through it.
  const parseHtml = DOMParser.prototype.parseFromString;
  DOMParser.prototype.parseFromString = function (markup, mediaType) {
    window.__backend.sanitized.push(String(markup));
    return parseHtml.call(this, markup, mediaType);
  };
  // Lets a check run the 20-second poll now.
  const startInterval = window.setInterval.bind(window);
  window.setInterval = (callback, milliseconds) => {
    window.__backend.intervals.push(callback);
    return startInterval(callback, milliseconds);
  };
  const replies = {
    get_projects: () => [project, movedProject],
    get_archived_sessions: () => [expiredSession],
    scan_projects: async () => {
      await new Promise((resolve) => setTimeout(resolve, window.__backend.scanDelay));
      return [liveSession, codexSession, ...(extraSessions[options.extra] ?? [])].map((session) => ({ ...session, last_written: window.__backend.lastWritten[session.jsonl_path] ?? session.last_written }));
    },
    get_session_questions: (args) => args.jsonlPath === markdownSession.jsonl_path ? { question: [answeredQuestion] } : {},
    get_session_artifacts: () => ({}),
    get_session_file_changes: async () => {
      await new Promise((resolve) => setTimeout(resolve, window.__backend.changesDelay));
      return [];
    },
    get_session_stats: (args) => stats(args.jsonlPath),
    list_subagents: () => [subagent],
    get_subagent_messages: () => agentConversation,
    get_tool_results: (args) => args.jsonlPath === searchSession.jsonl_path ? { far_bad: toolResult } : args.jsonlPath.includes("rollout-") ? { shell_bad: toolResult } : (args.jsonlPath.includes("agent-") ? { toolu_shot: { content: "done", is_error: false, persisted_path: null, image_count: 1 } } : {}),
    get_tool_result_images: () => [PIXEL],
    open_external_url: () => null,
    get_session_messages: async (args) => {
      await new Promise((resolve) => setTimeout(resolve, window.__backend.messageDelay));
      if (args.jsonlPath === searchSession.jsonl_path) return searchConversation;
      if (args.jsonlPath === markdownSession.jsonl_path) return markdownConversation;
      return args.jsonlPath.includes("rollout-") ? codexConversation : conversation;
    },
    global_search: async ({ query }) => {
      if (query === "slow") {
        await new Promise((resolve) => setTimeout(resolve, 1200));
        return [searchHit(liveSession, "STALE RESULT", "stale")];
      }
      if (query === "reopen") return [searchHit(codexSession, "Codex hit", "needle")];
      if (query === "needle") return [{ ...searchHit(searchSession, "Needle hit", "the last needle"), timestamp: "2026-01-02T00:02:00Z" }];
      return [searchHit(expiredSession, "Fresh result", "fresh")];
    },
    archive_session: () => {
      if (window.__backend.archiveFails) throw "Saved the transcript, but couldn't copy session files: disk full";
      return null;
    },
  };
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => 0,
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    invoke: async (command, args) => {
      window.__backend.calls.push({ command, args });
      if (command.startsWith("plugin:app")) return "0.0.0";
      if (command.startsWith("plugin:")) return null;
      const delay = window.__backend.delays[command]?.[args?.jsonlPath];
      if (delay) await new Promise((resolve) => setTimeout(resolve, delay));
      // A copy, as a real reply crossing from Rust always is: nothing shares identity with the last one.
      return replies[command] ? structuredClone(await replies[command](args)) : null;
    },
  };
  try {
    localStorage.setItem("claude-sessions-prefs", JSON.stringify({ hasSeenWelcome: options.welcome !== true, theme: options.theme ?? "dark" }));
    localStorage.setItem("claude-sessions-bookmarks", JSON.stringify(options.bookmarks ?? []));
  } catch {
    // Only the welcome dialog depends on this; the checks below would flag a real problem.
  }
}

const browser = await puppeteer.launch({ executablePath: CHROME, headless: "new", pipe: true });
const pageErrors = [];
let page;

// A fresh app, so one check's leftovers can't pass or fail the next.
async function openApp(options = {}, viewport = { width: 1200, height: 800 }) {
  await page?.close();
  page = await browser.newPage();
  await page.setViewport(viewport);
  await page.evaluateOnNewDocument(installMockBackend, options);
  await page.setRequestInterception(true);
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (url.hostname !== "evertusk.test") {
      request.abort();
      return;
    }
    const file = path.join(DIST, url.pathname === "/" ? "index.html" : url.pathname);
    if (!fs.existsSync(file)) {
      request.respond({ status: 404, body: "" });
      return;
    }
    request.respond({ status: 200, contentType: MIME_TYPES[path.extname(file)] ?? "application/octet-stream", body: fs.readFileSync(file) });
  });
  page.on("pageerror", (pageError) => pageErrors.push(String(pageError)));
  await page.goto("http://evertusk.test/");
}

const listReady = () => page.waitForFunction(() => document.querySelector(".board .row") && !document.querySelector(".reading"));

async function openSession(title) {
  await click(".board .row", title);
  await page.waitForSelector(".message-slot");
  await sleep(150);
}

async function pressWithMeta(key) {
  await page.keyboard.down("Meta");
  await page.keyboard.press(key);
  await page.keyboard.up("Meta");
  await sleep(80);
}

// True only if the element is what you'd actually see at its own position: scrolled into the
// pane, and not clipped by a clamp or hidden in a closed fold.
const reallyVisible = (selector) => page.evaluate((target) => {
  const element = document.querySelector(target);
  if (!element) return false;
  const box = element.getClientRects()[0];
  if (!box) return false;
  const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
  return hit === element || element.contains(hit) || !!hit?.contains(element);
}, selector);

const text = (selector) => page.evaluate((target) => document.querySelector(target)?.textContent ?? null, selector);
const count = (selector) => page.evaluate((target) => document.querySelectorAll(target).length, selector);
const click = (selector, containing = "") =>
  page.evaluate(
    (target, needle) => {
      const element = [...document.querySelectorAll(target)].find((candidate) => candidate.textContent.includes(needle));
      element?.click();
      return Boolean(element);
    },
    selector,
    containing
  );
const calls = (command) => page.evaluate((name) => window.__backend.calls.filter((call) => call.command === name), command);

await openApp();
await page.waitForSelector(".reading");

// The mock answers with what the Rust structs send, so a new field can't quietly leave it behind.
const backendSource = fs.readFileSync(path.join(path.dirname(DIST), "src-tauri", "src", "commands.rs"), "utf8");
for (const [structName, sample] of Object.entries(await page.evaluate(() => window.__backend.shapes))) {
  const { required, optional } = serializedFields(backendSource, structName === "Tool" ? "\\bTool" : `pub struct ${structName}`);
  const keys = Object.keys(sample);
  const missing = required.filter((field) => !keys.includes(field));
  const phantom = keys.filter((key) => key !== "kind" && !required.includes(key) && !optional.includes(key));
  check(`the mock's ${structName} matches the Rust struct`, missing.length === 0 && phantom.length === 0, `missing ${missing} phantom ${phantom}`);
}

// While the first read runs, every area says so rather than sitting blank or claiming "none".
check("the list says it's reading", (await text(".board .reading")).includes("Reading"));
check("the list shows a progress line", (await count(".board .progress")) === 1);
check("the session pane says it's reading", (await text(".empty-state")).includes("Reading your session history"));
check("the rail doesn't claim there are no projects", !(await text(".projects")).includes("No projects"));
await page.waitForSelector(".board .row:not(.skeleton)");
await page.waitForFunction(() => !document.querySelector(".board .reading"));
check("loading indicators clear", (await count(".board .progress")) === 0 && !(await text(".empty-state")).includes("Reading"));

// One folder is one project, with every tool's sessions mixed in the list.
check("a folder used by two tools is one project with every session", (await text(".projects .row .count")) === "3");
check("sessions from both tools share the list", new Set(await page.evaluate(() => [...document.querySelectorAll(".board .tool")].map((tool) => tool.textContent))).size === 2);
check("a moved folder says so in words", (await text(".projects .row.missing"))?.includes("not on disk"));
await click(".tool-filter button", "Codex");
await sleep(50);
check("the tool filter narrows the list", (await count(".board .row")) === 1 && (await text(".board .row .title")) === "Codex session");
await click(".tool-filter button", "All");
await sleep(50);

// A slow, older search must not overwrite a newer one.
await page.type(".search input", "slow");
await sleep(450);
await page.type(".search input", "er");
await sleep(1500);
let results = await text(".board");
check("stale global search is discarded", results.includes("Fresh result") && !results.includes("STALE"), results);
check("search indicator clears", !results.includes("Searching"), results);

// A search hit on an expired saved session opens it from the archive.
await click(".row.result", "Fresh result");
await page.waitForSelector(".session-title");
check("expired saved session opens from search", (await text(".session-title")) === "Expired session");
await page.evaluate(() => {
  const input = document.querySelector(".search input");
  input.focus();
  input.select();
});
await page.keyboard.press("Escape");
await sleep(100);

// Refresh during a slow load must not leave the session stuck on "Loading".
await page.evaluate(() => {
  window.__backend.messageDelay = 500;
  window.__backend.changesDelay = 500;
});
await click(".board .row", "Live session");
await sleep(50);
check("opening a session says it's reading", (await text(".loading-state"))?.includes("Reading this session"));
await click(".tabs button", "Changes");
await sleep(30);
check("Changes doesn't claim nothing changed while it's still reading", (await text(".tab-panel")).includes("Reading what changed"));
await click(".tabs button", "Conversation");
await page.evaluate(() => (window.__backend.changesDelay = 0));
await sleep(30);
await click(".header-action-btn.icon-only");
await sleep(1300);
check("refresh during a load doesn't stick on Loading", (await count(".loading-state")) === 0 && (await count(".assistant-row")) === 1);
await page.evaluate(() => (window.__backend.messageDelay = 0));

// Tool activity folds to one line; a whole file's worth of tool calls stays out of the way.
check("activity starts folded", (await count(".activity")) > 0 && (await count(".activity[open]")) === 0);

// Markdown fidelity.
const prose = await page.evaluate(() => document.querySelector(".assistant-content").textContent);
check("code keeps its asterisks, blank lines and dashes", (await text(".assistant-content pre code")) === "x = a * b * c\n\n- item");
check("numbered lists stay numbered", (await count(".assistant-content ol li")) === 2);
check("tags in prose are shown literally", prose.includes("<Entity>"));
check("file references aren't links", (await count(".assistant-content .file-ref")) === 1 && (await count('.assistant-content a[href*="apps/main"]')) === 0);

// Collapsed thinking renders only when opened.
check("collapsed thinking isn't rendered", (await count(".thinking-content")) === 0);
await click(".activity summary");
await sleep(100);
await click(".thinking-block summary");
await sleep(100);
check("thinking renders when opened", (await text(".thinking-content"))?.includes("private reasoning"));

// Links anywhere must open externally, never navigate the app.
await click(".compaction-details summary");
await sleep(100);
await click(".compaction-content a");
await sleep(100);
check("compaction link doesn't navigate the app", page.url().startsWith("http://evertusk.test"), page.url());
let opened = await calls("open_external_url");
check("compaction link opens externally", opened.some((call) => call.args.url === "https://example.com/summary"));
await click(".assistant-content a", "docs");
await sleep(100);
check("message link doesn't navigate the app", page.url().startsWith("http://evertusk.test"), page.url());
await click(".compaction-content .code-copy-btn");
await sleep(100);
const copies = await calls("plugin:clipboard-manager|write_text");
check("code copy works in compaction summaries", JSON.stringify(copies).includes("compacted code"));

// Search highlighting must not corrupt entities.
await page.type(".message-search-bar input", "quot");
await sleep(350);
check("searching 'quot' doesn't garble text", !(await page.evaluate(() => document.querySelector(".messages-container").textContent)).includes("&quot"));
check("searching 'quot' marks nothing", (await count("mark.search-mark")) === 0);
await page.keyboard.press("Escape");
await page.type(".message-search-bar input", '"');
await sleep(350);
const quoteMarks = await page.evaluate(() => [...document.querySelectorAll("mark.search-mark")].map((mark) => mark.textContent));
check("searching for a quote highlights it", quoteMarks.length > 0 && quoteMarks.every((mark) => mark === '"'), JSON.stringify(quoteMarks));
check("quotes highlight in Claude's text and in yours", (await count(".assistant-content mark.search-mark")) > 0 && (await count(".user-bubble mark.search-mark")) > 0);
await page.keyboard.press("Escape");
await page.type(".message-search-bar input", "&");
await sleep(350);
check("searching for & highlights it in Claude's text", (await count(".assistant-content mark.search-mark")) > 0);
await page.keyboard.press("Escape");
await sleep(50);

// Subagent panel: labels, images, and Esc closing the image before the panel.
await page.evaluate(() => {
  const fold = [...document.querySelectorAll(".activity")].find((activity) => activity.textContent.includes("agent"));
  fold.open = true;
});
await page.waitForSelector(".assistant-content button.tool-pill");
await click(".assistant-content button.tool-pill", "helper");
await page.waitForSelector(".subagent-panel .user-bubble");
check("opening an agent switches to the Agents tab", (await text(".tabs button.active")).includes("Agents"));
check("a subagent's brief is labelled Task", (await text(".subagent-panel .role-tag")) === "Task");
await click(".subagent-panel .activity summary");
await page.waitForSelector(".subagent-panel button.tool-pill");
await click(".subagent-panel button.tool-pill", "screenshot");
await page.waitForSelector(".subagent-panel .tool-images img");
await click(".subagent-panel .tool-images button");
await sleep(100);
check("subagent screenshots open full size", (await count(".lightbox-image")) === 1);
await page.keyboard.press("Escape");
await sleep(100);
check("Esc closes the image first", (await count(".lightbox-image")) === 0 && (await count(".subagent-panel")) === 1);
await page.keyboard.press("Escape");
await sleep(100);
check("Esc then returns to the agent list", (await count(".subagent-panel")) === 0 && (await count(".agents li")) === 1);
await click(".tabs button", "Conversation");
await sleep(50);

// A failed save is visible.
await page.evaluate(() => (window.__backend.archiveFails = true));
await click(".save-btn");
await sleep(150);
check("a failed save shows its reason", (await text(".header-notice"))?.includes("disk full"));

// A refresh keeps you on the tab you're reading.
await click(".tabs button", "Stats");
await sleep(50);
await page.evaluate(() => (window.__backend.archiveFails = false));
await click(".header-action-btn.icon-only");
await sleep(150);
check("a refresh doesn't change the tab", (await text(".tabs button.active")).includes("Stats"));

// A Codex session speaks for itself and claims nothing it doesn't have.
await click(".board .row", "Codex session");
await page.waitForSelector(".user-bubble");
await sleep(150);
check("a Codex reply is labelled Codex", (await text(".claude-label")) === "Codex");
check("your messages sit on the right, the agent's on the left", await page.evaluate(() => {
  const mine = document.querySelector(".user-bubble").getBoundingClientRect();
  const container = document.querySelector(".message-slot").getBoundingClientRect();
  return Math.abs(mine.right - container.right) < 8 && getComputedStyle(document.querySelector(".assistant-content")).backgroundColor === "rgba(0, 0, 0, 0)";
}));
check("search scopes stay out of the way until you search", (await count(".scope-toggle")) === 0, await page.evaluate(() => document.querySelector(".message-search-bar input")?.value ?? "no bar"));
await page.type(".message-search-bar input", "needle");
await sleep(250);
check("the session's search filter names its own tool", (await text(".scope-toggle")).includes("Codex") && !(await text(".scope-toggle")).includes("Claude"));
await page.evaluate(() => document.querySelector(".message-search-bar input").focus());
await page.keyboard.press("Escape");
await sleep(80);
check("Codex sessions offer no Save", (await count(".save-btn")) === 0);
const foldLine = await text(".activity summary");
check("a fold summarises in plain words", foldLine.includes("Ran 2 commands") && foldLine.includes("made 1 edit"), foldLine);
check("a fold shows its failures", foldLine.includes("1 failed"), foldLine);
check("the answer stays outside the fold", (await text(".assistant-content > .segment-text, .assistant-content"))?.includes("All done."));
check("a long prompt is cut down", (await page.evaluate(() => document.querySelector(".user-bubble").offsetHeight)) <= 240);
await click(".show-prompt");
await sleep(50);
check("Show full prompt reveals it", (await page.evaluate(() => document.querySelector(".user-bubble").offsetHeight)) > 240);
await click(".show-prompt");
await page.type(".message-search-bar input", "cargo test");
await sleep(350);
check("a search match inside a fold opens it", (await count(".activity[open]")) === 1);
await page.keyboard.press("Escape");
await sleep(50);

// Coming back to the conversation keeps your place and your search.
await page.evaluate(() => {
  const container = document.querySelector(".messages-container");
  container.style.height = "100px";
  container.scrollTop = 40;
});
await page.type(".message-search-bar input", "brief");
await sleep(250);
const beforeTabs = await page.evaluate(() => document.querySelector(".messages-container").scrollTop);
await click(".tabs button", "Stats");
await sleep(80);
await click(".tabs button", "Conversation");
await sleep(80);
check("returning to Conversation keeps the scroll position", (await page.evaluate(() => document.querySelector(".messages-container").scrollTop)) === beforeTabs);
check("returning to Conversation keeps the search", (await page.evaluate(() => document.querySelector(".message-search-bar input").value)) === "brief");
await page.evaluate(() => {
  document.querySelector(".messages-container").style.height = "";
  document.querySelector(".message-search-bar input").focus();
});
await page.keyboard.press("Escape");
await sleep(50);
await click(".tabs button", "Stats");
await page.keyboard.down("Meta");
await page.keyboard.press("f");
await page.keyboard.up("Meta");
await sleep(80);
check("⌘F works from any tab", (await text(".tabs button.active")).includes("Conversation") && (await page.evaluate(() => document.activeElement?.closest(".message-search-bar") !== null)));
await page.evaluate(() => document.activeElement?.blur());

// Words needn't be adjacent to match, the same as search everything.
await page.type(".message-search-bar input", "pasted long");
await sleep(250);
check("session search matches separate words", (await count("mark.search-mark")) >= 2);
await page.evaluate(() => document.querySelector(".message-search-bar input").focus());
await page.keyboard.press("Escape");
await sleep(50);

// Clicking a row gives it focus even in WebKit, where buttons don't take focus on click.
await click(".board .row", "Live session");
await sleep(100);
check("a clicked row takes focus", await page.evaluate(() => document.activeElement?.classList.contains("row")));
await click(".board .row", "Codex session");
await page.waitForSelector(".user-bubble");
await sleep(150);

// The title bar toggles hide each side panel, and nothing gets stranded.
const shown = (selector) => page.evaluate((target) => getComputedStyle(document.querySelector(target)).display !== "none", selector);
check("there is no full screen button", (await count(".full-btn")) === 0);
check("the session's name is in the title bar", (await text(".titlebar .session-title")) === "Codex session");
await page.hover(".rail-toggle");
await sleep(60);
check("the toggle's tooltip names it and its shortcut", (await text(".rail-toggle .tip")).includes("Toggle projects") && (await shown(".rail-toggle .tip")));
await click(".rail-toggle");
await sleep(60);
check("projects can be hidden", !(await shown(".sidebar")) && (await shown(".board")));
await click(".list-toggle");
await sleep(60);
check("hiding both gives the whole window to the chat", !(await shown(".board")) && (await page.evaluate(() => document.querySelector(".main-content").getBoundingClientRect().width === innerWidth)));
check("with both hidden, the search bar and stats get out of the way", (await count(".message-search-bar")) === 0 && (await count(".summary")) === 0);
check("Save and Export stay reachable", (await text(".session-actions")).includes("Export"));
await page.keyboard.down("Meta");
await page.keyboard.press("f");
await page.keyboard.up("Meta");
await sleep(80);
check("⌘F brings the search bar back", (await count(".message-search-bar")) === 1);
await page.evaluate(() => document.querySelector(".message-search-bar input").focus());
await page.keyboard.press("Escape");
await sleep(60);
check("Esc puts it away again", (await count(".message-search-bar")) === 0);
await page.keyboard.down("Meta");
await page.keyboard.press("k");
await page.keyboard.up("Meta");
await sleep(80);
check("⌘K brings back a hidden search box", (await shown(".sidebar")) && (await page.evaluate(() => document.activeElement?.closest(".search") !== null)));
await page.evaluate(() => document.activeElement?.blur());
await page.keyboard.down("Meta");
await page.keyboard.press("2");
await page.keyboard.up("Meta");
await sleep(60);
check("⌘2 toggles the session list", await shown(".board"));
await page.keyboard.down("Meta");
await page.keyboard.press("1");
await page.keyboard.up("Meta");
await sleep(60);
check("⌘1 toggles projects", !(await shown(".sidebar")));
await click(".rail-toggle");
await sleep(60);
await click(".list-toggle");
await sleep(40);
await page.type(".search input", "er");
await sleep(30);
check("searching brings the list back right away", await shown(".board"));
await page.evaluate(() => document.querySelector(".search input").focus());
await page.keyboard.press("Escape");
await sleep(600);

// On a wide window the chat is centred, not pinned to one side with empty space on the other.
await page.setViewport({ width: 1800, height: 900 });
await sleep(100);
const sideGaps = await page.evaluate(() => {
  const pane = document.querySelector(".messages-container").getBoundingClientRect();
  const column = document.querySelector(".message-slot").getBoundingClientRect();
  return { left: column.left - pane.left, right: pane.right - column.right };
});
check("the chat is centred on a wide window", Math.abs(sideGaps.left - sideGaps.right) < 40, JSON.stringify(sideGaps));
await page.setViewport({ width: 1200, height: 800 });
await sleep(100);

// Failure counts lead to the failure.
await click(".tabs button", "Stats");
await sleep(50);
await click(".summary button.bad", "tool failure");
await sleep(150);
check("a failure count opens the failed run", (await text(".tabs button.active")).includes("Conversation") && (await count(".activity[open]")) === 1);
await page.evaluate(() => (document.querySelector(".activity").open = false));

// Arrow keys belong to the list only while focus is in it.
await page.evaluate(() => document.activeElement?.blur());
await page.keyboard.press("ArrowDown");
await sleep(100);
check("arrows don't change the session from the conversation", (await text(".session-title")) === "Codex session");
await page.focus(".board .row.selected");
await page.keyboard.press("ArrowUp");
await sleep(150);
check("arrows move through sessions from the list", (await text(".session-title")) !== "Codex session");
check("the selected row keeps focus", await page.evaluate(() => document.activeElement?.classList.contains("selected")));
await click(".board .row", "Codex session");
await page.waitForSelector(".user-bubble");

// Every view fits both window sizes, and the small one keeps a way back to the list.
for (const [width, height] of [[1200, 800], [800, 600]]) {
  await page.setViewport({ width, height });
  for (const tab of ["Conversation", "Changes", "Agents", "Stats"]) {
    await click(".tabs button", tab);
    await sleep(80);
    const layout = await page.evaluate(() => {
      const exportButton = [...document.querySelectorAll(".header-action-btn")].find((button) => button.textContent.includes("Export")).getBoundingClientRect();
      const pane = document.querySelector(".conversation-main").getBoundingClientRect();
      return { fits: exportButton.right <= pane.right + 1, width: pane.width, overflow: document.documentElement.scrollWidth > innerWidth };
    });
    check(`${tab} tab fits at ${width}px`, layout.fits && layout.width >= 300 && !layout.overflow, JSON.stringify(layout));
  }
}
check("the session replaces the list at 800px", (await count(".board")) === 0 || (await page.evaluate(() => getComputedStyle(document.querySelector(".board")).display)) === "none");
await page.type(".search input", "er");
await sleep(600);
check("searching at 800px brings the results into view", await page.evaluate(() => getComputedStyle(document.querySelector(".board")).display !== "none" && document.querySelectorAll(".row.result").length > 0));
await page.focus(".search input");
await page.keyboard.press("ArrowDown");
check("ArrowDown moves from search into the results", await page.evaluate(() => document.activeElement?.classList.contains("result")));
await click(".row.result", "Fresh result");
await page.waitForSelector(".back");
await sleep(150);
check("opening a result shows it at 800px", await page.evaluate(() => getComputedStyle(document.querySelector(".main-content")).display !== "none"));
await click(".back");
await sleep(100);
check("Back returns to the list at 800px", (await page.evaluate(() => getComputedStyle(document.querySelector(".board")).display)) !== "none");

// Opening the session already open starts it over, and everything loads again.
await openApp();
await listReady();
await openSession("Codex session");
const reopenChecks = async (how) => {
  await sleep(300);
  check(`${how}: its stats come back`, (await count(".summary-loading")) === 0 && (await text(".summary"))?.includes("7 prompts"), await text(".summary"));
  check(`${how}: its tool results come back`, (await text(".summary"))?.includes("tool failure"));
  await click(".tabs button", "Changes");
  await sleep(50);
  check(`${how}: Changes finishes reading`, !(await text(".tab-panel")).includes("Reading what changed"));
  await click(".tabs button", "Agents");
  await sleep(50);
  check(`${how}: Agents finishes looking`, !(await text(".tab-panel")).includes("Looking for agents") && (await count(".agents li")) === 1);
  await click(".tabs button", "Conversation");
  await sleep(50);
};
const statsCalls = (await calls("get_session_stats")).length;
await click(".board .row", "Codex session");
await reopenChecks("re-clicking the open session");
check("re-clicking the open session loads it again", (await calls("get_session_stats")).length === statsCalls + 1);
await page.type(".search input", "reopen");
await sleep(450);
await click(".row.result", "Codex hit");
await reopenChecks("a search hit in the open session");

// Fold tools folds, and a fold that search opened closes with the search.
await openApp();
await listReady();
await openSession("Codex session");
await click(".activity-toggle");
await sleep(80);
check("Expand tools opens the folds", (await count(".activity[open]")) === 1 && (await text(".activity-toggle")) === "Fold tools");
await click(".activity-toggle");
await sleep(80);
check("Fold tools folds them again", (await count(".activity[open]")) === 0 && (await text(".activity-toggle")) === "Expand tools");
await page.type(".message-search-bar input", "cargo test");
await sleep(300);
check("search opens the fold holding a match", (await count(".activity[open]")) === 1);
await page.keyboard.press("Escape");
await sleep(100);
check("the fold closes when the search does", (await count(".activity[open]")) === 0);
await click(".activity summary");
await sleep(80);
await click(".activity-toggle");
await sleep(50);
await click(".activity-toggle");
await sleep(80);
check("a fold you opened yourself stays open", (await count(".activity[open]")) === 1);

// ⌘2 hides the list during a search, without searching again.
await openApp();
await listReady();
await openSession("Codex session");
await page.type(".search input", "er");
await sleep(450);
const searchesBefore = (await calls("global_search")).length;
await pressWithMeta("2");
await sleep(450);
const boardShown = () => page.evaluate(() => getComputedStyle(document.querySelector(".board")).display !== "none");
check("⌘2 hides the list during a search", !(await boardShown()));
check("hiding the list doesn't search again", (await calls("global_search")).length === searchesBefore);
await pressWithMeta("2");
check("⌘2 brings it back", await boardShown());
await click(".list-toggle");
await sleep(450);
check("the title-bar toggle hides it too", !(await boardShown()) && (await calls("global_search")).length === searchesBefore);
await page.type(".search input", "x");
await sleep(50);
check("typing again brings the results back", await boardShown());

// Slow-then-fast: whatever you opened last wins, whichever answer arrives last.
await openApp();
await listReady();
for (const [command, label] of [["get_session_messages", "messages"], ["get_tool_results", "tool results"], ["get_session_stats", "stats"]]) {
  await openSession("Live session");
  await page.evaluate((slowCommand) => {
    window.__backend.delays = { [slowCommand]: { "/p/live.jsonl": 500 } };
  }, command);
  await click(".board .row", "Live session");
  await sleep(30);
  await openSession("Codex session");
  await sleep(700);
  const summary = await text(".summary");
  const shownTurn = await text(".assistant-content");
  check(`a slow ${label} reply can't overwrite the newer session`, shownTurn.includes("All done.") && !shownTurn.includes("She said") && summary.includes("7 prompts") && summary.includes("tool failure"), `${summary} / ${shownTurn.slice(0, 60)}`);
  await page.evaluate(() => (window.__backend.delays = {}));
}

// Duplicates in the data don't take the app down.
await openApp({ extra: "duplicates" });
await sleep(700);
check("a session listed twice doesn't blank the app", (await count(".titlebar")) === 1 && (await count(".sidebar")) === 1 && (await count(".board")) + (await count(".pane-failed")) >= 1);
check("the list still works beside a duplicate, or says it stopped", (await count(".board .row")) >= 1 || (await text(".pane-failed"))?.includes("stopped working"));

// Search steps land on a highlighted word you can see, wherever it is.
await openApp({ extra: "search" });
await listReady();
await openSession("Search session");
await page.type(".message-search-bar input", "needle");
await sleep(500);
check("the count is the highlighted words", (await text(".match-count")) === "1 of 6" && (await count("mark.search-mark")) === 6, await text(".match-count"));
for (let step = 1; step <= 7; step += 1) {
  const expected = `${(step % 6) + 1} of 6`;
  await page.focus(".message-search-bar input");
  await page.keyboard.press("Enter");
  await sleep(350);
  check(`search step ${step} lands on a visible match`, (await text(".match-count")) === expected && (await count("mark.search-current")) === 1 && (await reallyVisible("mark.search-current")), await text(".match-count"));
}
await page.keyboard.down("Shift");
await page.keyboard.press("Enter");
await page.keyboard.up("Shift");
await sleep(350);
check("Shift+Enter steps back", (await text(".match-count")) === "1 of 6" && (await reallyVisible("mark.search-current")), await text(".match-count"));
await page.type(".search input", "needle");
await sleep(500);
await click(".row.result", "Needle hit");
await sleep(600);
check("a search result lands on its own match", await page.evaluate(() => document.querySelector("mark.search-current")?.closest(".assistant-content")?.textContent.includes("the last needle")) && (await reallyVisible("mark.search-current")));

// Markdown that used to lose its numbers, run off the edge, or could carry a script.
await openApp({ extra: "search" });
await listReady();
await openSession("Markdown session");
const listGeometry = await page.evaluate(() => {
  const list = document.querySelector(".markdown-body ol");
  return { style: getComputedStyle(list).listStyleType, indent: list.querySelector("li").getBoundingClientRect().left - document.querySelector(".message-slot").getBoundingClientRect().left };
});
check("numbered lists show their numbers", listGeometry.style === "decimal" && listGeometry.indent >= 24, JSON.stringify(listGeometry));
const tableScroll = await page.evaluate(() => {
  const wrapper = document.querySelector(".table-scroll");
  const slot = document.querySelector(".message-slot").getBoundingClientRect();
  wrapper.scrollLeft = 200;
  return { wide: wrapper.scrollWidth > wrapper.clientWidth, moved: wrapper.scrollLeft === 200, insideSlot: wrapper.getBoundingClientRect().right <= slot.right + 1 };
});
check("a wide table scrolls inside the message", tableScroll.wide && tableScroll.moved && tableScroll.insideSlot, JSON.stringify(tableScroll));
const overflowing = await page.evaluate(() => {
  // The message slot is what clips, so that's the edge that matters. Text rather than elements:
  // an inline code pill's padding may hang over without hiding anything.
  const edge = document.querySelector(".message-slot").getBoundingClientRect().right + 1;
  const walker = document.createTreeWalker(document.querySelector(".assistant-content"), NodeFilter.SHOW_TEXT);
  const contents = document.createRange();
  const cutOff = [];
  while (walker.nextNode()) {
    contents.selectNodeContents(walker.currentNode);
    if (!walker.currentNode.parentElement.closest(".table-scroll") && contents.getBoundingClientRect().right > edge) cutOff.push(walker.currentNode.textContent.slice(0, 30));
  }
  return cutOff;
});
check("long links and code stay inside the message", overflowing.length === 0, overflowing.join(" / "));
check("text in a message goes through the sanitizer", await page.evaluate(() => window.__backend.sanitized.some((markup) => markup.includes("sanitize-marker"))));
await sleep(100);
check("markup in a message stays text", await page.evaluate(() => {
  const body = document.querySelector(".markdown-body");
  return !window.__backend.attacked && body.querySelector("script, [onerror], a[href^='javascript']") === null && body.querySelector("img") === null && body.textContent.includes("<img src=x");
}));
const panelText = () => page.evaluate(() => document.querySelector(".messages-container").textContent);
check("a failed hook shows, a quiet one doesn't", (await panelText()).includes("A hook broke") && !(await panelText()).includes("A quiet hook ran"));
await click(".activity-toggle");
await sleep(80);
check("Expand tools shows the quiet hook too", (await panelText()).includes("A quiet hook ran"));
await click(".activity-toggle");
await sleep(80);
await page.evaluate(() => document.querySelector(".assistant-row .bookmark-btn").click());
await sleep(100);
check("bookmarking a Claude Code message also saves its session", (await calls("archive_session")).length === 1);
await page.evaluate(() => document.querySelector(".assistant-row .bookmark-btn").click());
await sleep(50);
check("a hook failure count says so", (await text(".summary"))?.includes("3 hooks failed"), await text(".summary"));
await click(".summary button.bad", "hooks failed");
await sleep(80);
check("the hook count opens Stats", (await text(".tabs button.active")).includes("Stats"));

// The answer you chose has to be readable in every theme.
for (const theme of ["dark", "light", "bright"]) {
  await openApp({ extra: "search", theme });
  await listReady();
  await openSession("Markdown session");
  const contrasts = await page.evaluate(() => {
    const paint = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
    const rgba = (color) => {
      paint.clearRect(0, 0, 1, 1);
      paint.fillStyle = color;
      paint.fillRect(0, 0, 1, 1);
      const [red, green, blue, alpha] = paint.getImageData(0, 0, 1, 1).data;
      return [red, green, blue, alpha / 255];
    };
    const over = (top, bottom) => [0, 1, 2].map((channel) => top[channel] * top[3] + bottom[channel] * (1 - top[3]));
    const backgroundOf = (element) => {
      const layers = [];
      for (let node = element; node; node = node.parentElement) layers.push(rgba(getComputedStyle(node).backgroundColor));
      return layers.reverse().reduce((below, layer) => over(layer, below), [255, 255, 255]);
    };
    const luminance = (color) => {
      const [red, green, blue] = color.map((channel) => (channel / 255 <= 0.03928 ? channel / 255 / 12.92 : ((channel / 255 + 0.055) / 1.055) ** 2.4));
      return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    };
    return Object.fromEntries([".question-option.chosen .option-label", ".question-tool", ".question-note-label", ".question-note-text"].map((selector) => {
      const element = document.querySelector(selector);
      if (!element) return [selector, 0];
      const background = backgroundOf(element);
      const [brighter, darker] = [luminance(over(rgba(getComputedStyle(element).color), background)), luminance(background)].sort((first, second) => second - first);
      return [selector, (brighter + 0.05) / (darker + 0.05)];
    }));
  });
  check(`the chosen answer reads in the ${theme} theme`, Object.values(contrasts).every((ratio) => ratio >= 4.5), JSON.stringify(contrasts));
}

// A dialog takes focus, keeps Tab inside, and keys don't reach what's behind it.
await openApp();
await listReady();
await openSession("Live session");
await click(".assistant-content .activity summary", "agent");
await page.waitForSelector(".assistant-content button.tool-pill");
await click(".assistant-content button.tool-pill", "helper");
await page.waitForSelector(".subagent-panel");
const openers = {
  Settings: () => pressWithMeta(","),
  Shortcuts: async () => {
    await page.focus(".messages-container");
    await page.keyboard.press("?");
    await sleep(80);
  },
  "Session details": async () => {
    await click(".header-action-btn[aria-label='Session details']");
    await sleep(80);
  },
};
for (const [name, openDialog] of Object.entries(openers)) {
  await openDialog();
  check(`${name} is a real dialog`, (await count("dialog.app-dialog[open]")) === 1);
  const insideDialog = () => page.evaluate(() => document.activeElement?.closest("dialog") !== null);
  check(`${name} takes focus`, await insideDialog());
  let staysInside = true;
  for (let press = 0; press < 25; press += 1) {
    await page.keyboard.press("Tab");
    staysInside &&= await insideDialog();
  }
  check(`Tab stays inside ${name}`, staysInside);
  const before = await page.evaluate(() => ({ heading: document.querySelector(".board-head h2").textContent, session: document.querySelector(".session-title").textContent, railHidden: document.querySelector(".app").classList.contains("rail-hidden"), listHidden: document.querySelector(".app").classList.contains("list-hidden"), tab: document.querySelector(".tabs button.active").textContent }));
  await page.keyboard.press("b");
  await page.keyboard.press("ArrowDown");
  await pressWithMeta("1");
  await pressWithMeta("2");
  await pressWithMeta("f");
  await pressWithMeta("k");
  await pressWithMeta(",");
  await sleep(100);
  const after = await page.evaluate(() => ({ heading: document.querySelector(".board-head h2").textContent, session: document.querySelector(".session-title").textContent, railHidden: document.querySelector(".app").classList.contains("rail-hidden"), listHidden: document.querySelector(".app").classList.contains("list-hidden"), tab: document.querySelector(".tabs button.active").textContent }));
  check(`keys don't reach what's behind ${name}`, JSON.stringify(before) === JSON.stringify(after) && (await count("dialog[open]")) === 1, JSON.stringify(after));
  await page.keyboard.press("Escape");
  await sleep(100);
  check(`Esc closes ${name} and nothing else`, (await count("dialog[open]")) === 0 && (await count(".subagent-panel")) === 1);
  check(`focus leaves ${name} when it closes`, !(await insideDialog()) || (await page.evaluate(() => document.activeElement === document.body)));
}
await openApp({ welcome: true });
await page.waitForSelector("dialog.welcome[open]");
check("the welcome dialog takes focus", await page.evaluate(() => document.activeElement?.closest("dialog.welcome") !== null));

// Esc at the smallest window always leaves something on screen, and a way back.
const paneShown = (selector) => page.evaluate((target) => {
  const element = document.querySelector(target);
  return !!element && getComputedStyle(element).display !== "none" && element.getBoundingClientRect().width > 0;
}, selector);
await openApp({}, { width: 800, height: 600 });
await listReady();
await page.type(".search input", "er");
await sleep(450);
await page.keyboard.press("Escape");
await sleep(100);
check("Esc from search with nothing open keeps the list at 800x600", await paneShown(".board"));
await openSession("Codex session");
check("the session fills the window at 800x600", (await paneShown(".main-content")) && !(await paneShown(".board")) && (await paneShown(".back")));
await page.type(".search input", "er");
await sleep(450);
check("searching shows results at 800x600", (await paneShown(".board")) && !(await paneShown(".main-content")));
await page.focus(".search input");
await page.keyboard.press("Escape");
await sleep(100);
check("Esc from search returns to the session at 800x600", (await paneShown(".main-content")) && !(await paneShown(".board")) && (await paneShown(".back")));
await page.focus(".messages-container");
await page.keyboard.press("Escape");
await sleep(100);
check("Esc from the conversation returns to the list at 800x600", (await paneShown(".board")) && !(await paneShown(".main-content")) && (await page.evaluate(() => document.activeElement?.classList.contains("selected"))));

// The failure count lands on the failed tool.
await openApp();
await listReady();
await openSession("Codex session");
await click(".summary button.bad", "tool failure");
await sleep(400);
check("a failure jump lands on the failed tool, visibly", (await text(".failure-current"))?.includes("cargo test") && (await reallyVisible(".failure-current")), await text(".failure-current"));
check("the count says which failure you're on", (await text(".summary button.bad")).includes("1 of 1"));
await openApp({ extra: "search" });
await listReady();
await openSession("Search session");
await click(".summary button.bad", "tool failure");
await sleep(500);
check("a failure jump scrolls to a failure far away", (await text(".failure-current"))?.includes("far away failure") && (await reallyVisible(".failure-current")), await text(".failure-current"));

// "Now" and New activity read the file's time; dates and order read the last message.
await openApp({ extra: "times" });
await listReady();
const rowTime = (title) => page.evaluate((needle) => [...document.querySelectorAll(".board .row")].find((row) => row.textContent.includes(needle))?.querySelector(".time").textContent.trim(), title);
check("a recently written file lights Now", (await rowTime("Still being written")) === "Now", await rowTime("Still being written"));
check("a quiet file doesn't", (await rowTime("Long quiet")) !== "Now", await rowTime("Long quiet"));
check("the list sorts by the last message, not the file", await page.evaluate(() => {
  const titles = [...document.querySelectorAll(".board .row .title")].map((title) => title.textContent);
  return titles.indexOf("Long quiet") < titles.indexOf("Still being written");
}));
await openSession("Long quiet");
const firePoll = async () => {
  await page.evaluate(() => window.__backend.intervals.forEach((callback) => callback()));
  await sleep(700);
};
await firePoll();
check("no New activity while the file stands still", (await count(".new-activity")) === 0);
await page.evaluate(() => (window.__backend.lastWritten["/p/quiet.jsonl"] = new Date().toISOString()));
await firePoll();
check("a file written since offers New activity", (await count(".new-activity")) === 1);
await click(".new-activity");
await sleep(700);
check("reloading clears New activity", (await count(".new-activity")) === 0);

// A bookmark opens its session at its message. A Codex one saves nothing, because Codex isn't archived.
await openApp({ extra: "search", bookmarks: [{ id: "far", role: "assistant", text: "the last needle", preview: "The far one", project_path: "/work/app", project_name: "app", session_id: "search", jsonl_path: "/p/search.jsonl", timestamp: "2026-01-02T00:02:00Z", created_at: 1 }] });
await listReady();
await page.keyboard.press("b");
await sleep(80);
await click(".saved-tabs button", "Messages");
await sleep(80);
await click(".bookmark-open", "The far one");
await page.waitForSelector(".bookmark-flash");
await sleep(400);
check("a bookmark lands on its message", (await page.evaluate(() => document.querySelector(".bookmark-flash").textContent.includes("the last needle"))) && (await reallyVisible(".bookmark-flash")));
await openApp();
await listReady();
await openSession("Codex session");
await page.evaluate(() => document.querySelector(".assistant-row .bookmark-btn").click());
await sleep(100);
check("bookmarking a Codex message saves nothing", (await calls("archive_session")).length === 0);

// A bookmark whose session has gone says so and can be removed.
const goneBookmark = { id: "gone", role: "user", text: "old", preview: "An old prompt", project_path: "/gone", project_name: "gone", session_id: "no-such-session", jsonl_path: "/gone/s.jsonl", timestamp: "2026-01-01T00:00:00Z", created_at: 1 };
await openApp({ bookmarks: [goneBookmark] });
await listReady();
await page.keyboard.press("b");
await sleep(80);
await click(".saved-tabs button", "Messages");
await sleep(80);
check("a gone bookmark says why it can't open", (await text(".gone"))?.includes("no longer on this Mac") && (await page.evaluate(() => document.querySelector(".bookmark-open").disabled)));
await click(".remove-text");
await sleep(80);
check("a gone bookmark can be removed", (await count(".bookmark")) === 0 && (await page.evaluate(() => localStorage.getItem("claude-sessions-bookmarks"))) === "[]");


check("no uncaught page errors", pageErrors.length === 0, pageErrors.join("; "));
await browser.close();
console.log(failures === 0 ? "\nAll UI checks passed." : `\n${failures} UI check(s) failed.`);
process.exit(failures === 0 ? 0 : 1);
