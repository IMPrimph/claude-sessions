<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
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

  async function loadTokensInBackground(sessionList: SessionInfo[]) {
    const currentProject = selectedProject;
    sessionTokenMap = new Map();
    for (const session of sessionList) {
      // Stop if user navigated away
      if (selectedProject !== currentProject) return;
      try {
        const tokens = await invoke<number>("get_session_tokens", {
          jsonlPath: session.jsonl_path,
        });
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

  $effect(() => {
    loadProjects();
  });
</script>

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
