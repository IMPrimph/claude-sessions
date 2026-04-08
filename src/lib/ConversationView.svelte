<script lang="ts">
  import type { ConversationMessage, SessionInfo } from "./types";
  import MessageBubble from "./MessageBubble.svelte";

  let {
    session,
    messages,
    loading,
  }: {
    session: SessionInfo | null;
    messages: ConversationMessage[];
    loading: boolean;
  } = $props();

  let scrollContainer: HTMLDivElement | undefined = $state();
  let messageSearchQuery = $state("");
  let matchedMessageIndices: number[] = $state([]);
  let currentMatchIndex = $state(0);
  let showScrollTop = $state(false);
  let showScrollBottom = $state(false);

  // Auto-scroll to top when session changes
  $effect(() => {
    if (session && scrollContainer) {
      scrollContainer.scrollTop = 0;
      messageSearchQuery = "";
    }
  });

  function handleScroll() {
    if (!scrollContainer) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollContainer;
    showScrollTop = scrollTop > 300;
    showScrollBottom = scrollTop < scrollHeight - clientHeight - 300;
  }

  function scrollToTop() {
    scrollContainer?.scrollTo({ top: 0, behavior: "smooth" });
  }

  function scrollToBottom() {
    if (!scrollContainer) return;
    scrollContainer.scrollTo({ top: scrollContainer.scrollHeight, behavior: "smooth" });
  }

  // Compute matched indices when search query changes
  $effect(() => {
    if (!messageSearchQuery) {
      matchedMessageIndices = [];
      currentMatchIndex = 0;
      return;
    }
    const query = messageSearchQuery.toLowerCase();
    matchedMessageIndices = messages
      .map((message, index) => (message.text.toLowerCase().includes(query) ? index : -1))
      .filter((index) => index !== -1);
    currentMatchIndex = 0;
  });

  function scrollToMatch(matchIndex: number) {
    if (matchedMessageIndices.length === 0 || !scrollContainer) return;
    currentMatchIndex = matchIndex;
    const targetMessageIndex = matchedMessageIndices[matchIndex];
    const messageElements = scrollContainer.querySelectorAll("[data-msg-index]");
    const targetElement = messageElements[targetMessageIndex];
    if (targetElement) {
      targetElement.scrollIntoView({ behavior: "smooth", block: "center" });
    }
  }

  function nextMatch() {
    if (matchedMessageIndices.length === 0) return;
    scrollToMatch((currentMatchIndex + 1) % matchedMessageIndices.length);
  }

  function prevMatch() {
    if (matchedMessageIndices.length === 0) return;
    scrollToMatch(
      (currentMatchIndex - 1 + matchedMessageIndices.length) % matchedMessageIndices.length
    );
  }

  let searchInput: HTMLInputElement | undefined = $state();

  function handleSearchKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.shiftKey ? prevMatch() : nextMatch();
    } else if (event.key === "Escape") {
      messageSearchQuery = "";
      searchInput?.blur();
    }
  }

  function handleGlobalKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "f" && session) {
      event.preventDefault();
      searchInput?.focus();
    }
  }

  function formatSessionDate(isoDate: string | null): string {
    if (!isoDate) return "";
    return new Date(isoDate).toLocaleString([], {
      month: "short",
      day: "numeric",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function cleanTitle(session: SessionInfo): string {
    if (session.summary) return session.summary.replace(/\*\*/g, "");
    if (session.first_prompt) {
      let prompt = session.first_prompt
        .replace(/<[^>]+>/g, "")
        .replace(/['"`]?\/[\w\-./]+['"`]?\s*/g, "")
        .replace(/['"`]?\.\/[\w\-./]+['"`]?\s*/g, "")
        .replace(/\b\w+\.(ts|js|svelte|rs|json|md|py|go|jsx|tsx|css|html)\b/g, "")
        .replace(/```[\s\S]*?```/g, "")
        .replace(/\[.*?\]\(.*?\)/g, "")
        .replace(/\n+/g, " ")
        .replace(/\s+/g, " ")
        .trim();
      if (!prompt || prompt.length < 10) {
        const paths = session.first_prompt.match(/\/[\w\-./]+/g);
        if (paths && paths.length > 0) {
          const segments = paths[paths.length - 1].split("/").filter(Boolean);
          prompt = segments.slice(-2).join("/");
        } else {
          prompt = session.first_prompt.replace(/\s+/g, " ").trim();
        }
      }
      return prompt.length > 120 ? prompt.slice(0, 120) + "..." : prompt;
    }
    return session.session_id.slice(0, 8);
  }

  function sessionDuration(
    created: string | null,
    modified: string | null
  ): string {
    if (!created || !modified) return "";
    const startTime = new Date(created).getTime();
    const endTime = new Date(modified).getTime();
    const diffMinutes = Math.round((endTime - startTime) / (1000 * 60));
    if (diffMinutes < 1) return "< 1 min";
    if (diffMinutes < 60) return `${diffMinutes} min`;
    const hours = Math.floor(diffMinutes / 60);
    const remainingMinutes = diffMinutes % 60;
    return `${hours}h ${remainingMinutes}m`;
  }
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div class="conversation-view">
  {#if !session}
    <div class="empty-state">
      <div class="empty-icon">
        <svg
          width="48"
          height="48"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
        >
          <path
            d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"
          />
        </svg>
      </div>
      <p>Select a session to view the conversation</p>
    </div>
  {:else}
    <div class="session-header">
      <div class="session-title">
        {cleanTitle(session)}
      </div>
      <div class="session-info">
        <span>{session.project_name}</span>
        {#if session.git_branch}
          <span class="separator">|</span>
          <span>{session.git_branch}</span>
        {/if}
        {#if session.created}
          <span class="separator">|</span>
          <span>{formatSessionDate(session.created)}</span>
          {#if session.modified && session.modified !== session.created}
            <span class="time-arrow">→</span>
            <span>{formatSessionDate(session.modified)}</span>
            <span class="duration-badge">({sessionDuration(session.created, session.modified)})</span>
          {/if}
        {/if}
        {#if session.message_count}
          <span class="separator">|</span>
          <span>{session.message_count} messages</span>
        {/if}
      </div>
    </div>

    {#if messages.length > 0}
      <div class="message-search-bar">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
        <input
          type="text"
          placeholder="Search messages... (Cmd+F, Enter to navigate)"
          autocomplete="off"
          spellcheck="false"
          bind:value={messageSearchQuery}
          bind:this={searchInput}
          onkeydown={handleSearchKeydown}
        />
        {#if messageSearchQuery}
          <span class="match-count">
            {#if matchedMessageIndices.length > 0}
              {currentMatchIndex + 1}/{matchedMessageIndices.length}
            {:else}
              0 results
            {/if}
          </span>
          <button class="nav-btn" onclick={prevMatch} title="Previous match">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="m18 15-6-6-6 6"/></svg>
          </button>
          <button class="nav-btn" onclick={nextMatch} title="Next match">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="m6 9 6 6 6-6"/></svg>
          </button>
          <button class="nav-btn dismiss-btn" onclick={() => { messageSearchQuery = ""; }} title="Clear search (Esc)">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
          </button>
        {/if}
      </div>
    {/if}

    <div class="messages-wrapper">
      <div class="messages-container" bind:this={scrollContainer} onscroll={handleScroll}>
        {#if loading}
          <div class="loading-state">Loading messages...</div>
        {:else if messages.length === 0}
          <div class="empty-messages">No messages found in this session</div>
        {:else}
          {#each messages as message, index (index)}
            <div
              data-msg-index={index}
              class:search-highlight={messageSearchQuery && matchedMessageIndices.includes(index)}
              class:search-active={messageSearchQuery && matchedMessageIndices[currentMatchIndex] === index}
            >
              <MessageBubble {message} searchQuery={messageSearchQuery} />
            </div>
          {/each}
        {/if}
      </div>

      {#if showScrollTop}
        <button class="scroll-fab scroll-fab-top" onclick={scrollToTop} title="Scroll to top">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="m18 15-6-6-6 6"/></svg>
        </button>
      {/if}
      {#if showScrollBottom}
        <button class="scroll-fab scroll-fab-bottom" onclick={scrollToBottom} title="Scroll to bottom">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="m6 9 6 6 6-6"/></svg>
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .conversation-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #12121e;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: #5a5a7a;
    gap: 16px;
  }

  .empty-icon {
    opacity: 0.4;
  }

  .empty-state p {
    font-size: 14px;
    margin: 0;
  }

  .session-header {
    padding: 16px 24px;
    border-bottom: 1px solid #2a2a4a;
    background: #16162a;
  }

  .session-title {
    font-size: 16px;
    font-weight: 600;
    color: #e0e0f0;
    margin-bottom: 8px;
    line-height: 1.4;
  }

  .session-info {
    font-size: 12px;
    color: #7a7a9a;
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
  }

  .separator {
    color: #3a3a5a;
    margin: 0 2px;
  }

  .time-arrow {
    color: #5a5a7a;
    margin: 0 2px;
  }

  .duration-badge {
    color: #6366f1;
    font-weight: 500;
  }

  .message-search-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 24px;
    border-bottom: 1px solid #2a2a4a;
    background: #16162a;
    color: #5a5a7a;
  }

  .message-search-bar input {
    flex: 1;
    background: #12121e;
    border: 1px solid #2a2a4a;
    color: #e0e0e0;
    padding: 6px 10px;
    border-radius: 5px;
    font-size: 13px;
    outline: none;
  }

  .message-search-bar input:focus {
    border-color: #6366f1;
  }

  .message-search-bar input::placeholder {
    color: #5a5a7a;
  }

  .match-count {
    font-size: 11px;
    color: #7a7a9a;
    white-space: nowrap;
  }

  .nav-btn {
    background: #2a2a4a;
    border: none;
    color: #a0a0c0;
    width: 24px;
    height: 24px;
    border-radius: 4px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .nav-btn:hover {
    background: #3a3a5a;
    color: #e0e0e0;
  }

  .search-highlight {
    border-left: 2px solid #6366f1;
    border-radius: 4px;
  }

  .search-active {
    border-left: 3px solid #f59e0b;
    background: rgba(245, 158, 11, 0.05);
    border-radius: 4px;
  }

  .messages-wrapper {
    flex: 1;
    position: relative;
    overflow: hidden;
  }

  .messages-container {
    height: 100%;
    overflow-y: auto;
    padding: 16px 24px;
  }

  .scroll-fab {
    position: absolute;
    right: 20px;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: #1e1e36;
    border: 1px solid #2a2a4a;
    color: #a0a0c0;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
    transition: all 0.15s;
    z-index: 10;
  }

  .scroll-fab:hover {
    background: #2a2a4a;
    color: #e0e0f0;
    border-color: #6366f1;
  }

  .scroll-fab-top {
    top: 12px;
  }

  .scroll-fab-bottom {
    bottom: 12px;
  }

  .loading-state,
  .empty-messages {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 200px;
    color: #5a5a7a;
    font-size: 14px;
  }

  .messages-container::-webkit-scrollbar {
    width: 8px;
  }

  .messages-container::-webkit-scrollbar-track {
    background: transparent;
  }

  .messages-container::-webkit-scrollbar-thumb {
    background: #2a2a4a;
    border-radius: 4px;
  }
</style>
