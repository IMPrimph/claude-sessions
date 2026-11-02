<script lang="ts">
  import { untrack } from "svelte";
  import type { FileChange, FileEditEntry } from "./types";

  let {
    changes,
    toolName,
  }: {
    changes: FileChange[];
    toolName: string;
  } = $props();

  let editedOnly = $state(true);

  function failedCount(change: FileChange): number {
    return change.edits.filter((edit) => edit.failed).length;
  }

  let editedFiles = $derived(changes.filter((change) => change.edit_count > 0));
  // edit_count also counts edits that were rejected, which changed nothing.
  let failedEdits = $derived(editedFiles.reduce((sum, change) => sum + failedCount(change), 0));
  let totalEdits = $derived(editedFiles.reduce((sum, change) => sum + change.edit_count, 0) - failedEdits);
  let changedFileCount = $derived(editedFiles.filter((change) => failedCount(change) < change.edits.length).length);
  let readOnlyCount = $derived(changes.length - editedFiles.length);

  // With nothing edited, the files that were read are the whole story.
  let visible = $derived(editedOnly && editedFiles.length > 0 ? editedFiles : changes);

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
      // Everything starts closed, so a session's whole list of files can be scanned at once.
      expandedPath = null;
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

  function allEditsOpen(change: FileChange): boolean {
    return [...change.edits.keys()].every((index) => expandedEdits.has(editKey(change.path, index)));
  }

  function toggleAllEdits(change: FileChange) {
    const open = !allEditsOpen(change);
    const next = new Set(expandedEdits);
    for (const index of change.edits.keys()) {
      if (open) next.add(editKey(change.path, index));
      else next.delete(editKey(change.path, index));
    }
    expandedEdits = next;
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
      case "delete":
        return "Delete";
      case "multiedit":
        return "MultiEdit";
      case "notebookedit":
        return "Notebook";
      case "bash":
        return "Bash";
      default:
        return "Edit";
    }
  }

  function formatTime(iso: string | null): string {
    if (!iso) return "";
    const date = new Date(iso);
    if (Number.isNaN(date.getTime())) return "";
    return date.toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  // ── Line diff ──

  type DiffLine = { kind: "same" | "add" | "del"; text: string };

  // ponytail: past this the table costs too much, so the block shows as all removed then all
  // added. Move to Myers if a real edit that size needs a precise diff.
  const MAX_DIFF_CELLS = 1_000_000;

  // Longest common subsequence of the lines left after trimming what both ends share.
  function diffLines(oldText: string | null, newText: string | null): DiffLine[] {
    const oldLines = oldText ? oldText.split("\n") : [];
    const newLines = newText ? newText.split("\n") : [];
    let start = 0;
    while (start < oldLines.length && start < newLines.length && oldLines[start] === newLines[start]) start++;
    let oldEnd = oldLines.length;
    let newEnd = newLines.length;
    while (oldEnd > start && newEnd > start && oldLines[oldEnd - 1] === newLines[newEnd - 1]) {
      oldEnd--;
      newEnd--;
    }
    const removed = oldLines.slice(start, oldEnd);
    const added = newLines.slice(start, newEnd);
    const same = (text: string): DiffLine => ({ kind: "same", text });
    const del = (text: string): DiffLine => ({ kind: "del", text });
    const add = (text: string): DiffLine => ({ kind: "add", text });
    const middle: DiffLine[] = [];
    if (removed.length * added.length > MAX_DIFF_CELLS) {
      middle.push(...removed.map(del), ...added.map(add));
    } else {
      const width = added.length + 1;
      const common = new Uint32Array((removed.length + 1) * width);
      for (let row = removed.length - 1; row >= 0; row--) {
        for (let column = added.length - 1; column >= 0; column--) {
          common[row * width + column] =
            removed[row] === added[column]
              ? common[(row + 1) * width + column + 1] + 1
              : Math.max(common[(row + 1) * width + column], common[row * width + column + 1]);
        }
      }
      let row = 0;
      let column = 0;
      while (row < removed.length || column < added.length) {
        if (row < removed.length && column < added.length && removed[row] === added[column]) {
          middle.push(same(removed[row]));
          row++;
          column++;
        } else if (row < removed.length && (column === added.length || common[(row + 1) * width + column] >= common[row * width + column + 1])) {
          middle.push(del(removed[row]));
          row++;
        } else {
          middle.push(add(added[column]));
          column++;
        }
      }
    }
    return [...oldLines.slice(0, start).map(same), ...middle, ...oldLines.slice(oldEnd).map(same)];
  }

  // Every row shows counts, so a session's diffs get worked out on first paint. Once each.
  const editDiffs = new WeakMap<FileEditEntry, DiffLine[]>();
  function editDiff(edit: FileEditEntry): DiffLine[] {
    let diff = editDiffs.get(edit);
    if (!diff) {
      diff = diffLines(edit.old_string, edit.new_string);
      editDiffs.set(edit, diff);
    }
    return diff;
  }

  function diffCounts(edits: FileEditEntry[]): { added: number; removed: number } {
    const counts = { added: 0, removed: 0 };
    for (const edit of edits) {
      if (edit.failed) continue;
      for (const line of editDiff(edit)) {
        if (line.kind === "add") counts.added++;
        else if (line.kind === "del") counts.removed++;
      }
    }
    return counts;
  }

  // Long diffs get a height cap with a "Show full" toggle.
  const MAX_LINES = 24;
  const GUTTER: Record<DiffLine["kind"], string> = { same: "", add: "+", del: "\u2212" };

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
</script>

<aside class="file-changes-panel">
  {#if changes.length > 0}
    <header class="changes-header">
      <div class="changes-summary">
        {#if changedFileCount === 0}
          No files edited
        {:else}
          {changedFileCount} {changedFileCount === 1 ? "file" : "files"} edited &middot; {totalEdits} {totalEdits === 1 ? "edit" : "edits"}
        {/if}
        {#if failedEdits > 0}
          &middot; {failedEdits} not applied
        {/if}
        {#if readOnlyCount > 0}
          &middot; {readOnlyCount} {readOnlyCount === 1 ? "file" : "files"} only read
        {/if}
      </div>
      {#if editedFiles.length > 0 && readOnlyCount > 0}
        <div class="filter-row">
          <button
            class="filter-chip"
            class:filter-active={editedOnly}
            aria-pressed={editedOnly}
            onclick={() => (editedOnly = !editedOnly)}
          >
            Edited only
          </button>
        </div>
      {/if}
    </header>
  {/if}

  <div class="changes-scroll">
    {#if visible.length === 0}
      <div class="empty-state">
        {toolName} didn't edit or read any files in this session.
      </div>
    {:else}
      {#each visible as change (change.path)}
        {@const isOpenFile = expandedPath === change.path}
        {@const fileCounts = diffCounts(change.edits)}
        <div class="file-block" class:file-block-open={isOpenFile}>
          <button
            class="file-row"
            class:file-row-open={isOpenFile}
            aria-expanded={isOpenFile}
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
            <span class="file-path" title={change.path}>{change.display_path.split("/").pop()}<span class="file-folder">{change.display_path.split("/").slice(0, -1).join("/")}</span></span>
            <span class="file-badges">
              {#if failedCount(change) > 0}
                <span class="edit-failed">{failedCount(change) === change.edits.length ? "Not applied" : `${failedCount(change)} not applied`}</span>
              {/if}
              {#if fileCounts.added > 0 || fileCounts.removed > 0}
                <span class="diff-stat"><span class="stat-added">+{fileCounts.added}</span> <span class="stat-removed">{GUTTER.del}{fileCounts.removed}</span></span>
              {/if}
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
              <div class="read-only-note">This file was only read, not changed.</div>
            {:else}
              <div class="edits-list">
                {#if change.edits.length > 1}
                  <button class="show-full-btn toggle-all-edits" onclick={() => toggleAllEdits(change)}>
                    {allEditsOpen(change) ? "Collapse all edits" : `Expand all ${change.edits.length} edits`}
                  </button>
                {/if}
                {#each change.edits as edit, index (`${change.path}::${index}`)}
                  {@const editEx = expandedEdits.has(editKey(change.path, index))}
                  {@const fkey = fullKey(change.path, index)}
                  {@const showFull = expandedFullSet.has(fkey)}
                  {@const editCounts = diffCounts([edit])}
                  <div class="edit-block" class:edit-block-open={editEx}>
                    <button class="edit-header" aria-expanded={editEx} onclick={() => toggleEdit(change.path, index)}>
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
                      <span class="edit-action">{actionLabel(edit.action)}</span>
                      {#if edit.failed}
                        <span class="edit-failed">Not applied</span>
                      {:else if editCounts.added > 0 || editCounts.removed > 0}
                        <span class="diff-stat"><span class="stat-added">+{editCounts.added}</span> <span class="stat-removed">{GUTTER.del}{editCounts.removed}</span></span>
                      {/if}
                      {#if edit.replace_all}
                        <span class="edit-replace-all">replace all</span>
                      {/if}
                      <span class="edit-index">#{index + 1}</span>
                      {#if edit.timestamp}
                        <span class="edit-time">{formatTime(edit.timestamp)}</span>
                      {/if}
                    </button>
                    {#if editEx}
                      {@const diff = editDiff(edit)}
                      <div class="edit-body">
                        {#if diff.length === 0}
                          <div class="read-only-note">No content was recorded for this edit.</div>
                        {:else}
                          <div class="diff-block" class:diff-failed={edit.failed}>
                            {#each showFull ? diff : diff.slice(0, MAX_LINES) as line}
                              <div class="diff-line diff-{line.kind}"><span class="diff-gutter">{GUTTER[line.kind]}</span><span class="diff-text">{line.text}</span></div>
                            {/each}
                          </div>
                          {#if diff.length > MAX_LINES}
                            <button class="show-full-btn" onclick={() => toggleFull(fkey)}>
                              {showFull ? "Show less" : `Show full (${diff.length} lines)`}
                            </button>
                          {/if}
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
    flex: 1;
    min-height: 0;
    width: 100%;
    background: var(--bg-app);
  }

  .changes-header {
    padding: 12px 24px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-panel);
    flex-shrink: 0;
  }

  .changes-summary {
    font-size: 12px;
    color: var(--text-muted);
  }

  .filter-row {
    margin-top: 10px;
  }

  .filter-chip {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 500;
    padding: 3px 9px;
    border-radius: 5px;
    min-height: 24px;
    cursor: pointer;
    transition: all 0.12s;
  }

  .filter-chip:hover {
    color: var(--text-secondary);
    border-color: var(--border-strong);
  }

  .filter-chip.filter-active {
    background: var(--bg-selected);
    color: var(--text-primary);
    border-color: var(--border-strong);
  }

  .changes-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
  }

  .empty-state {
    padding: 40px 12px;
    text-align: center;
    color: var(--text-faint);
    font-size: 13px;
  }

  .file-block {
    margin-bottom: 6px;
    border: 1px solid var(--bg-elevated);
    border-radius: 8px;
    background: var(--bg-app);
    overflow: hidden;
  }

  .file-block-open {
    border-color: var(--border-strong);
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
    background: var(--bg-panel);
    border: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s;
  }

  .file-row:hover {
    background: var(--bg-elevated);
  }

  .file-row-open {
    background: var(--bg-elevated);
    box-shadow: 0 1px 0 var(--border-strong);
  }

  .chevron {
    color: var(--text-faint);
    flex-shrink: 0;
    transition: transform 0.15s, color 0.15s;
  }

  .chevron.rotated {
    transform: rotate(90deg);
    color: var(--text-primary);
  }

  /* Name first: on a long path, end truncation would otherwise hide the part you want. */
  .file-folder {
    margin-left: 8px;
    color: var(--text-muted);
  }

  .file-path {
    flex: 1;
    min-width: 0;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    color: var(--text-primary);
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
    font-size: 12px;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 10px;
  }

  .badge-edit {
    background: var(--bg-selected);
    color: var(--text-secondary);
  }

  .badge-read {
    background: var(--bg-selected);
    color: var(--text-secondary);
  }

  .diff-stat {
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    white-space: nowrap;
  }

  .stat-added { color: var(--ok); }
  .stat-removed { color: var(--danger); }

  .edit-failed {
    font-size: 12px;
    font-weight: 600;
    color: var(--danger);
    white-space: nowrap;
  }

  .read-only-note {
    padding: 10px 14px;
    font-size: 12px;
    color: var(--text-faint);
    font-style: italic;
  }

  .edits-list {
    padding: 4px 0;
  }

  .toggle-all-edits {
    margin: 4px 0 8px 14px;
  }

  .edit-block {
    border-top: 1px solid var(--bg-elevated);
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
    background: var(--bg-selected);
  }

  .edit-block-open .edit-header {
    background: var(--bg-selected);
  }

  .edit-action {
    font-family: "SF Mono", "Fira Code", monospace;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 4px;
    font-size: 12px;
    background: var(--bg-selected);
    color: var(--text-secondary);
  }

  .edit-replace-all {
    font-size: 12px;
    color: var(--text-secondary);
    background: var(--bg-hover);
    padding: 1px 6px;
    border-radius: 4px;
  }

  .edit-index {
    font-size: 12px;
    color: var(--text-faint);
    font-family: "SF Mono", "Fira Code", monospace;
  }

  .edit-time {
    font-size: 12px;
    color: var(--text-faint);
    margin-left: auto;
  }

  .edit-body {
    padding: 0 14px 12px;
  }

  .diff-block {
    margin-top: 6px;
    border-radius: 6px;
    overflow: hidden auto;
    max-height: 360px;
    padding: 6px 0;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    line-height: 1.5;
  }

  .diff-line {
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr);
    min-height: 18px;
  }

  .diff-gutter {
    text-align: center;
    color: var(--text-muted);
    user-select: none;
  }

  .diff-text {
    padding-right: 10px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--text-primary);
  }

  .diff-same .diff-text { color: var(--text-secondary); }
  .diff-add { background: var(--ok-soft); }
  .diff-add .diff-gutter { color: var(--ok); font-weight: 600; }
  .diff-del { background: var(--danger-soft); }
  .diff-del .diff-gutter { color: var(--danger); font-weight: 600; }
  .diff-failed .diff-text { text-decoration: line-through; color: var(--text-secondary); }

  .show-full-btn {
    margin-top: 6px;
    min-height: 24px;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 500;
    padding: 3px 10px;
    border-radius: 5px;
    cursor: pointer;
    transition: all 0.12s;
  }

  .show-full-btn:hover {
    color: var(--text-primary);
    border-color: var(--border-strong);
    background: var(--bg-selected);
  }

  .changes-scroll::-webkit-scrollbar {
    width: 8px;
  }

  .changes-scroll::-webkit-scrollbar-thumb {
    background: var(--border);
    border-radius: 4px;
  }
</style>
