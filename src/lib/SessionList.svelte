<script lang="ts">
  import type { SessionInfo } from "./types";

  let {
    sessions,
    selectedSessionId,
    onSelect,
    sortOrder = "newest",
    onSortChange,
    tokenMap = new Map(),
  }: {
    sessions: SessionInfo[];
    selectedSessionId: string | null;
    onSelect: (session: SessionInfo) => void;
    sortOrder: "newest" | "oldest";
    onSortChange: (order: "newest" | "oldest") => void;
    tokenMap?: Map<string, number>;
  } = $props();

  let searchQuery = $state("");

  function fuzzyMatch(text: string | null | undefined, queryWords: string[]): boolean {
    if (!text) return false;
    const lower = text.toLowerCase();
    return queryWords.every((word) => lower.includes(word));
  }

  let filteredSessions = $derived(
    sessions.filter((session) => {
      if (!searchQuery) return true;
      const queryWords = searchQuery.toLowerCase().split(/\s+/).filter(Boolean);
      if (queryWords.length === 0) return true;
      return (
        fuzzyMatch(session.summary, queryWords) ||
        fuzzyMatch(session.first_prompt, queryWords) ||
        fuzzyMatch(session.project_name, queryWords) ||
        fuzzyMatch(session.session_id, queryWords)
      );
    })
  );

  function formatDate(isoDate: string | null): string {
    if (!isoDate) return "";
    const date = new Date(isoDate);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

    if (diffDays === 0) {
      return date.toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
      });
    } else if (diffDays === 1) {
      return "Yesterday";
    } else if (diffDays < 7) {
      return `${diffDays}d ago`;
    } else {
      return date.toLocaleDateString([], { month: "short", day: "numeric" });
    }
  }

  function formatTokens(tokens: number): string {
    if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
    if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
    return `${tokens}`;
  }

  function displayName(session: SessionInfo): string {
    if (session.summary) return session.summary.replace(/\*\*/g, "");
    if (session.first_prompt) {
      let prompt = session.first_prompt
        .replace(/<[^>]+>/g, "")                          // XML tags
        .replace(/['"`]?\/[\w\-./]+['"`]?\s*/g, "")       // Any absolute file paths
        .replace(/['"`]?\.\/[\w\-./]+['"`]?\s*/g, "")     // Relative paths
        .replace(/\b\w+\.(ts|js|svelte|rs|json|md|py|go|jsx|tsx|css|html)\b/g, "") // Bare filenames
        .replace(/```[\s\S]*?```/g, "")                    // Code blocks
        .replace(/\[.*?\]\(.*?\)/g, "")                    // Markdown links
        .replace(/\n+/g, " ")                              // Newlines
        .replace(/\s+/g, " ")
        .trim();
      if (!prompt || prompt.length < 10) {
        const paths = session.first_prompt.match(/\/[\w\-./]+/g);
        if (paths && paths.length > 0) {
          const lastPath = paths[paths.length - 1];
          const segments = lastPath.split("/").filter(Boolean);
          prompt = segments.slice(-2).join("/");
        } else {
          prompt = session.first_prompt.replace(/\s+/g, " ").trim();
        }
      }
      return prompt.length > 80 ? prompt.slice(0, 80) + "..." : prompt;
    }
    return session.session_id.slice(0, 8);
  }

  function getDateGroup(isoDate: string | null): string {
    if (!isoDate) return "older";
    const date = new Date(isoDate);
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const yesterday = new Date(today.getTime() - 86400000);
    const weekAgo = new Date(today.getTime() - 7 * 86400000);

    if (date >= today) return "today";
    if (date >= yesterday) return "yesterday";
    if (date >= weekAgo) return "previous7";
    return "older";
  }

  const groupLabels: Record<string, string> = {
    today: "TODAY",
    yesterday: "YESTERDAY",
    previous7: "PREVIOUS 7 DAYS",
    older: "OLDER",
  };

  let groupedSessions = $derived(() => {
    const groups: { label: string; sessions: typeof sessions }[] = [];
    const order = ["today", "yesterday", "previous7", "older"];
    const grouped = new Map<string, typeof sessions>();

    for (const group of order) grouped.set(group, []);

    for (const session of filteredSessions) {
      const group = getDateGroup(session.modified);
      grouped.get(group)!.push(session);
    }

    for (const group of order) {
      const items = grouped.get(group)!;
      if (items.length > 0) {
        groups.push({ label: groupLabels[group], sessions: items });
      }
    }
    return groups;
  });

  // Flat list of all visible sessions for keyboard navigation
  let flatSessions = $derived(
    groupedSessions().flatMap((group) => group.sessions)
  );

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    const target = event.target as HTMLElement;
    if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;
    if (flatSessions.length === 0) return;
    event.preventDefault();

    const currentIndex = flatSessions.findIndex(
      (session) => session.session_id === selectedSessionId
    );

    let nextIndex: number;
    if (event.key === "ArrowDown") {
      nextIndex = currentIndex < flatSessions.length - 1 ? currentIndex + 1 : 0;
    } else {
      nextIndex = currentIndex > 0 ? currentIndex - 1 : flatSessions.length - 1;
    }

    onSelect(flatSessions[nextIndex]);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="session-list">
  <div class="session-list-header">
    <h2>Sessions</h2>
    <button
      class="sort-btn"
      onclick={() => onSortChange(sortOrder === "newest" ? "oldest" : "newest")}
      title="Sort order"
    >
      {sortOrder === "newest" ? "Newest" : "Oldest"}
    </button>
  </div>

  <div class="search-bar">
    <input
      type="text"
      placeholder="Filter sessions..."
      autocomplete="off"
      spellcheck="false"
      bind:value={searchQuery}
    />
  </div>

  <div class="sessions-scroll">
    {#each groupedSessions() as group}
      <div class="date-group-label">{group.label}</div>
      {#each group.sessions as session (session.session_id)}
        <button
          class="session-item"
          class:selected={selectedSessionId === session.session_id}
          onclick={() => onSelect(session)}
        >
          <div class="session-name">{displayName(session)}</div>
          <div class="session-meta">
            {#if session.conversation_count > 0}
              <span class="stat-badge" title="Conversations">
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>
                {session.conversation_count}
              </span>
            {:else if session.message_count}
              <span class="stat-badge" title="Messages">
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>
                {session.message_count}
              </span>
            {/if}
            {#if tokenMap.get(session.session_id) !== undefined}
              <span class="stat-badge" title="Tokens (input + output)">
                {formatTokens(tokenMap.get(session.session_id)!)}
              </span>
            {/if}
            <span class="session-date">{formatDate(session.modified)}</span>
          </div>
        </button>
      {/each}
    {/each}

    {#if filteredSessions.length === 0}
      <div class="empty-state">No sessions found</div>
    {/if}
  </div>
</div>

<style>
  .session-list {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #1a1a2e;
  }

  .session-list-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px;
    border-bottom: 1px solid #2a2a4a;
  }

  .session-list-header h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: #e0e0e0;
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }

  .sort-btn {
    background: #2a2a4a;
    border: none;
    color: #a0a0c0;
    font-size: 11px;
    padding: 4px 10px;
    border-radius: 4px;
    cursor: pointer;
  }

  .sort-btn:hover {
    background: #3a3a5a;
    color: #e0e0e0;
  }

  .search-bar {
    padding: 8px 16px;
  }

  .search-bar input {
    width: 100%;
    background: #12121e;
    border: 1px solid #2a2a4a;
    color: #e0e0e0;
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    outline: none;
    box-sizing: border-box;
  }

  .search-bar input:focus {
    border-color: #6366f1;
  }

  .search-bar input::placeholder {
    color: #5a5a7a;
  }

  .sessions-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 4px 8px;
    -webkit-font-smoothing: antialiased;
  }

  .date-group-label {
    font-size: 10px;
    font-weight: 600;
    color: #5a5a7a;
    letter-spacing: 0.08em;
    padding: 8px 12px 6px;
    margin-top: 12px;
    border-top: 1px solid #222240;
    text-transform: uppercase;
  }

  .date-group-label:first-child {
    margin-top: 4px;
    border-top: none;
  }

  .session-item {
    display: block;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    color: #c0c0d8;
    padding: 11px 12px;
    margin: 1px 0;
    border-radius: 8px;
    cursor: pointer;
    transition: background 0.12s ease;
  }

  .session-item:hover {
    background: rgba(99, 102, 241, 0.06);
  }

  .session-item.selected {
    background: rgba(99, 102, 241, 0.12);
    box-shadow: inset 3px 0 0 0 #6366f1;
  }

  .session-item.selected .session-name {
    color: #ececff;
  }

  .session-name {
    font-size: 13px;
    font-weight: 600;
    color: #d8d8f0;
    line-height: 1.35;
    margin-bottom: 4px;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .session-meta {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .stat-badge {
    font-size: 11px;
    color: #585878;
    display: flex;
    align-items: center;
    gap: 4px;
    line-height: 1;
    padding: 2px 6px;
    background: rgba(255, 255, 255, 0.04);
    border-radius: 4px;
  }

  .session-date {
    font-size: 11px;
    color: #505070;
    margin-left: auto;
    line-height: 1;
  }

  .empty-state {
    padding: 40px 16px;
    text-align: center;
    color: #5a5a7a;
    font-size: 13px;
  }

  .sessions-scroll::-webkit-scrollbar {
    width: 6px;
  }

  .sessions-scroll::-webkit-scrollbar-track {
    background: transparent;
  }

  .sessions-scroll::-webkit-scrollbar-thumb {
    background: transparent;
    border-radius: 3px;
    transition: background 0.2s;
  }

  .sessions-scroll:hover::-webkit-scrollbar-thumb {
    background: #2a2a4a;
  }
</style>
