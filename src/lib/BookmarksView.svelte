<script lang="ts">
  import { bookmarks, removeBookmark } from "./bookmarks.svelte";
  import { copyToClipboard } from "./clipboard";
  import type { Bookmark } from "./types";

  let {
    onBack,
    onJump,
  }: {
    onBack: () => void;
    onJump: (bookmark: Bookmark) => void;
  } = $props();

  let filterQuery = $state("");

  let sorted = $derived([...bookmarks].sort((first, second) => second.created_at - first.created_at));

  let filtered = $derived.by(() => {
    const query = filterQuery.trim().toLowerCase();
    if (!query) return sorted;
    const words = query.split(/\s+/).filter(Boolean);
    return sorted.filter((bookmark) => {
      const haystack = `${bookmark.preview} ${bookmark.project_name} ${bookmark.text}`.toLowerCase();
      return words.every((word) => haystack.includes(word));
    });
  });

  let copiedId = $state<string | null>(null);
  async function copyBookmark(bookmark: Bookmark) {
    await copyToClipboard(bookmark.text);
    copiedId = bookmark.id;
    setTimeout(() => {
      if (copiedId === bookmark.id) copiedId = null;
    }, 1500);
  }

  function formatWhen(ms: number): string {
    const date = new Date(ms);
    const now = new Date();
    const diffDays = Math.floor((now.getTime() - date.getTime()) / 86400000);
    if (diffDays === 0) return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (diffDays === 1) return "Yesterday";
    if (diffDays < 7) return `${diffDays}d ago`;
    return date.toLocaleDateString([], { month: "short", day: "numeric" });
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onBack();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="bookmarks-page">
  <div class="bookmarks-header">
    <button class="back-btn" onclick={onBack} title="Back (Esc)" aria-label="Back">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M19 12H5M12 19l-7-7 7-7"/></svg>
    </button>
    <div class="header-title">
      <h1>Bookmarks</h1>
      <span class="subtitle">
        {bookmarks.length} saved {bookmarks.length === 1 ? "message" : "messages"}
      </span>
    </div>
  </div>

  {#if bookmarks.length > 0}
    <div class="filter-bar">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
      <input
        type="text"
        placeholder="Filter bookmarks..."
        autocomplete="off"
        spellcheck="false"
        bind:value={filterQuery}
      />
    </div>
  {/if}

  <div class="bookmarks-list">
    {#if bookmarks.length === 0}
      <div class="empty-state">
        <div class="empty-icon">
          <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
        </div>
        <p>No bookmarks yet.</p>
        <p class="hint">Click the bookmark icon on any message to save it here.</p>
      </div>
    {:else if filtered.length === 0}
      <div class="empty-state">
        <p>No bookmarks match "{filterQuery}".</p>
      </div>
    {:else}
      {#each filtered as bookmark (bookmark.id)}
        <div class="bookmark-card">
          <div class="bookmark-meta">
            <span class="role-badge role-{bookmark.role}">{bookmark.role === "user" ? "You" : "Claude"}</span>
            <span class="project-name">{bookmark.project_name}</span>
            <span class="when">{formatWhen(bookmark.created_at)}</span>
          </div>
          <button class="bookmark-preview" onclick={() => onJump(bookmark)} title="Jump to session">
            {bookmark.preview}{bookmark.text.length > bookmark.preview.length ? "…" : ""}
          </button>
          <div class="bookmark-actions">
            <button class="action-btn primary" onclick={() => onJump(bookmark)}>
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 18l6-6-6-6"/></svg>
              Jump to session
            </button>
            <button class="action-btn" onclick={() => copyBookmark(bookmark)}>
              {#if copiedId === bookmark.id}Copied!{:else}Copy{/if}
            </button>
            <button class="action-btn remove" onclick={() => removeBookmark(bookmark.id)} title="Remove bookmark">
              Remove
            </button>
          </div>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .bookmarks-page {
    height: 100vh;
    overflow-y: auto;
    background: #12121e;
    padding: 32px 48px 64px;
  }

  .bookmarks-header {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 24px;
  }

  .back-btn {
    background: #2a2a4a;
    border: none;
    color: #a0a0c0;
    width: 36px;
    height: 36px;
    border-radius: 8px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    transition: all 0.15s;
  }

  .back-btn:hover {
    background: #3a3a5a;
    color: #e0e0e0;
  }

  .header-title h1 {
    font-size: 24px;
    font-weight: 700;
    color: #e0e0f0;
    margin: 0;
  }

  .subtitle {
    font-size: 13px;
    color: #6a6a8a;
  }

  .filter-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 560px;
    background: #1a1a2e;
    border: 1px solid #2a2a4a;
    border-radius: 10px;
    padding: 0 12px;
    margin-bottom: 24px;
    color: #5a5a7a;
  }

  .filter-bar input {
    flex: 1;
    background: transparent;
    border: none;
    color: #e0e0e0;
    padding: 11px 0;
    font-size: 14px;
    outline: none;
  }

  .filter-bar input::placeholder {
    color: #5a5a7a;
  }

  .bookmarks-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 820px;
  }

  .bookmark-card {
    background: #1a1a2e;
    border: 1px solid #2a2a4a;
    border-radius: 10px;
    padding: 14px 16px;
    transition: border-color 0.15s;
  }

  .bookmark-card:hover {
    border-color: #3a3a5a;
  }

  .bookmark-meta {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 8px;
  }

  .role-badge {
    font-size: 10px;
    font-weight: 700;
    padding: 2px 8px;
    border-radius: 999px;
    letter-spacing: 0.04em;
  }

  .role-user {
    color: #c7d2fe;
    background: rgba(99, 102, 241, 0.15);
  }

  .role-assistant {
    color: #6ee7b7;
    background: rgba(16, 185, 129, 0.14);
  }

  .project-name {
    font-size: 12px;
    color: #8a8aaa;
    font-weight: 500;
  }

  .when {
    font-size: 11px;
    color: #5a5a7a;
    margin-left: auto;
  }

  .bookmark-preview {
    display: block;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    color: #d0d0e8;
    font-size: 13px;
    line-height: 1.55;
    cursor: pointer;
    padding: 0;
    margin-bottom: 12px;
    font-family: inherit;
  }

  .bookmark-preview:hover {
    color: #ececff;
  }

  .bookmark-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .action-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: #2a2a4a;
    border: none;
    color: #a0a0c0;
    font-size: 12px;
    font-weight: 500;
    padding: 6px 12px;
    border-radius: 6px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .action-btn:hover {
    background: #3a3a5a;
    color: #e0e0e0;
  }

  .action-btn.primary {
    background: rgba(99, 102, 241, 0.15);
    color: #a5b4fc;
  }

  .action-btn.primary:hover {
    background: rgba(99, 102, 241, 0.28);
    color: #c7d2fe;
  }

  .action-btn.remove {
    margin-left: auto;
    color: #8a8aaa;
  }

  .action-btn.remove:hover {
    background: rgba(239, 68, 68, 0.14);
    color: #f87171;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 80px 20px;
    color: #5a5a7a;
    text-align: center;
    gap: 6px;
  }

  .empty-icon {
    opacity: 0.4;
    margin-bottom: 8px;
  }

  .empty-state p {
    margin: 0;
    font-size: 14px;
  }

  .empty-state .hint {
    font-size: 12px;
    color: #4a4a6a;
  }

  .bookmarks-page::-webkit-scrollbar {
    width: 8px;
  }

  .bookmarks-page::-webkit-scrollbar-track {
    background: transparent;
  }

  .bookmarks-page::-webkit-scrollbar-thumb {
    background: #2a2a4a;
    border-radius: 4px;
  }
</style>
