import type { Bookmark } from "./types";

const STORAGE_KEY = "claude-sessions-bookmarks";

function isBookmark(entry: unknown): entry is Bookmark {
  if (typeof entry !== "object" || entry === null) return false;
  const fields = entry as Record<string, unknown>;
  return ["id", "session_id", "timestamp", "preview", "project_name"].every((field) => typeof fields[field] === "string");
}

function load(): Bookmark[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    // Storage can be edited or damaged: one bad entry mustn't take the Saved view down.
    const seen = new Set<string>();
    return parsed.filter((entry) => {
      if (!isBookmark(entry) || seen.has(entry.id)) return false;
      seen.add(entry.id);
      return true;
    });
  } catch {
    return [];
  }
}

// Reactive bookmark list, persisted to localStorage. Same pattern as preferences.svelte.ts.
export const bookmarks = $state<Bookmark[]>(load());

export function isBookmarked(bookmarkId: string): boolean {
  return bookmarks.some((bookmark) => bookmark.id === bookmarkId);
}

export function toggleBookmark(bookmark: Bookmark): void {
  const existingIndex = bookmarks.findIndex((entry) => entry.id === bookmark.id);
  if (existingIndex !== -1) {
    bookmarks.splice(existingIndex, 1);
  } else {
    bookmarks.unshift(bookmark);
  }
  persist();
}

export function removeBookmark(bookmarkId: string): void {
  const existingIndex = bookmarks.findIndex((entry) => entry.id === bookmarkId);
  if (existingIndex !== -1) {
    bookmarks.splice(existingIndex, 1);
    persist();
  }
}

// Deterministic id so the star reflects "already saved" and toggling is idempotent.
// session + timestamp would usually suffice; the text hash guards the rare case of
// two messages sharing both.
export function makeBookmarkId(
  sessionId: string,
  timestamp: string,
  text: string
): string {
  return `${sessionId}::${timestamp}::${hashText(text)}`;
}

function hashText(text: string): string {
  let hash = 5381;
  for (let index = 0; index < text.length; index++) {
    hash = ((hash << 5) + hash + text.charCodeAt(index)) | 0;
  }
  return (hash >>> 0).toString(36);
}

function persist(): void {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(bookmarks));
  } catch {
    // localStorage can throw (quota/private mode) — bookmarks just won't persist
  }
}
