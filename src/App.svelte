<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import type {
    ProjectInfo,
    SessionInfo,
    ConversationMessage,
    GlobalSearchResult,
  } from "./lib/types";
  import ProjectGrid from "./lib/ProjectGrid.svelte";
  import SessionList from "./lib/SessionList.svelte";
  import ConversationView from "./lib/ConversationView.svelte";

  let projects: ProjectInfo[] = $state([]);
  let selectedProject: ProjectInfo | null = $state(null);
  let sessions: SessionInfo[] = $state([]);
  let selectedSession: SessionInfo | null = $state(null);
  let messages: ConversationMessage[] = $state([]);
  let loadingMessages = $state(false);
  let loadingSessions = $state(false);
  let initialLoading = $state(true);
  let sortOrder: "newest" | "oldest" = $state("newest");
  let sessionTokenMap: Map<string, number> = $state(new Map());

  let sortedSessions = $derived(
    sortOrder === "newest" ? sessions : [...sessions].reverse()
  );

  async function loadProjects() {
    try {
      projects = await invoke<ProjectInfo[]>("get_projects");
    } catch (loadError) {
      console.error("Failed to load projects:", loadError);
    } finally {
      initialLoading = false;
    }
  }

  async function selectProject(project: ProjectInfo) {
    selectedProject = project;
    selectedSession = null;
    messages = [];
    loadingSessions = true;

    try {
      sessions = await invoke<SessionInfo[]>("scan_projects", {
        projectPath: project.project_path,
      });
      loadTokensInBackground(sessions);
    } catch (loadError) {
      console.error("Failed to load sessions:", loadError);
    } finally {
      loadingSessions = false;
    }
  }

  // Bumped whenever the token loader should abandon its in-flight work — e.g.
  // when the user switches projects or triggers a refresh. The running loop
  // compares this counter on each iteration AND after each awaited invoke so
  // stale invocations stop committing to sessionTokenMap.
  let tokenLoaderGeneration = 0;

  async function loadTokensInBackground(sessionList: SessionInfo[]) {
    tokenLoaderGeneration += 1;
    const myGeneration = tokenLoaderGeneration;
    sessionTokenMap = new Map();

    for (const session of sessionList) {
      if (myGeneration !== tokenLoaderGeneration) return;
      try {
        const tokens = await invoke<number>("get_session_tokens", {
          jsonlPath: session.jsonl_path,
        });
        // Re-check after the await: the user may have navigated away while the
        // Rust scan was running. Don't write stale data into sessionTokenMap.
        if (myGeneration !== tokenLoaderGeneration) return;
        if (tokens > 0) {
          sessionTokenMap = new Map(sessionTokenMap).set(session.session_id, tokens);
        }
      } catch {
        // Skip failures silently
      }
    }
  }

  function goBackToProjects() {
    selectedProject = null;
    selectedSession = null;
    sessions = [];
    sessionTokenMap = new Map();
    messages = [];
  }

  let refreshing = $state(false);

  async function refreshCurrent() {
    if (!selectedProject || refreshing) return;
    refreshing = true;
    const currentProject = selectedProject;
    const currentSessionId = selectedSession?.session_id;

    try {
      // Reload sessions for the current project
      const updatedSessions = await invoke<SessionInfo[]>("scan_projects", {
        projectPath: currentProject.project_path,
      });
      sessions = updatedSessions;
      loadTokensInBackground(updatedSessions);

      // If a session was selected, reload its messages (it may have new content)
      if (currentSessionId) {
        const updatedSession = updatedSessions.find(
          (session) => session.session_id === currentSessionId
        );
        if (updatedSession) {
          selectedSession = updatedSession;
          const updatedMessages = await invoke<ConversationMessage[]>(
            "get_session_messages",
            { jsonlPath: updatedSession.jsonl_path }
          );
          messages = updatedMessages;
        }
      }
    } catch (refreshError) {
      console.error("Failed to refresh:", refreshError);
    } finally {
      refreshing = false;
    }
  }

  async function selectSession(session: SessionInfo) {
    selectedSession = session;
    loadingMessages = true;
    messages = [];

    try {
      messages = await invoke<ConversationMessage[]>("get_session_messages", {
        jsonlPath: session.jsonl_path,
      });
    } catch (loadError) {
      console.error("Failed to load messages:", loadError);
    } finally {
      loadingMessages = false;
    }
  }

  async function openSearchResult(result: GlobalSearchResult) {
    const matchingProject = projects.find(
      (project) => project.project_path === result.project_path
    );
    if (matchingProject) {
      await selectProject(matchingProject);
      const matchingSession = sessions.find(
        (session) => session.session_id === result.session_id
      );
      if (matchingSession) {
        await selectSession(matchingSession);
      }
    }
  }

  // ── Auto-updater ─────────────────────────────────────────

  let pendingUpdate: Update | null = $state(null);
  let updateInstalling = $state(false);
  let updateError = $state("");

  async function checkForUpdates() {
    try {
      const update = await check();
      if (update) {
        pendingUpdate = update;
      }
    } catch (checkError) {
      console.error("Update check failed:", checkError);
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

  $effect(() => {
    loadProjects();
    checkForUpdates();
  });
</script>

{#if pendingUpdate}
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
{/if}

<main>
  {#if initialLoading}
    <div class="loading-screen">
      <div class="loading-spinner"></div>
      <p>Loading projects...</p>
    </div>
  {:else if !selectedProject}
    <ProjectGrid {projects} onSelect={selectProject} onOpenResult={openSearchResult} />
  {:else}
    <div class="app-layout">
      <aside class="sidebar">
        <div class="sidebar-header">
          <button class="back-btn" onclick={goBackToProjects} title="Back to projects">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M19 12H5M12 19l-7-7 7-7" />
            </svg>
          </button>
          <div class="project-title">
            <span class="project-name">{selectedProject.project_name}</span>
            <span class="project-path">{selectedProject.short_path}</span>
          </div>
          <button
            class="refresh-btn"
            class:spinning={refreshing}
            onclick={refreshCurrent}
            disabled={refreshing}
            title="Refresh sessions"
          >
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/>
              <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/>
              <path d="M21 3v5h-5"/>
              <path d="M3 21v-5h5"/>
            </svg>
          </button>
        </div>
        {#if loadingSessions}
          <div class="sidebar-loading">Loading sessions...</div>
        {:else}
          <SessionList
            sessions={sortedSessions}
            selectedSessionId={selectedSession?.session_id ?? null}
            onSelect={selectSession}
            {sortOrder}
            onSortChange={(order) => (sortOrder = order)}
            tokenMap={sessionTokenMap}
          />
        {/if}
      </aside>
      <section class="main-content">
        <ConversationView
          session={selectedSession}
          {messages}
          loading={loadingMessages}
        />
      </section>
    </div>
  {/if}
</main>

<style>
  :global(*) {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
  }

  :global(body) {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
      "Helvetica Neue", Arial, sans-serif;
    background: #12121e;
    color: #e0e0e0;
    overflow: hidden;
  }

  main {
    height: 100vh;
    width: 100vw;
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
    background: linear-gradient(135deg, #1e1e3a, #16162a);
    border: 1px solid #6366f1;
    border-radius: 10px;
    box-shadow: 0 10px 40px rgba(99, 102, 241, 0.25);
    color: #e0e0f0;
    font-size: 13px;
    max-width: 90%;
  }

  .update-banner svg {
    color: #818cf8;
    flex-shrink: 0;
  }

  .update-text {
    white-space: nowrap;
  }

  .update-error {
    color: #f87171;
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
    background: #6366f1;
    color: white;
  }

  .update-btn.primary:hover:not(:disabled) {
    background: #818cf8;
  }

  .update-btn.primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .update-btn.ghost {
    background: transparent;
    color: #8a8aaa;
    padding: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .update-btn.ghost:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.05);
    color: #e0e0f0;
  }

  .loading-screen {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
    gap: 16px;
    color: #7a7a9a;
  }

  .loading-spinner {
    width: 32px;
    height: 32px;
    border: 3px solid #2a2a4a;
    border-top-color: #6366f1;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .app-layout {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    width: 340px;
    min-width: 280px;
    border-right: 1px solid #2a2a4a;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    background: #1a1a2e;
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid #2a2a4a;
  }

  .back-btn {
    background: #2a2a4a;
    border: none;
    color: #a0a0c0;
    width: 32px;
    height: 32px;
    border-radius: 6px;
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

  .refresh-btn {
    background: #2a2a4a;
    border: none;
    color: #a0a0c0;
    width: 30px;
    height: 30px;
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    margin-left: auto;
    transition: all 0.15s;
  }

  .refresh-btn:hover:not(:disabled) {
    background: #3a3a5a;
    color: #e0e0e0;
  }

  .refresh-btn:disabled {
    cursor: not-allowed;
    opacity: 0.7;
  }

  .refresh-btn.spinning svg {
    animation: spin-refresh 0.8s linear infinite;
  }

  @keyframes spin-refresh {
    to {
      transform: rotate(360deg);
    }
  }

  .project-title {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .project-name {
    font-size: 14px;
    font-weight: 600;
    color: #e0e0f0;
  }

  .project-path {
    font-size: 11px;
    color: #5a5a7a;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sidebar-loading {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 40px 16px;
    color: #5a5a7a;
    font-size: 13px;
  }

  .main-content {
    flex: 1;
    min-width: 0;
  }

</style>
