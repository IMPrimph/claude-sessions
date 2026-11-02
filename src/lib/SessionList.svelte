<script lang="ts">
  import { tick } from "svelte";
  import type { Bookmark, GlobalSearchResult, SessionInfo } from "./types";
  import { preferences } from "./preferences.svelte";
  import ToolIcon from "./ToolIcon.svelte";
  import { formatRowTime, formatToolName, sessionTitle, timeMs } from "./format";
  import { dialogIsOpen } from "./dialog";

  let {
    heading,
    sessions,
    allSessions,
    selectedPath,
    labels,
    savedSessionIds,
    loading,
    showProject,
    searchQuery,
    searchResults,
    searching,
    savedMode,
    bookmarks,
    toolFilter,
    problems,
    onClearToolFilter,
    onSelect,
    onOpenResult,
    onOpenBookmark,
    onRemoveBookmark,
  }: {
    heading: string;
    sessions: SessionInfo[];
    // Every session, so a fork can name a parent outside the current filter.
    allSessions: SessionInfo[];
    // The open session's transcript. Ids can repeat across folders; paths can't.
    selectedPath: string | null;
    // Project labels that tell same-name folders apart.
    labels: Map<string, string>;
    savedSessionIds: Set<string>;
    loading: boolean;
    // Across projects, each row needs to say which project it belongs to.
    showProject: boolean;
    searchQuery: string;
    searchResults: GlobalSearchResult[];
    searching: boolean;
    savedMode: boolean;
    bookmarks: Bookmark[];
    toolFilter: string;
    // Session folders that exist but can't be read.
    problems: string[];
    onClearToolFilter: () => void;
    onSelect: (session: SessionInfo) => void;
    onOpenResult: (result: GlobalSearchResult) => void;
    onOpenBookmark: (bookmark: Bookmark) => void;
    onRemoveBookmark: (bookmarkId: string) => void;
  } = $props();

  const RUNNING_WINDOW_MS = 2 * 60 * 1000;
  const CLOCK_TOLERANCE_MS = 60 * 1000;
  // The backend returns at most this many search results.
  const RESULT_LIMIT = 50;

  let newestFirst = $state(true);
  let savedTab = $state<"sessions" | "messages">("sessions");
  // Time moves on while the app stays open, so "running" and relative times need a clock.
  let now = $state(Date.now());

  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 30000);
    return () => clearInterval(timer);
  });

  let isSearching = $derived(searchQuery.trim().length > 0);
  let showingMessages = $derived(savedMode && savedTab === "messages");

  let orderedSessions = $derived(newestFirst ? sessions : [...sessions].reverse());

  // Names match the moment you type, while the slower search reads every message. Only the
  // title counts: the search behind it doesn't look at project names either.
  let nameMatches = $derived.by((): GlobalSearchResult[] => {
    const words = searchQuery.toLowerCase().split(/\s+/).filter(Boolean);
    return allSessions
      .filter((session) => toolFilter === "all" || session.tool === toolFilter)
      .filter((session) => {
        const name = sessionTitle(session).toLowerCase();
        return words.every((word) => name.includes(word));
      })
      .slice(0, RESULT_LIMIT)
      .map((session) => ({
        session_id: session.session_id,
        tool: session.tool,
        project_name: session.project_name,
        project_path: session.project_path,
        session_name: sessionTitle(session),
        matched_text: "",
        match_source: "session_name",
        timestamp: session.modified,
        jsonl_path: session.jsonl_path,
      }));
  });
  // A name that matched as you typed stays in the results, even if the search worded the
  // title differently and missed it.
  let visibleResults = $derived.by(() => {
    if (searching && searchResults.length === 0) return nameMatches;
    const found = new Set(searchResults.map((result) => result.jsonl_path));
    return [...searchResults, ...nameMatches.filter((result) => !found.has(result.jsonl_path))].slice(0, RESULT_LIMIT);
  });

  // Newest-first groups read as a timeline; oldest-first is a plain list with the same headings.
  // Calendar days rather than 24-hour steps, so clock changes don't move a session between groups.
  function dateGroup(isoDate: string | null): string {
    const time = timeMs(isoDate);
    if (time === 0) return "No date";
    const today = new Date(now);
    const daysAgo = (days: number) => new Date(today.getFullYear(), today.getMonth(), today.getDate() - days).getTime();
    if (time >= daysAgo(0)) return "Today";
    if (time >= daysAgo(1)) return "Yesterday";
    if (time >= daysAgo(7)) return "Previous 7 days";
    return new Date(time).toLocaleDateString([], { month: "long", year: "numeric" });
  }

  let groups = $derived.by(() => {
    const grouped: { label: string; sessions: SessionInfo[] }[] = [];
    for (const session of orderedSessions) {
      const label = dateGroup(session.modified);
      const last = grouped[grouped.length - 1];
      if (last?.label === label) last.sessions.push(session);
      else grouped.push({ label, sessions: [session] });
    }
    return grouped;
  });

  let flatSessions = $derived(groups.flatMap((group) => group.sessions));

  // One Tab stop for the whole list: arrow keys move inside it.
  let sessionTabStop = $derived(
    flatSessions.find((session) => session.jsonl_path === selectedPath)?.jsonl_path ?? flatSessions[0]?.jsonl_path
  );
  let resultTabStop = $derived(
    visibleResults.find((result) => result.jsonl_path === selectedPath)?.jsonl_path ?? visibleResults[0]?.jsonl_path
  );

  let knownSessionIds = $derived(new Set(allSessions.map((session) => session.session_id)));

  function projectLabel(path: string, name: string): string {
    return labels.get(path) ?? (name || path);
  }

  function isRunning(session: SessionInfo): boolean {
    // The clock ticks every 30 seconds, so a fresh write can look slightly ahead of it. Anything
    // further ahead is a bad timestamp, not a running session.
    const age = now - timeMs(session.last_written);
    return age < RUNNING_WINDOW_MS && age > -CLOCK_TOLERANCE_MS;
  }

  function forkParentTitle(session: SessionInfo): string {
    const parent = allSessions.find((entry) => entry.session_id === session.forked_from_session_id);
    return parent ? sessionTitle(parent, 40) : "another session";
  }

  // Text split around the searched words, so a result shows why it matched.
  function matchParts(text: string): { text: string; matched: boolean }[] {
    const words = searchQuery.trim().split(/\s+/).filter(Boolean).map((word) => word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
    if (words.length === 0) return [{ text, matched: false }];
    return text.split(new RegExp(`(${words.join("|")})`, "gi")).map((part, index) => ({ text: part, matched: index % 2 === 1 }));
  }

  // Values seen per row, so a value flips when it changes rather than whenever a row appears.
  const lastSeen = new Map<string, unknown>();

  function hasChanged(rowKey: string, value: unknown): boolean {
    const changed = lastSeen.has(rowKey) && lastSeen.get(rowKey) !== value;
    lastSeen.set(rowKey, value);
    return changed;
  }

  // Arrows belong to the list only while focus is in it, so they still scroll a conversation
  // and do nothing behind an open dialog.
  async function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    if (dialogIsOpen()) return;
    const target = event.target as HTMLElement;
    if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;
    const board = target.closest(".board");
    if (!board && !(target === document.body && !selectedPath && !isSearching)) return;
    const step = event.key === "ArrowDown" ? 1 : -1;
    if (isSearching || showingMessages) {
      event.preventDefault();
      const rows = [...(board?.querySelectorAll<HTMLElement>(".scroll button.row, .scroll button.bookmark-open") ?? [])];
      const nextRow = rows[rows.indexOf(target) + step];
      nextRow?.focus();
      nextRow?.scrollIntoView({ block: "nearest" });
      return;
    }
    if (flatSessions.length === 0) return;
    event.preventDefault();
    const currentIndex = flatSessions.findIndex((session) => session.jsonl_path === selectedPath);
    const nextIndex = currentIndex === -1 ? 0 : (currentIndex + step + flatSessions.length) % flatSessions.length;
    onSelect(flatSessions[nextIndex]);
    await tick();
    const selectedRow = document.querySelector<HTMLElement>(".board .row.selected");
    selectedRow?.scrollIntoView({ block: "nearest" });
    selectedRow?.focus({ preventScroll: true });
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#snippet highlighted(text: string)}
  {#each matchParts(text) as part}{#if part.matched}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
{/snippet}

<section class="board" aria-label="Sessions">
  {#each problems as problem, index (index)}
    <p class="problem" role="alert">{problem}</p>
  {/each}
  {#if isSearching}
    <header class="board-head">
      <h2>Results</h2>
      <span class="head-note" role="status">{searching ? "Searching messages…" : visibleResults.length >= RESULT_LIMIT ? `Newest ${RESULT_LIMIT} sessions` : `${visibleResults.length} session${visibleResults.length === 1 ? "" : "s"}`}</span>
    </header>
    {#if searching}<div class="progress" role="progressbar" aria-label="Searching"></div>{/if}
    <div class="scroll">
      {#if searching && visibleResults.length === 0}
        <p class="empty" role="status">Searching every session. Large histories can take a few seconds.</p>
      {/if}
      {#each visibleResults as result, index (index)}
        <button class="row result" class:selected={selectedPath === result.jsonl_path} aria-current={selectedPath === result.jsonl_path ? "true" : undefined} tabindex={result.jsonl_path === resultTabStop ? 0 : -1} onclick={(event) => { event.currentTarget.focus(); onOpenResult(result); }}>
          <span class="time">{formatRowTime(result.timestamp, now, preferences.dateFormat)}</span>
          <span class="tool" title={formatToolName(result.tool)}><ToolIcon tool={result.tool} size={15} /><span class="tool-name">{formatToolName(result.tool)}</span></span>
          <span class="body">
            <span class="title">{@render highlighted(result.session_name)}</span>
            <span class="meta"><span class="project">{projectLabel(result.project_path, result.project_name)}</span></span>
            {#if result.matched_text}<span class="snippet">{@render highlighted(result.matched_text)}</span>{/if}
          </span>
        </button>
      {:else}
        {#if !searching}
          <p class="empty"><strong>Nothing matches “{searchQuery.trim()}”.</strong> Search covers session names and what you and the agent wrote. Tool output isn't searched.</p>
        {/if}
      {/each}
    </div>
  {:else}
    <header class="board-head">
      <h2>{heading}</h2>
      {#if savedMode}
        <div class="saved-tabs" role="group" aria-label="Saved items">
          <button class:active={savedTab === "sessions"} aria-pressed={savedTab === "sessions"} onclick={() => (savedTab = "sessions")}>Sessions</button>
          <button class:active={savedTab === "messages"} aria-pressed={savedTab === "messages"} onclick={() => (savedTab = "messages")}>Messages {bookmarks.length}</button>
        </div>
      {:else}
        <button class="sort" onclick={() => (newestFirst = !newestFirst)} title="Change sort order">{newestFirst ? "Newest first" : "Oldest first"}</button>
      {/if}
    </header>

    {#if showingMessages}
      <div class="scroll">
        {#each bookmarks as bookmark (bookmark.id)}
          {@const isGone = !loading && !knownSessionIds.has(bookmark.session_id)}
          <div class="row bookmark">
            <span class="time">{formatRowTime(bookmark.timestamp, now, preferences.dateFormat)}</span>
            <button class="bookmark-open" disabled={isGone} onclick={() => onOpenBookmark(bookmark)}>
              <span class="title">{bookmark.preview}</span>
              <span class="meta"><span>{bookmark.role === "user" ? "You" : "Agent"}</span><span class="project">{projectLabel(bookmark.project_path, bookmark.project_name)}</span></span>
              {#if isGone}<span class="gone">This session is no longer on this Mac.</span>{/if}
            </button>
            {#if isGone}
              <button class="remove-text" onclick={() => onRemoveBookmark(bookmark.id)}>Remove</button>
            {:else}
              <button class="remove" onclick={() => onRemoveBookmark(bookmark.id)} title="Remove bookmark" aria-label="Remove bookmark">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
              </button>
            {/if}
          </div>
        {:else}
          <p class="empty"><strong>No saved messages yet.</strong> Click the bookmark icon on a message to keep it here.</p>
        {/each}
      </div>
    {:else}
      {#if loading}<div class="progress" role="progressbar" aria-label="Reading sessions"></div>{/if}
      <div class="scroll">
        {#if loading}
          <!-- Saved sessions appear first, so a partial list must say more is coming. -->
          <p class="reading" role="status">Reading your Claude Code and Codex history. The first load takes the longest.</p>
          {#if sessions.length === 0}
            {#each { length: 8 } as _, index (index)}
              <div class="skeleton" aria-hidden="true"><i></i><i></i><span><i></i><i></i></span></div>
            {/each}
          {/if}
        {/if}
        {#each groups as group, groupIndex (groupIndex)}
          <h3>{group.label}<span>{group.sessions.length}</span></h3>
          {#each group.sessions as session (session.jsonl_path)}
            {@const running = isRunning(session)}
            <button class="row" class:selected={selectedPath === session.jsonl_path} aria-current={selectedPath === session.jsonl_path ? "true" : undefined} tabindex={session.jsonl_path === sessionTabStop ? 0 : -1} onclick={(event) => { event.currentTarget.focus(); onSelect(session); }}>
              {#key running ? "now" : session.modified}
                <span class="time" class:flip={hasChanged(`${session.session_id}:time`, running ? "now" : session.modified)} class:running>
                  {#if running}<i class="lamp" aria-hidden="true"></i>Now{:else}{formatRowTime(session.modified, now, preferences.dateFormat, group.label === "Yesterday")}{/if}
                </span>
              {/key}
              <span class="tool" title={formatToolName(session.tool)}><ToolIcon tool={session.tool} size={15} /><span class="tool-name">{formatToolName(session.tool)}</span></span>
              <span class="body">
                <span class="title" title={sessionTitle(session)}>{sessionTitle(session, 80)}</span>
                <span class="meta">
                  {#if showProject}<span class="project">{projectLabel(session.project_path, session.project_name)}</span>{/if}
                  {#if savedSessionIds.has(session.session_id) && !savedMode}<span class="flag saved">Saved</span>{/if}
                  {#if session.archived_in_tool}<span class="flag" title="Archived inside {formatToolName(session.tool)}. Different from Saved, which keeps a copy in Evertusk.">Archived</span>{/if}
                  {#if session.forked_from_session_id}<span class="flag" title="Forked from {forkParentTitle(session)}">Forked</span>{/if}
                  {#key session.conversation_count}
                    <span class="num" class:flip={hasChanged(`${session.session_id}:prompts`, session.conversation_count)}>{session.conversation_count} prompt{session.conversation_count === 1 ? "" : "s"}</span>
                  {/key}
                </span>
              </span>
            </button>
          {/each}
        {:else}
          {#if !loading}
            <p class="empty">
              {#if toolFilter !== "all"}
                <strong>No {formatToolName(toolFilter)} sessions {savedMode ? "saved" : "here"}.</strong>
                <button class="show-all" onclick={onClearToolFilter}>Show all tools</button>
              {:else if savedMode}
                <strong>Nothing saved yet.</strong> Click Save on a Claude Code session to keep a copy after Claude Code's cleanup.
              {:else}
                <strong>No sessions here.</strong> Sessions appear once Claude Code or Codex has run on this Mac.
              {/if}
            </p>
          {/if}
        {/each}
      </div>
    {/if}
  {/if}
</section>

<style>
  .board {
    width: 384px;
    flex-shrink: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--bg-app);
    border-right: 1px solid var(--border);
  }

  .reading {
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-muted);
  }

  /* Same columns as a real row, so nothing jumps when the sessions arrive. */
  .skeleton {
    display: grid;
    grid-template-columns: 72px 22px minmax(0, 1fr);
    gap: 0 6px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
  }

  .skeleton i {
    display: block;
    height: 9px;
    width: 70%;
    border-radius: 2px;
    background: var(--bg-hover);
    animation: skeleton-pulse 1.4s ease-in-out infinite;
  }

  .skeleton span i:first-child { width: 85%; margin-bottom: 7px; }
  .skeleton span i:last-child { width: 55%; }

  @keyframes skeleton-pulse {
    50% { opacity: 0.45; }
  }

  .problem {
    padding: 8px 14px;
    background: var(--danger-soft);
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    line-height: 1.4;
    overflow-wrap: anywhere;
    color: var(--danger);
  }

  .board-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 46px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--border);
  }

  h2 {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .head-note {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .sort,
  .saved-tabs button {
    flex-shrink: 0;
    min-height: 24px;
    padding: 3px 8px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 5px;
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .sort:hover,
  .saved-tabs button:hover { color: var(--text-primary); }

  .saved-tabs {
    display: flex;
    gap: 4px;
  }

  .saved-tabs button.active {
    background: var(--bg-selected);
    color: var(--text-primary);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  h3 {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    gap: 6px;
    padding: 12px 14px 5px;
    background: var(--bg-app);
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
  }

  h3 span {
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: var(--text-faint);
  }

  /* Fixed columns: the eye runs straight down time and tool, like a departure board. */
  .row {
    display: grid;
    grid-template-columns: 72px 22px minmax(0, 1fr);
    gap: 0 6px;
    align-items: baseline;
    width: 100%;
    padding: 9px 14px;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    text-align: left;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .row:hover { background: var(--bg-hover); }
  /* .scroll clips anything outside the row, so the focus ring sits inside it. */
  .row:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .row.selected { background: var(--bg-selected); }
  .row.selected .time { color: var(--accent-text); }

  .show-all {
    padding: 0;
    background: none;
    border: none;
    font-size: 13px;
    color: var(--text-secondary);
    text-decoration: underline;
    cursor: pointer;
  }

  .time {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    color: var(--text-muted);
  }

  .time.running { color: var(--accent-text); }

  mark {
    background: var(--accent-soft);
    color: var(--text-primary);
    border-radius: 2px;
  }

  .lamp {
    width: 6px;
    height: 6px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--accent);
    animation: lamp 1.6s ease-in-out infinite;
  }

  @keyframes lamp {
    50% { opacity: 0.35; }
  }

  /* A value that changes turns over into place, like a flap on a departure board. */
  .flip {
    display: inline-flex;
    transform-origin: 50% 40%;
    animation: flap 320ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes flap {
    from { opacity: 0; transform: perspective(160px) rotateX(-68deg); }
  }

  .tool {
    display: flex;
    align-items: center;
    align-self: start;
    height: 18px;
  }

  /* The icon is what you scan; screen readers still get the name. */
  .tool-name {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .title {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
    font-size: 13px;
    font-weight: 500;
    line-height: 1.35;
    color: var(--text-primary);
  }

  /* Whatever doesn't fit wraps onto a hidden second line, so the least important detail
     drops out whole instead of being cut mid-word. Tokens are always in the session header. */
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0 8px;
    height: 1.35em;
    line-height: 1.35;
    overflow: hidden;
    white-space: nowrap;
    font-size: 12px;
    color: var(--text-muted);
  }

  .project {
    max-width: 130px;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-secondary);
  }

  .num {
    font-variant-numeric: tabular-nums;
  }

  .flag { color: var(--text-muted); font-weight: 500; }
  .flag.saved { color: var(--accent-text); }

  .snippet {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
    margin-top: 2px;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-muted);
  }

  .row.bookmark {
    grid-template-columns: 72px minmax(0, 1fr) auto;
    cursor: default;
  }

  .bookmark-open {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    padding: 0;
    background: none;
    border: none;
    text-align: left;
    cursor: pointer;
  }

  .bookmark-open:disabled { cursor: default; }
  .bookmark-open:disabled .title { color: var(--text-muted); }

  .gone { font-size: 12px; color: var(--danger); }

  .remove-text {
    align-self: start;
    min-height: 24px;
    padding: 2px 8px;
    background: transparent;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .remove-text:hover { color: var(--text-primary); }

  .remove {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--text-faint);
    cursor: pointer;
  }

  .remove:hover { color: var(--danger); }

  .empty {
    padding: 28px 18px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-muted);
  }

  .empty strong {
    display: block;
    margin-bottom: 3px;
    font-weight: 500;
    color: var(--text-primary);
    overflow-wrap: anywhere;
  }

  @media (prefers-reduced-motion: reduce) {
    .flip, .lamp, .skeleton i { animation: none; }
  }

  @media (max-width: 1000px) {
    .board { flex: 1; width: auto; min-width: 0; }
  }
</style>
