<script lang="ts">
  import { tick, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import type {
    ProjectInfo,
    SessionInfo,
    ConversationMessage,
    GlobalSearchResult,
    Bookmark,
  } from "./lib/types";
  import Sidebar, { type Scope } from "./lib/Sidebar.svelte";
  import SessionList from "./lib/SessionList.svelte";
  import ConversationView from "./lib/ConversationView.svelte";
  import ShortcutsOverlay from "./lib/ShortcutsOverlay.svelte";
  import SettingsPanel from "./lib/SettingsPanel.svelte";
  import WelcomeModal from "./lib/WelcomeModal.svelte";
  import { bookmarks, removeBookmark } from "./lib/bookmarks.svelte";
  import { projectLabels, sessionTitle, timeMs } from "./lib/format";
  import { dialogIsOpen } from "./lib/dialog";
  import { preferences, dismissWelcome, setPanels } from "./lib/preferences.svelte";

  const REFRESH_INTERVAL_MS = 20000;
  const SEARCH_DEBOUNCE_MS = 300;
  const SESSION_GONE = "This session's file can't be read any more. You're seeing what was already loaded.";

  // One list of every session from every tool. Each view is just a filter over it.
  let liveProjects: ProjectInfo[] = $state([]);
  let liveSessions: SessionInfo[] = $state([]);
  let archivedSessions: SessionInfo[] = $state([]);
  let initialLoading = $state(true);
  let projectsLoading = $state(true);

  let scope: Scope = $state({ kind: "all" });
  let toolFilter = $state("all");

  let selectedSession = $state<SessionInfo | null>(null);
  let messages: ConversationMessage[] = $state([]);
  let loadingMessages = $state(false);
  let refreshing = $state(false);
  // Set when arriving from a bookmark or search hit, so the conversation jumps there.
  let pendingScrollTimestamp: string | null = $state(null);
  let pendingSearch = $state("");
  // Counts deliberate navigation. A refresh replaces the session object but must leave you
  // where you are, so the view resets on this rather than on the object.
  let navigation = $state(0);
  let messageError = $state("");
  let readProblems: string[] = $state([]);
  let missingArchiveFolders = $state(new Set<string>());

  let archivedSessionIds = $derived(new Set(archivedSessions.map((session) => session.session_id)));

  // Saved sessions whose live transcript has expired still belong in every list.
  let allSessions = $derived.by(() => {
    const liveIds = new Set(liveSessions.map((session) => session.session_id));
    const expired = archivedSessions.filter((session) => !liveIds.has(session.session_id));
    const merged = expired.length === 0 ? liveSessions : [...liveSessions, ...expired];
    return [...merged].sort((first, second) => timeMs(second.modified) - timeMs(first.modified));
  });

  // Projects from live folders, plus any that now exist only as saved sessions.
  let allProjects = $derived.by(() => {
    const livePaths = new Set(liveProjects.map((project) => project.project_path));
    const archiveOnly = new Map<string, ProjectInfo>();
    for (const session of archivedSessions) {
      if (livePaths.has(session.project_path) || liveSessions.some((live) => live.session_id === session.session_id)) continue;
      const modifiedMs = timeMs(session.modified);
      const existing = archiveOnly.get(session.project_path);
      if (existing) {
        existing.session_count += 1;
        existing.last_active_ms = Math.max(existing.last_active_ms, modifiedMs);
        continue;
      }
      archiveOnly.set(session.project_path, {
        tools: [session.tool],
        folder_exists: !missingArchiveFolders.has(session.project_path),
        project_path: session.project_path,
        project_name: session.project_name,
        short_path: session.project_path,
        session_count: 1,
        last_active: null,
        last_active_ms: modifiedMs,
      });
    }
    return [...liveProjects, ...archiveOnly.values()].sort((first, second) => second.last_active_ms - first.last_active_ms);
  });

  let labels = $derived(projectLabels(allProjects));

  let tools = $derived([...new Set(allSessions.map((session) => session.tool))].sort());

  let visibleSessions = $derived(
    allSessions.filter((session) => {
      if (toolFilter !== "all" && session.tool !== toolFilter) return false;
      if (scope.kind === "saved") return archivedSessionIds.has(session.session_id);
      if (scope.kind === "project") return session.project_path === scope.path;
      return true;
    })
  );

  let heading = $derived.by(() => {
    if (scope.kind === "all") return "All activity";
    if (scope.kind === "saved") return "Saved";
    const path = scope.path;
    return labels.get(path) ?? "Project";
  });

  let forkParent = $derived(
    selectedSession?.forked_from_session_id
      ? allSessions.find((entry) => entry.session_id === selectedSession!.forked_from_session_id) ?? null
      : null
  );

  // A running session changes constantly, so new activity is offered rather than yanked in.
  let hasNewActivity = $derived.by(() => {
    if (!selectedSession) return false;
    const current = allSessions.find((session) => session.jsonl_path === selectedSession!.jsonl_path);
    return !!current && current.last_written !== selectedSession.last_written;
  });

  let selectedSessionSaved = $derived(selectedSession ? archivedSessionIds.has(selectedSession.session_id) : false);
  // A saved session is locked while bookmarks depend on it: removing it would break them.
  let selectedSaveLocked = $derived(
    selectedSessionSaved && !!selectedSession && bookmarks.some((entry) => entry.session_id === selectedSession!.session_id)
  );

  async function loadArchivedSessions() {
    try {
      archivedSessions = await invoke<SessionInfo[]>("get_archived_sessions");
      const paths = [...new Set(archivedSessions.map((session) => session.project_path))];
      const existing = (await invoke<string[]>("existing_folders", { paths })) ?? [];
      missingArchiveFolders = new Set(paths.filter((path) => !existing.includes(path)));
    } catch {
      // The archive is optional: everything else works without it.
    }
  }

  // At the smallest window the list and the session share one pane. Searching or changing
  // scope has to bring the list back, or the results would be invisible.
  let listWanted = $state(false);
  const smallWindow = () => matchMedia("(max-width: 1000px)").matches;

  function focusConversation() {
    document.querySelector<HTMLElement>(".messages-container")?.focus({ preventScroll: true });
  }

  function focusSelectedRow() {
    const row = document.querySelector<HTMLElement>(".board .row.selected") ?? document.querySelector<HTMLElement>(".board .scroll button.row");
    row?.scrollIntoView({ block: "nearest" });
    row?.focus({ preventScroll: true });
  }

  // Back keeps the session selected, so its row is still marked in the list.
  async function returnToList() {
    listWanted = true;
    await tick();
    focusSelectedRow();
  }

  async function handleSearchDismissed() {
    if (selectedSession) listWanted = false;
    await tick();
    if (selectedSession && smallWindow()) focusConversation();
    else focusSelectedRow();
  }

  let projectCounts = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const session of allSessions) {
      if (toolFilter === "all" || session.tool === toolFilter) {
        counts.set(session.project_path, (counts.get(session.project_path) ?? 0) + 1);
      }
    }
    return counts;
  });

  // With no session open, the list is the only thing to look at, so it can't be hidden.
  let listHidden = $derived(preferences.listHidden && !!selectedSession);

  let loadError = $state("");
  let loadInFlight = false;

  async function loadAll() {
    if (loadInFlight) return;
    loadInFlight = true;
    try {
      // One after the other: the first read fills the metadata caches the second relies on,
      // so a cold start doesn't decompress every Codex file twice.
      liveSessions = await invoke<SessionInfo[]>("scan_projects");
      initialLoading = false;
      liveProjects = await invoke<ProjectInfo[]>("get_projects");
      readProblems = (await invoke<string[]>("get_read_problems")) ?? [];
      loadError = "";
    } catch (loadFailure) {
      console.error("Failed to load sessions:", loadFailure);
      loadError = String(loadFailure);
    } finally {
      loadInFlight = false;
      initialLoading = false;
      projectsLoading = false;
    }
  }

  // Why the last save fell short, shown in the session header. The transcript can still
  // have been saved, so the saved list reloads either way.
  let saveError = $state("");

  async function handleSessionArchived(archiveError = "") {
    saveError = archiveError;
    await loadArchivedSessions();
  }

  function archiveArguments(session: SessionInfo) {
    return {
      jsonlPath: session.jsonl_path,
      sessionId: session.session_id,
      projectPath: session.project_path,
      projectName: session.project_name,
      title: session.custom_title || session.summary || session.ai_title || null,
    };
  }

  async function toggleSaveSelectedSession() {
    const session = selectedSession;
    if (!session) return;
    saveError = "";
    if (archivedSessionIds.has(session.session_id)) {
      if (bookmarks.some((entry) => entry.session_id === session.session_id)) return;
      try {
        await invoke("unarchive_session", { sessionId: session.session_id });
        await loadArchivedSessions();
      } catch (unsaveError) {
        console.error("Failed to remove saved session:", unsaveError);
        saveError = String(unsaveError);
      }
      return;
    }
    try {
      await invoke("archive_session", archiveArguments(session));
      await loadArchivedSessions();
    } catch (archiveError) {
      await handleSessionArchived(String(archiveError));
    }
  }

  // Keep a saved session's copy current. Only Claude Code sessions are archived.
  function refreshArchiveIfSaved(session: SessionInfo) {
    if (session.tool !== "claude" || !archivedSessionIds.has(session.session_id)) return;
    invoke("archive_session", archiveArguments(session)).catch(() => {
      // The live file has expired: the archive is what's being read, so nothing to do.
    });
  }

  // Why the open session couldn't be reloaded. What was already read stays on screen.
  let reloadProblem = $state("");

  // Bumped per navigation, so a slow response can't overwrite a newer selection.
  let messagesLoaderGeneration = 0;

  async function selectSession(session: SessionInfo, scrollTimestamp: string | null = null, search = "") {
    messagesLoaderGeneration += 1;
    const myGeneration = messagesLoaderGeneration;
    selectedSession = session;
    navigation += 1;
    messageError = "";
    listWanted = false;
    saveError = "";
    pendingScrollTimestamp = scrollTimestamp;
    pendingSearch = search;
    loadingMessages = true;
    messages = [];
    reloadProblem = "";
    // On a small window the row that was clicked has just been hidden.
    if (smallWindow()) tick().then(focusConversation);
    try {
      const loadedMessages = await invoke<ConversationMessage[]>("get_session_messages", { jsonlPath: session.jsonl_path });
      // Arrow-key navigation can outrun a large session's load.
      if (myGeneration !== messagesLoaderGeneration) return;
      messages = loadedMessages;
    } catch (loadFailure) {
      console.error("Failed to load messages:", loadFailure);
      if (myGeneration === messagesLoaderGeneration) messageError = String(loadFailure);
    } finally {
      if (myGeneration === messagesLoaderGeneration) loadingMessages = false;
    }
    refreshArchiveIfSaved(session);
  }

  async function refreshSelectedSession() {
    if (refreshing) return;
    refreshing = true;
    reloadProblem = "";
    try {
      await loadAll();
      const current = selectedSession;
      const updated = allSessions.find((session) => session.jsonl_path === current?.jsonl_path)
        ?? allSessions.find((session) => session.session_id === current?.session_id);
      if (current && !updated) reloadProblem = SESSION_GONE;
      if (updated) {
        messagesLoaderGeneration += 1;
        const myGeneration = messagesLoaderGeneration;
        selectedSession = updated;
        const updatedMessages = await invoke<ConversationMessage[]>("get_session_messages", { jsonlPath: updated.jsonl_path });
        if (myGeneration !== messagesLoaderGeneration) return;
        messages = updatedMessages;
        // This may have superseded a load in progress, which leaves the flag to us.
        loadingMessages = false;
        refreshArchiveIfSaved(updated);
      }
    } catch (refreshFailure) {
      console.error("Failed to refresh:", refreshFailure);
      loadingMessages = false;
      reloadProblem = `${SESSION_GONE} (${refreshFailure})`;
    } finally {
      refreshing = false;
    }
  }

  async function handleArchiveMoved() {
    await loadArchivedSessions();
    await refreshSelectedSession();
  }

  // Open a bookmark. If the live session has expired, its archived copy still opens.
  async function openBookmark(bookmark: Bookmark) {
    const session = allSessions.find((entry) => entry.session_id === bookmark.session_id);
    if (session) {
      await selectSession(session, bookmark.timestamp);
      return;
    }
    try {
      const archivedPath = await invoke<string | null>("get_archived_session_path", { sessionId: bookmark.session_id });
      if (archivedPath) {
        await selectSession(
          {
            session_id: bookmark.session_id,
            tool: "claude",
            archived_in_tool: false,
            summary: null,
            custom_title: null,
            ai_title: null,
            first_prompt: bookmark.preview,
            project_path: bookmark.project_path,
            project_name: bookmark.project_name,
            created: null,
            modified: null,
            last_written: null,
            conversation_count: 0,
            git_branch: null,
            forked_from_session_id: null,
            jsonl_path: archivedPath,
          },
          bookmark.timestamp
        );
      }
    } catch (resolveError) {
      console.error("Failed to resolve archived session:", resolveError);
    }
  }

  // ── Search everything ─────────────────────────────────────

  let searchQuery = $state("");
  let searchResults: GlobalSearchResult[] = $state([]);
  let searching = $state(false);
  // A search can take seconds, so an older one can finish after a newer one started.
  let searchGeneration = 0;

  $effect(() => {
    const query = searchQuery.trim();
    const tool = toolFilter === "all" ? null : toolFilter;
    // Only the query brings the list back. Reading the panel state here would make hiding
    // the list rerun the search, and the search show it again.
    if (query) {
      untrack(() => {
        listWanted = true;
        if (preferences.listHidden) setPanels(preferences.railHidden, false);
      });
    }
    searchGeneration += 1;
    const myGeneration = searchGeneration;
    if (!query) {
      searchResults = [];
      searching = false;
      return;
    }
    searching = true;
    const timer = setTimeout(async () => {
      try {
        const results = await invoke<GlobalSearchResult[]>("global_search", { query, tool });
        if (myGeneration === searchGeneration) searchResults = results;
      } catch (searchFailure) {
        console.error("Search failed:", searchFailure);
      } finally {
        if (myGeneration === searchGeneration) searching = false;
      }
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  async function openSearchResult(result: GlobalSearchResult) {
    const findSession = () => allSessions.find((entry) => entry.jsonl_path === result.jsonl_path);
    // Search reads the disk directly, so it can find a session the list hasn't picked up yet.
    if (!findSession()) await loadAll();
    const session = findSession();
    if (!session) return;
    // Results stay on the left so you can hop between them, and the words stay highlighted.
    const isMessageHit = result.match_source === "message";
    selectSession(session, isMessageHit ? result.timestamp : null, isMessageHit ? searchQuery.trim() : "");
  }

  // ── Auto-updater ─────────────────────────────────────────

  let pendingUpdate: Update | null = $state(null);
  let updateInstalling = $state(false);
  let updateError = $state("");
  let updateStatus: "idle" | "checking" | "no-update" | "error" = $state("idle");
  let updateStatusMessage = $state("");
  const UPDATE_NOTE_MS = 4000;
  // Settings is modal, so a check started there reports there, not on a banner behind it.
  let updateNote = $derived.by(() => {
    if (updateStatus === "checking") return "Checking…";
    if (pendingUpdate) return `Update available: v${pendingUpdate.version}. Close Settings to install.`;
    if (updateStatus === "error") return `Update check failed: ${updateStatusMessage}`;
    return updateStatusMessage;
  });

  async function checkForUpdates(manual = false) {
    updateStatus = "checking";
    updateStatusMessage = "";
    updateError = "";
    try {
      const update = await check();
      if (update) {
        pendingUpdate = update;
        updateStatus = "idle";
      } else {
        updateStatus = "no-update";
        if (manual) {
          updateStatusMessage = "You're on the latest version";
          setTimeout(dismissStatus, UPDATE_NOTE_MS);
        }
      }
    } catch (checkError) {
      console.error("Update check failed:", checkError);
      updateStatus = "error";
      updateStatusMessage = String(checkError);
    }
  }

  async function installUpdate() {
    if (!pendingUpdate || updateInstalling) return;
    updateInstalling = true;
    updateError = "";
    try {
      await pendingUpdate.downloadAndInstall();
      await relaunch();
    } catch (installError) {
      console.error("Update install failed:", installError);
      updateError = String(installError);
      updateInstalling = false;
    }
  }

  function dismissUpdate() {
    pendingUpdate = null;
  }

  function dismissStatus() {
    updateStatus = "idle";
    updateStatusMessage = "";
  }

  $effect(() => {
    loadAll();
    loadArchivedSessions();
    checkForUpdates();
    const timer = setInterval(() => {
      if (!document.hidden) loadAll();
    }, REFRESH_INTERVAL_MS);
    return () => clearInterval(timer);
  });

  let showShortcuts = $state(false);
  let showSettings = $state(false);

  // Every link comes from a transcript. Clicking one must open the browser and never
  // navigate this window away, wherever it was rendered.
  function handleLinkClick(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest("a");
    if (!link) return;
    event.preventDefault();
    invoke("open_external_url", { url: link.href }).catch((openError) => {
      console.error("Cannot open link:", openError);
    });
  }

  function handleGlobalShortcut(event: KeyboardEvent) {
    if (dialogIsOpen()) return;
    // Cmd+, opens settings (macOS convention), even from inputs since it's a real shortcut.
    if ((event.metaKey || event.ctrlKey) && event.key === ",") {
      event.preventDefault();
      showSettings = true;
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === "1") {
      event.preventDefault();
      setPanels(!preferences.railHidden, preferences.listHidden);
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === "2") {
      event.preventDefault();
      // A small window already swaps the list and the session, so there's nothing to toggle.
      if (!smallWindow()) setPanels(preferences.railHidden, !preferences.listHidden);
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === "3") {
      event.preventDefault();
      if (selectedSession) listWanted = false;
      return;
    }
    // Ignore plain-key shortcuts while typing.
    const target = event.target as HTMLElement | null;
    if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) return;
    if (event.key === "?" || (event.key === "/" && event.shiftKey)) {
      event.preventDefault();
      showShortcuts = true;
      return;
    }
    if ((event.key === "b" || event.key === "B") && !event.metaKey && !event.ctrlKey && !event.altKey) {
      event.preventDefault();
      scope = scope.kind === "saved" ? { kind: "all" } : { kind: "saved" };
      searchQuery = "";
      listWanted = true;
      setPanels(preferences.railHidden, false);
    }
  }
</script>

<svelte:window onkeydown={handleGlobalShortcut} onclick={handleLinkClick} />

{#if showSettings}
  <!-- Reported inside Settings. -->
{:else if pendingUpdate}
  <div class="update-banner">
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
      <polyline points="7 10 12 15 17 10"/>
      <line x1="12" y1="15" x2="12" y2="3"/>
    </svg>
    <span class="update-text">
      Update available: <strong>v{pendingUpdate.version}</strong>
      {#if updateError}
        <span class="update-error">— {updateError}</span>
      {/if}
    </span>
    <button class="update-btn primary" onclick={installUpdate} disabled={updateInstalling}>
      {#if updateInstalling}Installing...{:else}Install & Restart{/if}
    </button>
    <button class="update-btn ghost" onclick={dismissUpdate} disabled={updateInstalling} title="Dismiss">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
    </button>
  </div>
{:else if updateStatus === "error" || (updateStatus === "no-update" && updateStatusMessage)}
  <div class="update-banner" class:banner-error={updateStatus === "error"}>
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      {#if updateStatus === "error"}
        <circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/>
      {:else}
        <path d="M20 6L9 17l-5-5"/>
      {/if}
    </svg>
    <span class="update-text">
      {#if updateStatus === "error"}
        Update check failed: <span class="update-error">{updateStatusMessage}</span>
      {:else}
        {updateStatusMessage}
      {/if}
    </span>
    {#if updateStatus === "error"}
      <button class="update-btn ghost" onclick={() => checkForUpdates(true)}>Retry</button>
    {/if}
    <button class="update-btn ghost" onclick={dismissStatus} title="Dismiss">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
    </button>
  </div>
{/if}

{#if showShortcuts}
  <ShortcutsOverlay onClose={() => (showShortcuts = false)} />
{/if}

{#if showSettings}
  <SettingsPanel
    onClose={() => (showSettings = false)}
    onArchiveMoved={handleArchiveMoved}
    onCheckUpdates={() => checkForUpdates(true)}
    {updateNote}
  />
{/if}

{#if !initialLoading && !preferences.hasSeenWelcome}
  <WelcomeModal onDismiss={dismissWelcome} />
{/if}

{#snippet paneFailed(error: unknown, reset: () => void)}
  <div class="pane-failed" role="alert">
    <h2>This part of Evertusk stopped working</h2>
    <p>{String(error)}</p>
    <button class="update-btn ghost" onclick={reset}>Try again</button>
  </div>
{/snippet}

<div class="app" class:has-session={!!selectedSession && !listWanted} class:rail-hidden={preferences.railHidden} class:list-hidden={listHidden}>
  <header class="titlebar" data-tauri-drag-region>
    <div class="title-controls">
      <button class="title-btn back" onclick={returnToList} aria-label="Back to sessions">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
        <span class="tip" role="tooltip">Back to sessions</span>
      </button>
      <button class="title-btn rail-toggle" aria-pressed={!preferences.railHidden} aria-label="Toggle projects" onclick={() => setPanels(!preferences.railHidden, preferences.listHidden)}>
        <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
        <span class="tip" role="tooltip">Toggle projects <kbd>⌘1</kbd></span>
      </button>
      <button class="title-btn list-toggle" aria-pressed={!listHidden} aria-label="Toggle sessions" disabled={!selectedSession} onclick={() => setPanels(preferences.railHidden, !preferences.listHidden)}>
        <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><rect x="3" y="4.5" width="18" height="15" rx="3" /><path d="M9.5 4.5v15" /></svg>
        <span class="tip" role="tooltip">{selectedSession ? "Toggle sessions" : "Open a session first"}{#if selectedSession} <kbd>⌘2</kbd>{/if}</span>
      </button>
    </div>
    {#if selectedSession}
      <h1 class="session-title" data-tauri-drag-region>{sessionTitle(selectedSession)}</h1>
    {:else}
      <h1 class="sr-only">Evertusk</h1>
    {/if}
  </header>
  <div class="panes">
  <svelte:boundary failed={paneFailed}>
  <Sidebar
    projects={allProjects}
    {tools}
    {scope}
    {toolFilter}
    {projectCounts}
    loading={projectsLoading}
    savedCount={archivedSessionIds.size + bookmarks.length}
    bind:searchQuery
    onScope={(nextScope) => {
      scope = nextScope;
      searchQuery = "";
      listWanted = true;
      setPanels(preferences.railHidden, false);
    }}
    onToolFilter={(tool) => (toolFilter = tool)}
    onOpenSettings={() => (showSettings = true)}
    onOpenShortcuts={() => (showShortcuts = true)}
    onRevealSearch={() => setPanels(false, preferences.listHidden)}
    onSearchDismissed={handleSearchDismissed}
  />
  </svelte:boundary>
  <svelte:boundary failed={paneFailed}>
  <SessionList
    {heading}
    sessions={visibleSessions}
    {allSessions}
    selectedPath={selectedSession?.jsonl_path ?? null}
    {labels}
    savedSessionIds={archivedSessionIds}
    loading={initialLoading}
    showProject={scope.kind !== "project"}
    {searchQuery}
    {searchResults}
    {searching}
    savedMode={scope.kind === "saved"}
    {bookmarks}
    {toolFilter}
    problems={readProblems}
    onClearToolFilter={() => (toolFilter = "all")}
    onSelect={(session) => selectSession(session)}
    onOpenResult={openSearchResult}
    onOpenBookmark={openBookmark}
    onRemoveBookmark={removeBookmark}
  />
  </svelte:boundary>
  <main class="main-content">
    <svelte:boundary failed={paneFailed}>
    <ConversationView
      session={selectedSession}
      {messages}
      loading={loadingMessages}
      {loadError}
      hasSessions={allSessions.length > 0}
      readingHistory={initialLoading}
      scrollToTimestamp={pendingScrollTimestamp}
      initialSearch={pendingSearch}
      {navigation}
      {messageError}
      {forkParent}
      onOpenParent={() => forkParent && selectSession(forkParent)}
      saved={selectedSessionSaved}
      saveLocked={selectedSaveLocked}
      onToggleSave={toggleSaveSelectedSession}
      {saveError}
      onSessionArchived={handleSessionArchived}
      {hasNewActivity}
      {refreshing}
      onRefresh={refreshSelectedSession}
      {reloadProblem}
      onDismissReloadProblem={() => (reloadProblem = "")}
      onReturnToList={returnToList}
      chatOnly={preferences.railHidden && listHidden}
    />
    </svelte:boundary>
  </main>
  </div>
</div>

<style>
  :global(*) {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
  }

  :global(body) {
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
    background: var(--bg-app);
    color: var(--text-primary);
    overflow: hidden;
  }

  /* Only for assistive technology. */
  :global(.sr-only) {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  /* Native modals. margin:auto centres them, undoing the reset above. The body is the part that
     scrolls, so Close and every setting stay reachable at 200% zoom. */
  :global(dialog.app-dialog) {
    margin: auto;
    padding: 0;
    width: min(var(--dialog-width, 540px), calc(100vw - 48px));
    max-width: none;
    max-height: calc(100vh - 48px);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.6);
    color: var(--text-primary);
  }

  :global(dialog.app-dialog[open]) {
    display: flex;
    flex-direction: column;
  }

  :global(dialog.app-dialog::backdrop) {
    background: var(--overlay-scrim, rgba(0, 0, 0, 0.62));
  }

  :global(.dialog-body) {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  /* It takes focus so the keys scroll it. A ring round the whole dialog would say nothing. */
  :global(.dialog-body:focus-visible) { outline: none; }

  @media (prefers-reduced-motion: reduce) {
    :global(*), :global(*::before), :global(*::after) {
      animation-duration: 0.01ms !important;
      animation-iteration-count: 1 !important;
      transition-duration: 0.01ms !important;
      scroll-behavior: auto !important;
    }
  }

  .pane-failed {
    flex: 1;
    min-width: 0;
    padding: 24px;
    font-size: 13px;
    line-height: 1.5;
    overflow-wrap: anywhere;
    color: var(--text-muted);
  }

  .pane-failed h2 {
    margin-bottom: 4px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .pane-failed button { padding: 6px 10px; margin-top: 8px; border: 1px solid var(--border-strong); }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    width: 100vw;
  }

  .panes {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  /* The window's own controls sit over the left end, so the bar starts after them. */
  .titlebar {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    height: 46px;
    padding: 0 16px 0 88px;
    background: var(--bg-sidebar);
    border-bottom: 1px solid var(--border);
    user-select: none;
  }

  .title-controls {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }

  .title-btn {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    background: none;
    border: none;
    border-radius: 7px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .title-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .title-btn:disabled { color: var(--text-faint); cursor: default; }
  .title-btn[aria-pressed="false"] { color: var(--text-muted); }

  .tip {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 100;
    display: none;
    align-items: center;
    gap: 8px;
    padding: 6px 8px 6px 10px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.3);
    font-size: 12.5px;
    white-space: nowrap;
    color: var(--text-primary);
    pointer-events: none;
  }

  .title-btn:hover .tip,
  .title-btn:focus-visible .tip { display: flex; }

  .tip kbd {
    padding: 1px 6px;
    background: var(--bg-selected);
    border-radius: 5px;
    font: inherit;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .session-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .back { display: none; }

  .main-content {
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .app.rail-hidden :global(.sidebar) { display: none; }

  @media (min-width: 1001px) {
    .app.list-hidden :global(.board) { display: none; }
  }

  /* On a small window there's room for the session or the list, not both. */
  @media (max-width: 1000px) {
    /* At this width a session already replaces the list, so Back does that job. */
    .list-toggle { display: none; }
    .app.has-session .back { display: flex; }
    .main-content { display: none; }
    .app.has-session :global(.board) { display: none; }
    .app.has-session .main-content { display: block; }
    .app:not(.has-session) .session-title { display: none; }
  }

  .update-banner {
    position: fixed;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 200;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px 8px 14px;
    background: var(--bg-elevated);
    border: 1px solid var(--accent);
    border-radius: 10px;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.3);
    color: var(--text-primary);
    font-size: 13px;
    max-width: 90%;
  }

  .update-banner.banner-error {
    border-color: var(--danger);
  }

  .update-banner.banner-error svg {
    color: var(--danger);
  }

  .update-banner svg {
    color: var(--accent-hover);
    flex-shrink: 0;
  }

  .update-text {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .update-error {
    color: var(--danger);
    margin-left: 6px;
  }

  .update-btn {
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 12px;
    font-weight: 600;
    padding: 6px 12px;
    transition: all 0.15s;
  }

  .update-btn.primary {
    background: var(--accent);
    color: var(--on-accent);
  }

  .update-btn.primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .update-btn.primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .update-btn.ghost {
    background: transparent;
    color: var(--text-muted);
    padding: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .update-btn.ghost:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

</style>
