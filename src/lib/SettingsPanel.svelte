<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    preferences,
    setDateFormat,
    setTheme,
    THEMES,
    type DateFormat,
  } from "./preferences.svelte";
  import { closeDialog, modal } from "./dialog";

  let {
    onClose,
    onArchiveMoved,
    onCheckUpdates,
    updateNote = "",
  }: {
    onClose: () => void;
    // Saved sessions' paths changed, so anything holding them needs a reload.
    onArchiveMoved?: () => void;
    onCheckUpdates?: () => void;
    // How the last update check went.
    updateNote?: string;
  } = $props();

  let appVersion = $state("");
  getVersion().then((version) => (appVersion = version));

  const dateFormats: { value: DateFormat; label: string; description: string }[] = [
    { value: "relative", label: "Relative", description: "“2 hours ago”, “Yesterday”" },
    { value: "absolute", label: "Absolute", description: "A date, such as “Oct 7”" },
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
      onArchiveMoved?.();
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
      onArchiveMoved?.();
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

<dialog class="app-dialog" aria-labelledby="settings-title" use:modal={onClose}>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="dialog-body" tabindex="0">
    <header>
      <h2 id="settings-title">Settings</h2>
      <button class="close-btn" onclick={closeDialog} title="Close (Esc)" aria-label="Close">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    </header>

    <section>
      <h3 class="section-label">Display</h3>
      <div class="setting-row">
        <div class="setting-info">
          <div class="setting-name" id="theme-label">Theme</div>
          <div class="setting-help">Dark charcoal, soft light, or bright white</div>
        </div>
        <div class="segmented" role="radiogroup" aria-labelledby="theme-label">
          {#each THEMES as option}
            <label class="segment" class:segment-active={preferences.theme === option.value} title={option.description}>
              <input class="sr-only" type="radio" name="theme" value={option.value} checked={preferences.theme === option.value} onchange={() => setTheme(option.value)} />
              {option.label}
            </label>
          {/each}
        </div>
      </div>
      <div class="setting-row setting-row-spaced">
        <div class="setting-info">
          <div class="setting-name" id="date-format-label">Date format</div>
          <div class="setting-help">How times appear in the session list</div>
        </div>
        <div class="segmented" role="radiogroup" aria-labelledby="date-format-label">
          {#each dateFormats as option}
            <label class="segment" class:segment-active={preferences.dateFormat === option.value} title={option.description}>
              <input class="sr-only" type="radio" name="date-format" value={option.value} checked={preferences.dateFormat === option.value} onchange={() => setDateFormat(option.value)} />
              {option.label}
            </label>
          {/each}
        </div>
      </div>
    </section>

    <section>
      <h3 class="section-label">Storage</h3>
      <div class="setting-info">
        <div class="setting-name">Saved sessions</div>
        <div class="setting-help">
          When you save a Claude Code session or bookmark a message in one, Evertusk copies
          that whole session here so it survives Claude Code's cleanup: the transcript, its
          agents' transcripts, full tool output and pasted images. Everything stays on this
          Mac. Nothing is uploaded.
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
        <div class="storage-path">{archive?.path ?? "…"}</div>
        {#if archiveError}
          <div class="storage-error" role="alert">{archiveError}</div>
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
      <span class="footer-hint">Evertusk {appVersion} · Not affiliated with Anthropic or OpenAI.</span>
      <button class="storage-btn" onclick={() => onCheckUpdates?.()}>Check for updates</button>
    </footer>
    <p class="update-note" role="status">{updateNote}</p>
  </div>
</dialog>

<style>
  .dialog-body { padding: 18px 26px; }

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
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
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
    font-size: 12px;
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
    position: relative;
    display: flex;
    align-items: center;
    min-height: 24px;
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
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  .segment:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid var(--bg-elevated);
  }

  .footer-hint {
    font-size: 12px;
    color: var(--text-muted);
  }

  .update-note {
    margin-top: 8px;
    font-size: 12px;
    overflow-wrap: anywhere;
    color: var(--text-secondary);
  }

  .update-note:empty { display: none; }

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
    font-size: 12px;
    color: var(--text-muted);
  }

  .storage-tag {
    margin-left: auto;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-text);
    background: var(--accent-soft);
    border: 1px solid var(--accent);
    border-radius: 999px;
    padding: 2px 8px;
  }

  .storage-path {
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    color: var(--text-muted);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 6px 9px;
    overflow-wrap: anywhere;
  }

  .storage-error {
    margin-top: 8px;
    font-size: 12px;
    color: var(--danger);
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
