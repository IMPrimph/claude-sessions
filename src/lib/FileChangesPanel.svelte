<script lang="ts">
  import { untrack } from "svelte";
  import type { FileChange, FileEditEntry } from "./types";

  let {
    changes,
    onClose,
  }: {
    changes: FileChange[];
    onClose: () => void;
  } = $props();

  let editedOnly = $state(true);

  let editedFiles = $derived(changes.filter((change) => change.edit_count > 0));
  let totalEdits = $derived(editedFiles.reduce((sum, change) => sum + change.edit_count, 0));

  let visible = $derived(editedOnly ? editedFiles : changes);

  // Accordion: only one file expanded at a time. Matches the "see one, close, move
  // to the next" workflow users expect when scanning a session's file changes.
  let expandedPath: string | null = $state(null);
  // Per-edit toggle state. Keys are `${path}::${index}`. Edit state persists across
  // file toggles so re-opening a file restores what you had expanded.
  let expandedEdits = $state(new Set<string>());

  // Non-reactive sentinel: tracks which first-file we've already auto-initialized for.
  // Lets us distinguish "new session loaded" from "user clicked something" so we don't
  // snap the user's selection back to the first file whenever any state mutates.
  let lastInitPath: string | null = null;

  $effect(() => {
    // Track `visible` only — everything else goes through `untrack`.
    const currentVisible = visible;
    untrack(() => {
      if (currentVisible.length === 0) {
        expandedPath = null;
        lastInitPath = null;
        return;
      }
      const first = currentVisible[0];
      if (lastInitPath === first.path) return;
      lastInitPath = first.path;
      expandedPath = first.path;
      if (first.edits.length > 0) {
        const key = editKey(first.path, 0);
        if (!expandedEdits.has(key)) {
          expandedEdits = new Set([...expandedEdits, key]);
        }
      }
    });
  });

  function toggleFile(path: string, change: FileChange) {
    if (expandedPath === path) {
      expandedPath = null;
      return;
    }
    expandedPath = path;
    // First time opening this file? Auto-expand the first edit so the user sees content.
    if (change.edits.length > 0) {
      const firstKey = editKey(path, 0);
      let hasAnyExpanded = false;
      for (let editIndex = 0; editIndex < change.edits.length; editIndex++) {
        if (expandedEdits.has(editKey(path, editIndex))) {
          hasAnyExpanded = true;
          break;
        }
      }
      if (!hasAnyExpanded) {
        expandedEdits = new Set([...expandedEdits, firstKey]);
      }
    }
  }

  function editKey(path: string, index: number): string {
    return `${path}::${index}`;
  }

  function toggleEdit(path: string, index: number) {
    const key = editKey(path, index);
    const next = new Set(expandedEdits);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    expandedEdits = next;
  }

  function actionLabel(action: FileEditEntry["action"]): string {
    switch (action) {
      case "write":
        return "Write";
      case "multiedit":
        return "MultiEdit";
      case "notebookedit":
        return "Notebook";
      default:
        return "Edit";
    }
  }

  function formatTime(iso: string | null): string {
    if (!iso) return "";
    return new Date(iso).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  // Long content (old/new strings) gets a height cap with a "Show full" toggle
  const MAX_LINES = 24;
  function maybeTruncate(text: string | null, expandedFull: boolean): {
    text: string;
    truncated: boolean;
    totalLines: number;
  } {
    if (!text) return { text: "", truncated: false, totalLines: 0 };
    const lines = text.split("\n");
    const totalLines = lines.length;
    if (expandedFull || totalLines <= MAX_LINES) {
      return { text, truncated: false, totalLines };
    }
    return {
      text: lines.slice(0, MAX_LINES).join("\n") + "\n…",
      truncated: true,
      totalLines,
    };
  }

  let expandedFullSet = $state(new Set<string>());

  function fullKey(path: string, index: number): string {
    return `${path}::${index}::full`;
  }

  function toggleFull(key: string) {
    const next = new Set(expandedFullSet);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    expandedFullSet = next;
  }

  // Short label for the sticky header (just the filename without path prefix)
  function fileBaseName(displayPath: string): string {
    const slash = displayPath.lastIndexOf("/");
    return slash === -1 ? displayPath : displayPath.slice(slash + 1);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<aside class="file-changes-panel">
  <header class="changes-header">
    <div class="changes-title-row">
      <span class="changes-tag">FILE CHANGES</span>
      <button class="close-btn" onclick={onClose} title="Close (Esc)" aria-label="Close panel">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    </div>
    <div class="changes-summary">
      {#if changes.length === 0}
        No file operations in this session
      {:else}
        {editedFiles.length} {editedFiles.length === 1 ? "file" : "files"} edited
        {#if totalEdits > 0} &middot; {totalEdits} {totalEdits === 1 ? "edit" : "edits"} total{/if}
        {#if !editedOnly && changes.length !== editedFiles.length}
          &middot; {changes.length - editedFiles.length} read-only
        {/if}
      {/if}
    </div>
    <div class="filter-row">
      <button
        class="filter-chip"
        class:filter-active={editedOnly}
        onclick={() => (editedOnly = !editedOnly)}
      >
        Edited only
      </button>
    </div>
  </header>

  <div class="changes-scroll">
    {#if visible.length === 0}
      <div class="empty-state">
        {#if changes.length === 0}
          Nothing edited or read in this session.
        {:else}
          No edited files. Toggle the filter to see read-only files.
        {/if}
      </div>
    {:else}
      {#each visible as change (change.path)}
        {@const isOpenFile = expandedPath === change.path}
        <div class="file-block" class:file-block-open={isOpenFile}>
          <button
            class="file-row"
            class:file-row-open={isOpenFile}
            onclick={() => toggleFile(change.path, change)}
            title={change.path}
          >
            <svg
              class="chevron"
              class:rotated={isOpenFile}
              width="11"
              height="11"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2.5"
            ><path d="m9 18 6-6-6-6"/></svg>
            <span class="file-path">{change.display_path}</span>
            <span class="file-badges">
              {#if change.edit_count > 0}
                <span class="badge badge-edit">Edit&times;{change.edit_count}</span>
              {/if}
              {#if change.read_count > 0}
                <span class="badge badge-read">Read&times;{change.read_count}</span>
              {/if}
            </span>
          </button>

          {#if isOpenFile}
            {#if change.edits.length === 0 && change.read_count > 0}
              <div class="read-only-note">Read-only — no content changes.</div>
            {:else}
              <div class="edits-list">
                {#each change.edits as edit, index (`${change.path}::${index}`)}
                  {@const editEx = expandedEdits.has(editKey(change.path, index))}
                  {@const fkey = fullKey(change.path, index)}
                  {@const showFull = expandedFullSet.has(fkey)}
                  <div class="edit-block" class:edit-block-open={editEx}>
                    <button class="edit-header" onclick={() => toggleEdit(change.path, index)}>
                      <svg
                        class="chevron"
                        class:rotated={editEx}
                        width="10"
                        height="10"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2.5"
                      ><path d="m9 18 6-6-6-6"/></svg>
                      <span class="edit-action edit-action-{edit.action}">{actionLabel(edit.action)}</span>
                      {#if edit.replace_all}
                        <span class="edit-replace-all">replace all</span>
                      {/if}
                      <span class="edit-index">#{index + 1}</span>
                      {#if edit.timestamp}
                        <span class="edit-time">{formatTime(edit.timestamp)}</span>
                      {/if}
                    </button>
                    {#if editEx}
                      <div class="edit-body">
                        {#if edit.old_string}
                          {@const oldText = maybeTruncate(edit.old_string, showFull)}
                          <div class="diff-block diff-old">
                            <pre>{oldText.text}</pre>
                            {#if oldText.truncated && !showFull}
                              <button class="show-full-btn" onclick={() => toggleFull(fkey)}>
                                Show full ({oldText.totalLines} lines)
                              </button>
                            {/if}
                          </div>
                        {/if}
                        {#if edit.new_string}
                          {@const newText = maybeTruncate(edit.new_string, showFull)}
                          <div class="diff-block diff-new">
                            <pre>{newText.text}</pre>
                            {#if newText.truncated && !showFull}
                              <button class="show-full-btn" onclick={() => toggleFull(fkey)}>
                                Show full ({newText.totalLines} lines)
                              </button>
                            {/if}
                          </div>
                        {/if}
                        {#if showFull && (edit.old_string || edit.new_string)}
                          <button class="show-full-btn collapse" onclick={() => toggleFull(fkey)}>
                            Collapse
                          </button>
                        {/if}
                      </div>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</aside>

<style>
  .file-changes-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 580px;
    min-width: 420px;
    background: #0e0e1a;
    border-left: 1px solid #2a2a4a;
    flex-shrink: 0;
  }

  .changes-header {
    padding: 14px 18px 12px;
    border-bottom: 1px solid #2a2a4a;
    background: #16162a;
    flex-shrink: 0;
  }

  .changes-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 6px;
  }

  .changes-tag {
    font-size: 10px;
    font-weight: 700;
    color: #818cf8;
    background: rgba(99, 102, 241, 0.12);
    border: 1px solid rgba(99, 102, 241, 0.28);
    padding: 2px 8px;
    border-radius: 999px;
    letter-spacing: 0.06em;
  }

  .close-btn {
    background: #2a2a4a;
    border: none;
    color: #a0a0c0;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.12s;
  }

  .close-btn:hover {
    background: #3a3a5a;
    color: #e0e0f0;
  }

  .changes-summary {
    font-size: 12px;
    color: #8a8aaa;
  }

  .filter-row {
    margin-top: 10px;
  }

  .filter-chip {
    background: transparent;
    border: 1px solid #2a2a4a;
    color: #8a8aaa;
    font-size: 11px;
    font-weight: 500;
    padding: 3px 9px;
    border-radius: 5px;
    cursor: pointer;
    transition: all 0.12s;
  }

  .filter-chip:hover {
    color: #c0c0d8;
    border-color: #3a3a5a;
  }

  .filter-chip.filter-active {
    background: rgba(99, 102, 241, 0.18);
    color: #c7d2fe;
    border-color: rgba(99, 102, 241, 0.4);
  }

  .changes-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
  }

  .empty-state {
    padding: 40px 12px;
    text-align: center;
    color: #5a5a7a;
    font-size: 13px;
  }

  .file-block {
    margin-bottom: 6px;
    border: 1px solid #1e1e36;
    border-radius: 8px;
    background: #12121e;
    overflow: hidden;
  }

  .file-block-open {
    border-color: rgba(99, 102, 241, 0.3);
  }

  /* Sticky file header: when you scroll inside an expanded file's edits, the
     filename stays pinned at the top of the scroll area so you always know
     which file you're looking at. */
  .file-row {
    position: sticky;
    top: -1px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 9px 12px;
    background: #16162a;
    border: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s;
  }

  .file-row:hover {
    background: #1a1a30;
  }

  .file-row-open {
    background: #1a1a36;
    box-shadow: 0 1px 0 rgba(99, 102, 241, 0.2);
  }

  .chevron {
    color: #6a6a8a;
    flex-shrink: 0;
    transition: transform 0.15s, color 0.15s;
  }

  .chevron.rotated {
    transform: rotate(90deg);
    color: #a5b4fc;
  }

  .file-path {
    flex: 1;
    min-width: 0;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    color: #d8d8f0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file-badges {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .badge {
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 10px;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 10px;
  }

  .badge-edit {
    background: rgba(245, 158, 11, 0.14);
    color: #fbbf24;
  }

  .badge-read {
    background: rgba(99, 102, 241, 0.1);
    color: #818cf8;
    opacity: 0.7;
  }

  .read-only-note {
    padding: 10px 14px;
    font-size: 12px;
    color: #6a6a8a;
    font-style: italic;
  }

  .edits-list {
    padding: 4px 0;
  }

  .edit-block {
    border-top: 1px solid #1e1e36;
  }

  .edit-header {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 14px;
    background: transparent;
    border: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s;
  }

  .edit-header:hover {
    background: rgba(99, 102, 241, 0.05);
  }

  .edit-block-open .edit-header {
    background: rgba(99, 102, 241, 0.04);
  }

  .edit-action {
    font-family: "SF Mono", "Fira Code", monospace;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 4px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 10px;
  }

  .edit-action-write {
    background: rgba(34, 197, 94, 0.14);
    color: #4ade80;
  }

  .edit-action-edit,
  .edit-action-multiedit {
    background: rgba(245, 158, 11, 0.14);
    color: #fbbf24;
  }

  .edit-action-notebookedit {
    background: rgba(168, 85, 247, 0.14);
    color: #c084fc;
  }

  .edit-replace-all {
    font-size: 10px;
    color: #c084fc;
    background: rgba(168, 85, 247, 0.1);
    padding: 1px 6px;
    border-radius: 4px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .edit-index {
    font-size: 11px;
    color: #6a6a8a;
    font-family: "SF Mono", "Fira Code", monospace;
  }

  .edit-time {
    font-size: 11px;
    color: #5a5a7a;
    margin-left: auto;
  }

  .edit-body {
    padding: 0 14px 12px;
  }

  .diff-block {
    margin-top: 6px;
    border-radius: 6px;
    overflow: hidden;
    position: relative;
  }

  .diff-block pre {
    margin: 0;
    padding: 8px 12px;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 360px;
    overflow-y: auto;
  }

  .diff-old {
    background: rgba(239, 68, 68, 0.08);
    border-left: 3px solid rgba(239, 68, 68, 0.5);
  }

  .diff-old pre {
    color: #fca5a5;
  }

  .diff-new {
    background: rgba(34, 197, 94, 0.08);
    border-left: 3px solid rgba(34, 197, 94, 0.5);
  }

  .diff-new pre {
    color: #86efac;
  }

  .show-full-btn {
    margin-top: 4px;
    margin-left: 12px;
    background: transparent;
    border: 1px solid #2a2a4a;
    color: #8a8aaa;
    font-size: 11px;
    font-weight: 500;
    padding: 3px 10px;
    border-radius: 5px;
    cursor: pointer;
    transition: all 0.12s;
  }

  .show-full-btn:hover {
    color: #c7d2fe;
    border-color: rgba(99, 102, 241, 0.4);
    background: rgba(99, 102, 241, 0.08);
  }

  .show-full-btn.collapse {
    margin-top: 8px;
    margin-left: 0;
  }

  .changes-scroll::-webkit-scrollbar {
    width: 8px;
  }

  .changes-scroll::-webkit-scrollbar-thumb {
    background: #2a2a4a;
    border-radius: 4px;
  }

  .diff-block pre::-webkit-scrollbar {
    width: 6px;
  }

  .diff-block pre::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.1);
    border-radius: 3px;
  }
</style>
