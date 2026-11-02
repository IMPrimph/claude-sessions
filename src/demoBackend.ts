// The website's working demo: the real app over made-up sessions, held in the visitor's browser.
// Answers each backend command from src/demoData.json, which is the real parser's output for
// invented transcripts (scripts/build_demo_data.py). Reads no file and sends nothing. Only
// `vite build --mode demo` includes this; the desktop app never does.

import demoData from "./demoData.json";
import { version } from "../package.json";
import type { ConversationMessage, GlobalSearchResult, SessionInfo } from "./lib/types";

type Arguments = Record<string, unknown>;

const NOT_IN_DEMO = "This demo runs in your browser, so it can't reach your files. The app can.";

// Everything is dated as if it happened just now, whenever the page is opened.
const offset = Date.now() - Date.parse(demoData.anchor);
const data: typeof demoData = JSON.parse(
  JSON.stringify(demoData).replace(/\d{4}-\d\d-\d\dT[\d:.]+Z/g, (timestamp) => new Date(Date.parse(timestamp) + offset).toISOString()),
);
const allSessions = data.sessions as unknown as SessionInfo[];
const details = data.details as Record<string, (typeof data.details)[keyof typeof data.details]>;
const detail = (path: unknown) => details[String(path)];
const live = allSessions.filter((session) => session.session_id !== data.saved_only);
const saved = new Set([data.saved_only]);

function search(query: string, tool: string | null): GlobalSearchResult[] {
  const needle = query.toLowerCase();
  const results: GlobalSearchResult[] = [];
  for (const session of allSessions) {
    if (tool && session.tool !== tool) continue;
    const name = session.custom_title ?? session.ai_title ?? session.first_prompt ?? "";
    const base = { session_id: session.session_id, tool: session.tool, project_name: session.project_name, project_path: session.project_path, session_name: name, jsonl_path: session.jsonl_path };
    for (const message of detail(session.jsonl_path).messages as ConversationMessage[]) {
      const at = message.text.toLowerCase().indexOf(needle);
      if (at < 0) continue;
      results.push({ ...base, matched_text: message.text.slice(Math.max(0, at - 50), at + 110), match_source: "message", timestamp: message.timestamp });
    }
  }
  return results;
}

function answer(command: string, args: Arguments): unknown {
  switch (command) {
    case "scan_projects": return live;
    case "get_projects": return data.projects;
    case "get_read_problems": return [];
    case "get_archived_sessions": return allSessions.filter((session) => saved.has(session.session_id));
    case "existing_folders": return args.paths;
    case "get_session_messages": return detail(args.jsonlPath).messages;
    case "get_tool_results": return detail(args.jsonlPath).results;
    case "get_session_questions": return detail(args.jsonlPath).questions;
    case "get_session_artifacts": return detail(args.jsonlPath).artifacts;
    case "get_session_stats": return detail(args.jsonlPath).stats;
    case "get_session_file_changes": return detail(args.jsonlPath).files;
    case "list_subagents": return detail(args.jsonlPath).subagents;
    case "get_subagent_messages":
      return Object.values(details).flatMap((session) => session.subagent_transcripts).find((agent) => agent.path === args.jsonlPath)?.messages ?? [];
    case "get_tool_result_images": return [];
    case "get_image_path":
    case "read_local_image": return null;
    case "global_search": return search(String(args.query), (args.tool as string | null) ?? null);
    case "archive_session": saved.add(String(args.sessionId)); return null;
    case "unarchive_session": saved.delete(String(args.sessionId)); return null;
    case "get_archived_session_path":
      return saved.has(String(args.sessionId)) ? allSessions.find((session) => session.session_id === args.sessionId)?.jsonl_path ?? null : null;
    case "get_archive_info": return { path: "/Users/alex/Library/Application Support/Evertusk/archive", session_count: saved.size, total_bytes: saved.size * 48000, is_custom: false };
    case "open_external_url":
      // The made-up artifact in the sessions isn't a real page.
      if (String(args.url).includes("/artifact/")) throw NOT_IN_DEMO;
      window.open(String(args.url), "_blank", "noopener");
      return null;
    case "plugin:app|version": return version;
    case "plugin:updater|check": return null;
    case "plugin:clipboard-manager|write_text": return navigator.clipboard.writeText(String(args.text));
    default: throw NOT_IN_DEMO;
  }
}

(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
  transformCallback: () => 0,
  convertFileSrc: (path: string) => path,
  invoke: async (command: string, args: Arguments = {}) => answer(command, args),
};

// No welcome screen, and the title bar's room for the Mac's window buttons gets drawn ones.
try {
  const key = "claude-sessions-prefs";
  localStorage.setItem(key, JSON.stringify({ ...JSON.parse(localStorage.getItem(key) ?? "{}"), hasSeenWelcome: true }));
} catch {
  // Blocked storage: the welcome screen shows, and the demo still works.
}
const style = document.createElement("style");
style.textContent = `.titlebar{position:relative}.titlebar::before{content:"";position:absolute;left:18px;top:17px;width:12px;height:12px;border-radius:50%;background:#ff5f57;box-shadow:20px 0 #febc2e,40px 0 #28c840}`;
document.head.append(style);

// Opens on a session with something to show, once the list is up.
const opener = setInterval(() => {
  const row = [...document.querySelectorAll<HTMLElement>(".board .row")].find((candidate) => candidate.textContent?.includes("Fix double charge"));
  if (!row) return;
  clearInterval(opener);
  row.click();
}, 50);
