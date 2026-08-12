<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    preferences,
    setDateFormat,
    setDefaultSearchScope,
    setTheme,
    THEMES,
    type DateFormat,
    type SearchScope,
  } from "./preferences.svelte";

  let { onClose }: { onClose: () => void } = $props();

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }

  const dateFormats: { value: DateFormat; label: string; description: string }[] = [
    { value: "relative", label: "Relative", description: "“2 hours ago”, “Yesterday”" },
    { value: "absolute", label: "Absolute", description: "“Today 14:32”, “7 May, 14:32”" },
  ];

  const scopes: { value: SearchScope; label: string; description: string }[] = [
    { value: "all", label: "All", description: "Match every message" },
    { value: "user", label: "Your prompts", description: "Only search your messages" },
    { value: "assistant", label: "Claude's responses", description: "Only search assistant messages" },
  ];

  // ── Storage (archived sessions) ───────────────────────────────
  interface ArchiveInfo {
    path: string;
    session_count: number;
    total_bytes: number;
    is_custom: boolean;
  }

  let archive = $state<ArchiveInfo | null>(null);
  let archiveBusy = $state(false);
  let archiveError = $state("");

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  async function loadArchiveInfo() {
    try {
      archive = await invoke<ArchiveInfo>("get_archive_info");
    } catch (infoError) {
      archiveError = String(infoError);
    }
  }

  $effect(() => {
    loadArchiveInfo();
  });

  async function changeArchiveLocation() {
    if (archiveBusy) return;
    archiveError = "";
    const chosen = await open({ directory: true, title: "Choose where to keep saved sessions" });
    if (!chosen || typeof chosen !== "string") return;
    archiveBusy = true;
    try {
      archive = await invoke<ArchiveInfo>("set_archive_location", { newParentDir: chosen });
    } catch (moveError) {
      archiveError = String(moveError);
    } finally {
      archiveBusy = false;
    }
  }

  async function resetArchiveLocation() {
    if (archiveBusy) return;
    archiveBusy = true;
    archiveError = "";
    try {
      archive = await invoke<ArchiveInfo>("reset_archive_location");
    } catch (resetError) {
      archiveError = String(resetError);
    } finally {
      archiveBusy = false;
    }
  }

  async function openArchiveFolder() {
    try {
      await invoke("open_archive_location");
    } catch (openError) {
      archiveError = String(openError);
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onClose}>
  <div class="dialog" onclick={(event) => event.stopPropagation()}>
    <header>
      <h2>Settings</h2>
      <button class="close-btn" onclick={onClose} title="Close (Esc)">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    </header>

    <section>
      <div class="section-label">Display</div>
      <div class="setting-row">
        <div class="setting-info">
          <div class="setting-name">Theme</div>
          <div class="setting-help">Dark charcoal, soft light, or bright white</div>
        </div>
        <div class="segmented" role="radiogroup" aria-label="Theme">
          {#each THEMES as option}
            <button
              class="segment"
              class:segment-active={preferences.theme === option.value}
              onclick={() => setTheme(option.value)}
              title={option.description}
            >
              {option.label}
            </button>
          {/each}
        </div>
      </div>
      <div class="setting-row setting-row-spaced">
        <div class="setting-info">
          <div class="setting-name">Date format</div>
          <div class="setting-help">How timestamps render in lists and cards</div>
        </div>
        <div class="segmented" role="radiogroup" aria-label="Date format">
          {#each dateFormats as option}
            <button
              class="segment"
              class:segment-active={preferences.dateFormat === option.value}
              onclick={() => setDateFormat(option.value)}
              title={option.description}
            >
              {option.label}
            </button>
          {/each}
        </div>
      </div>
    </section>

    <section>
      <div class="section-label">Search</div>
      <div class="setting-row">
        <div class="setting-info">
          <div class="setting-name">Default scope</div>
          <div class="setting-help">Which messages the in-conversation search starts with</div>
        </div>
        <div class="segmented" role="radiogroup" aria-label="Default search scope">
          {#each scopes as option}
            <button
              class="segment"
              class:segment-active={preferences.defaultSearchScope === option.value}
              onclick={() => setDefaultSearchScope(option.value)}
              title={option.description}
            >
              {option.label}
            </button>
          {/each}
        </div>
      </div>
    </section>

    <section>
      <div class="section-label">Storage</div>
      <div class="setting-info">
        <div class="setting-name">Saved sessions</div>
        <div class="setting-help">
          When you bookmark a message, Claude Sessions copies that whole session
          here so it survives Claude Code's 30-day cleanup. Stored: transcript,
          subagent logs, pasted images, and a small metadata file. Everything stays
          on your machine — nothing is uploaded.
        </div>
      </div>

      <div class="storage-card">
        <div class="storage-stat-row">
          <div class="storage-stat">
            <span class="storage-stat-value">{archive?.session_count ?? 0}</span>
            <span class="storage-stat-label">sessions</span>
          </div>
          <div class="storage-stat">
            <span class="storage-stat-value">{archive ? formatBytes(archive.total_bytes) : "—"}</span>
            <span class="storage-stat-label">on disk</span>
          </div>
          {#if archive?.is_custom}
            <span class="storage-tag">custom location</span>
          {/if}
        </div>
        <div class="storage-path" title={archive?.path ?? ""}>{archive?.path ?? "…"}</div>
        {#if archiveError}
          <div class="storage-error">{archiveError}</div>
        {/if}
        <div class="storage-actions">
          <button class="storage-btn" onclick={changeArchiveLocation} disabled={archiveBusy}>
            {archiveBusy ? "Moving…" : "Change location…"}
          </button>
          <button class="storage-btn" onclick={openArchiveFolder}>Open folder</button>
          {#if archive?.is_custom}
            <button class="storage-btn ghost" onclick={resetArchiveLocation} disabled={archiveBusy}>
              Reset to default
            </button>
          {/if}
        </div>
      </div>
    </section>

    <footer>
      <span class="footer-hint">Preferences are saved locally.</span>
    </footer>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 250;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    animation: overlay-fade 0.15s ease-out;
  }

  @keyframes overlay-fade {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .dialog {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.6);
    width: 100%;
    max-width: 540px;
    padding: 22px 26px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 18px;
  }

  h2 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    margin: 0;
  }

  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    width: 28px;
    height: 28px;
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .close-btn:hover {
    background: var(--border);
    color: var(--text-primary);
  }

  section {
    padding: 14px 0;
    border-top: 1px solid var(--bg-elevated);
  }

  section:first-of-type {
    border-top: none;
    padding-top: 0;
  }

  .section-label {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-muted);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    margin-bottom: 12px;
  }

  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .setting-row-spaced {
    margin-top: 16px;
  }

  .setting-info {
    flex: 1;
    min-width: 0;
  }

  .setting-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 2px;
  }

  .setting-help {
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .segmented {
    display: inline-flex;
    background: var(--bg-app);
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 2px;
    gap: 1px;
    flex-shrink: 0;
  }

  .segment {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 500;
    padding: 5px 12px;
    border-radius: 5px;
    cursor: pointer;
    transition: all 0.12s;
  }

  .segment:hover {
    color: var(--text-primary);
  }

  .segment-active {
    background: rgba(99, 102, 241, 0.2);
    color: var(--accent-text);
  }

  footer {
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid var(--bg-elevated);
  }

  .footer-hint {
    font-size: 11px;
    color: var(--text-faint);
  }

  /* ── Storage card ── */
  .storage-card {
    margin-top: 12px;
    padding: 12px 14px;
    background: var(--bg-app);
    border: 1px solid var(--border);
    border-radius: 8px;
  }

  .storage-stat-row {
    display: flex;
    align-items: baseline;
    gap: 20px;
    margin-bottom: 10px;
  }

  .storage-stat {
    display: flex;
    align-items: baseline;
    gap: 5px;
  }

  .storage-stat-value {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    font-variant-numeric: tabular-nums;
  }

  .storage-stat-label {
    font-size: 11px;
    color: var(--text-muted);
  }

  .storage-tag {
    margin-left: auto;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--accent-text);
    background: rgba(99, 102, 241, 0.12);
    border: 1px solid rgba(99, 102, 241, 0.25);
    border-radius: 999px;
    padding: 2px 8px;
  }

  .storage-path {
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 11px;
    color: var(--text-muted);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 6px 9px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .storage-error {
    margin-top: 8px;
    font-size: 11px;
    color: #f87171;
  }

  .storage-actions {
    display: flex;
    gap: 8px;
    margin-top: 12px;
    flex-wrap: wrap;
  }

  .storage-btn {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-elevated);
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 6px 12px;
    cursor: pointer;
    transition: all 0.12s;
  }

  .storage-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--accent);
  }

  .storage-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .storage-btn.ghost {
    background: transparent;
    color: var(--text-muted);
  }
</style>
