// Pretty-print a tool name for display. MCP tools use double-underscore namespacing,
// e.g. "mcp__team-db__query" → "team-db / query".
export function prettyToolName(name: string): string {
  if (!name.startsWith("mcp__")) return name;
  const parts = name.slice(5).split("__");
  if (parts.length < 2) return name.slice(5);
  const server = parts[0];
  const method = parts.slice(1).join(".");
  return `${server} / ${method}`;
}

import type { ProjectInfo, SessionInfo } from "./types";

// A timestamp as a number, for comparing and sorting: timestamps can carry different UTC
// offsets, so comparing the strings gets the order wrong. Missing or invalid sorts as oldest.
export function timeMs(isoDate: string | null | undefined): number {
  const parsed = isoDate ? Date.parse(isoDate) : NaN;
  return Number.isNaN(parsed) ? 0 : parsed;
}

// Compact token count formatting: 1234567 → "1.2M", 12345 → "12.3k"
export function formatTokenCompact(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
  return `${tokens}`;
}

// Friendly model labels: "claude-opus-4-7" → "Opus 4.7"
export function formatModel(model: string): string {
  const match = model.match(/claude-([a-z]+)-(\d+)-(\d+)/);
  if (!match) return model;
  const family = match[1].charAt(0).toUpperCase() + match[1].slice(1);
  return `${family} ${match[2]}.${match[3]}`;
}

// 267945 → "4m 28s", 8200 → "8.2s", 9000000 → "2h 30m"
export function formatDuration(milliseconds: number): string {
  const totalSeconds = Math.round(milliseconds / 1000);
  if (totalSeconds < 60) return `${(milliseconds / 1000).toFixed(1)}s`;
  const minutes = Math.floor(totalSeconds / 60);
  if (minutes < 60) return `${minutes}m ${totalSeconds % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

// Detect a slash command prompt like `<command-name>/effort</command-name><command-args>max</command-args>`
// and render it as "/effort max" — otherwise the XML strip leaves nothing usable.
function extractSlashCommand(prompt: string): string | null {
  const nameMatch = prompt.match(/<command-name>([^<]+)<\/command-name>/);
  if (!nameMatch) return null;
  const name = nameMatch[1].trim();
  if (!name.startsWith("/")) return null;
  const argsMatch = prompt.match(/<command-args>([^<]*)<\/command-args>/);
  const args = argsMatch?.[1].trim();
  return args ? `${name} ${args}` : name;
}

// Try aggressive cleanup first, fall back to lightly-cleaned original if cleanup
// strips too much. Avoids landing on a UUID for prompts that are mostly paths/code.
function formatFirstPrompt(prompt: string, maxLength: number): string {
  const capped = (text: string) =>
    text.length > maxLength ? text.slice(0, maxLength) + "..." : text;
  const aggressive = prompt
    .replace(/<[^>]+>/g, "") // XML tags
    .replace(/['"`]?\/[\w\-./]+['"`]?\s*/g, "") // Absolute paths
    .replace(/['"`]?\.\/[\w\-./]+['"`]?\s*/g, "") // Relative paths
    .replace(/\b\w+\.(ts|js|svelte|rs|json|md|py|go|jsx|tsx|css|html)\b/g, "") // Bare filenames
    .replace(/```[\s\S]*?```/g, "") // Code blocks
    .replace(/\[.*?\]\(.*?\)/g, "") // Markdown links
    .replace(/\s+/g, " ")
    .trim();
  if (aggressive.length >= 10) return capped(aggressive);

  // Aggressive cleanup over-stripped — keep the original, just normalize whitespace
  // and strip code blocks (which look terrible inline anyway).
  const soft = prompt.replace(/```[\s\S]*?```/g, " ").replace(/\s+/g, " ").trim();
  if (soft.length >= 4) return capped(soft);
  return prompt.slice(0, maxLength);
}

// A session's display title, in order of how deliberately it was named.
export function sessionTitle(session: SessionInfo, maxLength = 120): string {
  if (session.custom_title) return session.custom_title;
  if (session.summary) return session.summary.replace(/\*\*/g, "");
  if (session.ai_title) return session.ai_title.replace(/^Codex Companion Task:\s*/, "").replace(/<\/?task>\s*/g, "");
  if (session.first_prompt) {
    return extractSlashCommand(session.first_prompt) ?? formatFirstPrompt(session.first_prompt, maxLength);
  }
  return `Session ${session.session_id.slice(0, 8)}`;
}

// A default file name for an export: keeps the title's own language, drops only what
// file systems forbid, and never ends in a space or dot.
export function exportFileName(title: string, fallback: string): string {
  const cleaned = title.replace(/[/\\:*?"<>|\p{Cc}]/gu, " ").replace(/\s+/g, " ").trim().replace(/^\.+/, "");
  return [...cleaned].slice(0, 60).join("").replace(/[\s.]+$/, "") || fallback;
}

// What to call each project: its folder name, plus as many parent folders as it takes to
// tell apart folders that share a name.
export function projectLabels(projects: ProjectInfo[]): Map<string, string> {
  const labels = new Map<string, string>();
  for (const project of projects) {
    const levels = (path: string) => path.split("/").filter(Boolean);
    const labelAt = (path: string, depth: number) => levels(path).slice(-depth).join("/");
    const twins = projects.filter((other) => other.project_name === project.project_name && other.project_path !== project.project_path);
    let depth = 1;
    while (depth < levels(project.project_path).length && twins.some((other) => labelAt(other.project_path, depth) === labelAt(project.project_path, depth))) depth += 1;
    const label = twins.length > 0 ? labelAt(project.project_path, depth) : project.project_name;
    labels.set(project.project_path, label || project.project_name || project.project_path || "Unknown folder");
  }
  return labels;
}

// A tool's name as people say it. Unknown tools still get a readable label.
export function formatToolName(tool: string): string {
  if (tool === "claude") return "Claude";
  if (tool === "codex") return "Codex";
  return tool.charAt(0).toUpperCase() + tool.slice(1);
}

// A search's separate words: matching is by word anywhere in a message, not by exact phrase,
// in search everything and in a session alike.
export function searchWords(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

// A session row's time: short enough for a fixed column, whichever date format is chosen.
// dayIsShown: a heading above the row already names the day, so only the time is needed.
export function formatRowTime(isoDate: string | null, now: number, dateFormat: "relative" | "absolute", dayIsShown = false): string {
  if (!isoDate) return "";
  const date = new Date(isoDate);
  if (Number.isNaN(date.getTime())) return "";
  const today = new Date(now);
  const isToday = date.toDateString() === today.toDateString();
  if (isToday || dayIsShown) return date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  // Calendar days, so 29 hours ago at 1am is two days back rather than yesterday.
  const startOfDay = (day: Date) => new Date(day.getFullYear(), day.getMonth(), day.getDate()).getTime();
  const diffDays = Math.round((startOfDay(today) - startOfDay(date)) / 86400000);
  if (dateFormat === "relative" && diffDays >= 0 && diffDays < 7) return diffDays <= 1 ? "Yesterday" : `${diffDays} days ago`;
  return date.toLocaleDateString([], {
    month: "short",
    day: "numeric",
    ...(date.getFullYear() === today.getFullYear() ? {} : { year: "2-digit" }),
  });
}
