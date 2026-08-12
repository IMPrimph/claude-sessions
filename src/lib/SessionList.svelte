<script lang="ts">
  import type { SessionInfo } from "./types";
  import { preferences, toggleDateFormat } from "./preferences.svelte";
  import { formatTokenCompact } from "./format";

  let {
    sessions,
    selectedSessionId,
    onSelect,
    sortOrder = "newest",
    onSortChange,
    tokenMap = new Map(),
    savedSessionIds = new Set(),
  }: {
    sessions: SessionInfo[];
    selectedSessionId: string | null;
    onSelect: (session: SessionInfo) => void;
    sortOrder: "newest" | "oldest";
    onSortChange: (order: "newest" | "oldest") => void;
    tokenMap?: Map<string, number>;
    // Ids of sessions with a local archived copy — shown with a "saved" pin.
    savedSessionIds?: Set<string>;
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
        fuzzyMatch(session.custom_title, queryWords) ||
        fuzzyMatch(session.summary, queryWords) ||
        fuzzyMatch(session.ai_title, queryWords) ||
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

    if (preferences.dateFormat === "absolute") {
      const sameDay = date.toDateString() === now.toDateString();
      const sameYear = date.getFullYear() === now.getFullYear();
      const time = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      if (sameDay) return `Today ${time}`;
      const datePart = date.toLocaleDateString([], {
        month: "short",
        day: "numeric",
        ...(sameYear ? {} : { year: "numeric" }),
      });
      return `${datePart}, ${time}`;
    }

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

  function displayName(session: SessionInfo): string {
    if (session.custom_title) return session.custom_title;
    if (session.summary) return session.summary.replace(/\*\*/g, "");
    if (session.ai_title) return session.ai_title;
    if (session.first_prompt) {
      const slash = extractSlashCommand(session.first_prompt);
      if (slash) return slash;
      return formatFirstPrompt(session.first_prompt, 80);
    }
    return session.session_id.slice(0, 8);
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
    const aggressive = prompt
      .replace(/<[^>]+>/g, "")                          // XML tags
      .replace(/['"`]?\/[\w\-./]+['"`]?\s*/g, "")       // Absolute paths
      .replace(/['"`]?\.\/[\w\-./]+['"`]?\s*/g, "")     // Relative paths
      .replace(/\b\w+\.(ts|js|svelte|rs|json|md|py|go|jsx|tsx|css|html)\b/g, "") // Bare filenames
      .replace(/```[\s\S]*?```/g, "")                    // Code blocks
      .replace(/\[.*?\]\(.*?\)/g, "")                    // Markdown links
      .replace(/\n+/g, " ")
      .replace(/\s+/g, " ")
      .trim();

    if (aggressive.length >= 10) {
      return aggressive.length > maxLength
        ? aggressive.slice(0, maxLength) + "..."
        : aggressive;
    }

    // Aggressive cleanup over-stripped — keep the original, just normalize whitespace
    // and strip code blocks (which look terrible inline anyway).
    const soft = prompt
      .replace(/```[\s\S]*?```/g, " ")
      .replace(/\s+/g, " ")
      .trim();

    if (soft.length >= 4) {
      return soft.length > maxLength ? soft.slice(0, maxLength) + "..." : soft;
    }

    return prompt.slice(0, maxLength);
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

  let groupedSessions = $derived.by(() => {
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
    groupedSessions.flatMap((group) => group.sessions)
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
    <div class="header-actions">
      <button
        class="sort-btn"
        onclick={toggleDateFormat}
        title="Toggle relative / absolute dates"
      >
        {preferences.dateFormat === "relative" ? "Relative" : "Absolute"}
      </button>
      <button
        class="sort-btn"
        onclick={() => onSortChange(sortOrder === "newest" ? "oldest" : "newest")}
        title="Sort order"
      >
        {sortOrder === "newest" ? "Newest" : "Oldest"}
      </button>
    </div>
  </div>

  <div class="search-bar">
    <input
      type="text"
      placeholder={searchQuery
        ? `${filteredSessions.length} of ${sessions.length} match`
        : `Filter ${sessions.length} ${sessions.length === 1 ? "session" : "sessions"}...`}
      autocomplete="off"
      spellcheck="false"
      bind:value={searchQuery}
    />
  </div>

  <div class="sessions-scroll">
    {#each groupedSessions as group}
      <div class="date-group-label">
        {group.label}
        <span class="group-count">{group.sessions.length}</span>
      </div>
      {#each group.sessions as session (session.session_id)}
        <button
          class="session-item"
          class:selected={selectedSessionId === session.session_id}
          onclick={() => onSelect(session)}
        >
          <div class="session-name-row">
            <div class="session-name">{displayName(session)}</div>
            {#if savedSessionIds.has(session.session_id)}
              <svg class="saved-pin" width="12" height="12" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1.5" aria-label="Saved"><title>Saved — kept on your machine</title><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
            {/if}
          </div>
          {#if session.forked_from_session_id}
            {@const parent = sessions.find((entry) => entry.session_id === session.forked_from_session_id)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <span
              class="fork-badge"
              class:fork-linked={!!parent}
              title={parent
                ? `Forked from "${displayName(parent)}" — click to open it`
                : "Forked from another session (parent not in this project)"}
              onclick={(event) => {
                if (parent) {
                  event.stopPropagation();
                  onSelect(parent);
                }
              }}
            >
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="6" y1="3" x2="6" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/></svg>
              <span class="fork-label">forked from</span><span class="fork-parent">{parent ? displayName(parent) : "a session"}</span>
            </span>
          {/if}
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
                {formatTokenCompact(tokenMap.get(session.session_id)!)}
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
    flex: 1;
    min-height: 0;
    background: var(--bg-sidebar);
  }

  .session-list-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .session-list-header h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .sort-btn {
    background: var(--border);
    border: none;
    color: var(--text-secondary);
    font-size: 11px;
    padding: 4px 10px;
    border-radius: 4px;
    cursor: pointer;
  }

  .sort-btn:hover {
    background: var(--border-strong);
    color: var(--text-primary);
  }

  .search-bar {
    padding: 8px 16px;
    flex-shrink: 0;
  }

  .search-bar input {
    width: 100%;
    background: var(--bg-app);
    border: 1px solid var(--border);
    color: var(--text-primary);
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    outline: none;
    box-sizing: border-box;
  }

  .search-bar input:focus {
    border-color: var(--accent);
  }

  .search-bar input::placeholder {
    color: var(--text-faint);
  }

  .sessions-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 4px 8px;
    -webkit-font-smoothing: antialiased;
  }

  .date-group-label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 10px;
    font-weight: 600;
    color: var(--text-faint);
    letter-spacing: 0.08em;
    padding: 8px 12px 6px;
    margin-top: 12px;
    border-top: 1px solid var(--bg-hover);
    text-transform: uppercase;
  }

  .group-count {
    font-weight: 500;
    color: var(--border-strong);
    letter-spacing: 0;
    text-transform: none;
    font-size: 10px;
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
    color: var(--text-secondary);
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
    box-shadow: inset 3px 0 0 0 var(--accent);
  }

  .session-item.selected .session-name {
    color: var(--text-primary);
  }

  .session-name-row {
    display: flex;
    align-items: flex-start;
    gap: 6px;
  }

  .session-name-row .session-name {
    flex: 1;
    min-width: 0;
  }

  .saved-pin {
    flex-shrink: 0;
    margin-top: 2px;
    color: #f4bf5f;
  }

  /* Subtle "forked from …" subtitle — no filled box, GitHub-style. */
  .fork-badge {
    display: flex;
    align-items: center;
    gap: 5px;
    max-width: 100%;
    margin: 4px 0 0;
    font-size: 11px;
    color: var(--text-muted);
  }

  .fork-badge svg {
    flex-shrink: 0;
    opacity: 0.65;
  }

  .fork-label {
    flex-shrink: 0;
  }

  .fork-parent {
    font-weight: 500;
    color: var(--accent-hover);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fork-linked {
    cursor: pointer;
  }

  .fork-linked:hover .fork-parent {
    text-decoration: underline;
  }

  .session-name {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
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
    color: var(--text-faint);
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
    color: var(--border-strong);
    margin-left: auto;
    line-height: 1;
  }

  .empty-state {
    padding: 40px 16px;
    text-align: center;
    color: var(--text-faint);
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
    background: var(--border);
  }
</style>
