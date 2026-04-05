<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { ProjectInfo, GlobalSearchResult } from "./types";

  let {
    projects,
    onSelect,
    onOpenResult,
  }: {
    projects: ProjectInfo[];
    onSelect: (project: ProjectInfo) => void;
    onOpenResult: (result: GlobalSearchResult) => void;
  } = $props();

  let searchQuery = $state("");
  let globalResults: GlobalSearchResult[] = $state([]);
  let searching = $state(false);
  let searchTimeout: ReturnType<typeof setTimeout> | null = null;
  let searchInputElement: HTMLInputElement | undefined = $state();
  let showDropdown = $state(false);

  let filteredProjects = $derived(
    projects.filter((project) => {
      if (!searchQuery) return true;
      const query = searchQuery.toLowerCase();
      return (
        project.project_name.toLowerCase().includes(query) ||
        project.short_path.toLowerCase().includes(query)
      );
    })
  );

  async function runGlobalSearch() {
    if (!searchQuery.trim()) {
      globalResults = [];
      return;
    }
    searching = true;
    try {
      globalResults = await invoke<GlobalSearchResult[]>("global_search", {
        query: searchQuery,
      });
    } catch (searchError) {
      console.error("Global search failed:", searchError);
    } finally {
      searching = false;
    }
  }

  function onSearchInput() {
    showDropdown = !!searchQuery.trim();
    if (searchTimeout) clearTimeout(searchTimeout);
    searchTimeout = setTimeout(runGlobalSearch, 300);
  }

  function handleSearchKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      showDropdown = false;
      searchInputElement?.blur();
    }
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "k") {
      event.preventDefault();
      searchInputElement?.focus();
      showDropdown = !!searchQuery.trim();
    }
  }

  function selectResult(result: GlobalSearchResult) {
    showDropdown = false;
    onOpenResult(result);
  }

  function selectProjectFromDropdown(project: ProjectInfo) {
    showDropdown = false;
    searchQuery = "";
    globalResults = [];
    onSelect(project);
  }

  function handleClickOutside(event: MouseEvent) {
    const target = event.target as HTMLElement;
    if (!target.closest(".search-wrapper")) {
      showDropdown = false;
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} onclick={handleClickOutside} />

<div class="project-grid-page">
  <div class="project-header">
    <h1>Claude Sessions</h1>
    <p class="subtitle">Select a project to view sessions</p>
  </div>

  <div class="search-wrapper">
    <div class="search-container">
      <svg class="search-icon" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="11" cy="11" r="8" />
        <path d="m21 21-4.35-4.35" />
      </svg>
      <input
        type="text"
        placeholder="Search projects and sessions..."
        autocomplete="off"
        spellcheck="false"
        bind:value={searchQuery}
        bind:this={searchInputElement}
        oninput={onSearchInput}
        onfocus={() => { if (searchQuery.trim()) showDropdown = true; }}
        onkeydown={handleSearchKeydown}
      />
      <kbd class="search-kbd">&#8984;K</kbd>
    </div>

    {#if showDropdown}
      <div class="search-dropdown">
        {#if filteredProjects.length > 0}
          <div class="dropdown-section">
            <div class="dropdown-section-label">Projects</div>
            {#each filteredProjects.slice(0, 5) as project (project.project_path)}
              <button class="dropdown-item project-item" onclick={() => selectProjectFromDropdown(project)}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                </svg>
                <div class="dropdown-item-text">
                  <span class="dropdown-item-name">{project.project_name}</span>
                  <span class="dropdown-item-meta">{project.short_path} &middot; {project.session_count} session{project.session_count !== 1 ? "s" : ""}</span>
                </div>
              </button>
            {/each}
          </div>
        {/if}

        {#if searching}
          <div class="dropdown-section">
            <div class="dropdown-section-label">Sessions</div>
            <div class="dropdown-status">Searching...</div>
          </div>
        {:else if globalResults.length > 0}
          <div class="dropdown-section">
            <div class="dropdown-section-label">Sessions</div>
            {#each globalResults.slice(0, 8) as result}
              <button class="dropdown-item session-item" onclick={() => selectResult(result)}>
                <div class="dropdown-item-text">
                  <div class="result-header">
                    <span class="result-project">{result.project_name}</span>
                    <span class="result-badge">{result.match_source === "session_name" ? "NAME" : "MESSAGE"}</span>
                  </div>
                  <span class="dropdown-item-name">{result.session_name.length > 80 ? result.session_name.slice(0, 80) + "..." : result.session_name}</span>
                  {#if result.match_source === "message"}
                    <span class="dropdown-item-match">{result.matched_text.length > 120 ? result.matched_text.slice(0, 120) + "..." : result.matched_text}</span>
                  {/if}
                </div>
              </button>
            {/each}
          </div>
        {:else if searchQuery.trim()}
          <div class="dropdown-section">
            <div class="dropdown-section-label">Sessions</div>
            <div class="dropdown-status">No session matches</div>
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <div class="grid">
    {#each filteredProjects as project (project.project_path)}
      <button class="project-card" onclick={() => onSelect(project)}>
        <div class="card-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
            <path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
          </svg>
        </div>
        <div class="card-name">{project.project_name}</div>
        <div class="card-path">{project.short_path}</div>
        <div class="card-meta">
          <span>{project.session_count} session{project.session_count !== 1 ? 's' : ''}</span>
          {#if project.last_active}
            <span class="dot">&middot;</span>
            <span>{project.last_active}</span>
          {/if}
        </div>
      </button>
    {/each}

    {#if filteredProjects.length === 0}
      <div class="empty-state">No projects found</div>
    {/if}
  </div>
</div>

<style>
  .project-grid-page {
    height: 100vh;
    overflow-y: auto;
    background: #12121e;
    padding: 48px 64px;
  }

  .project-header {
    margin-bottom: 32px;
  }

  .project-header h1 {
    font-size: 24px;
    font-weight: 700;
    color: #e0e0f0;
    margin: 0 0 8px 0;
  }

  .subtitle {
    font-size: 14px;
    color: #6a6a8a;
    margin: 0;
  }

  .search-wrapper {
    position: relative;
    max-width: 560px;
    margin-bottom: 32px;
  }

  .search-container {
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-icon {
    position: absolute;
    left: 14px;
    top: 50%;
    transform: translateY(-50%);
    color: #5a5a7a;
    pointer-events: none;
  }

  .search-container input {
    width: 100%;
    background: #1a1a2e;
    border: 1px solid #2a2a4a;
    color: #e0e0e0;
    padding: 12px 48px 12px 44px;
    border-radius: 10px;
    font-size: 14px;
    outline: none;
    box-sizing: border-box;
  }

  .search-container input:focus {
    border-color: #6366f1;
  }

  .search-container input::placeholder {
    color: #5a5a7a;
  }

  .search-kbd {
    position: absolute;
    right: 12px;
    top: 50%;
    transform: translateY(-50%);
    font-size: 10px;
    color: #5a5a7a;
    background: #12121e;
    border: 1px solid #2a2a4a;
    border-radius: 4px;
    padding: 2px 6px;
    font-family: monospace;
    pointer-events: none;
  }

  .search-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    background: #16162a;
    border: 1px solid #2a2a4a;
    border-radius: 10px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
    max-height: 420px;
    overflow-y: auto;
    z-index: 100;
    padding: 4px;
  }

  .dropdown-section {
    padding: 4px 0;
  }

  .dropdown-section + .dropdown-section {
    border-top: 1px solid #2a2a4a;
  }

  .dropdown-section-label {
    font-size: 10px;
    font-weight: 600;
    color: #5a5a7a;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 8px 12px 4px;
  }

  .dropdown-status {
    padding: 12px;
    text-align: center;
    color: #5a5a7a;
    font-size: 12px;
  }

  .dropdown-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    color: #c0c0d8;
    padding: 8px 12px;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.12s;
  }

  .dropdown-item:hover {
    background: rgba(99, 102, 241, 0.1);
  }

  .dropdown-item svg {
    color: #6366f1;
    flex-shrink: 0;
    margin-top: 2px;
  }

  .dropdown-item-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .dropdown-item-name {
    font-size: 13px;
    font-weight: 500;
    color: #d8d8f0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dropdown-item-meta {
    font-size: 11px;
    color: #5a5a7a;
  }

  .dropdown-item-match {
    font-size: 11px;
    color: #7a7a9a;
    line-height: 1.4;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .result-header {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .result-project {
    font-size: 11px;
    color: #6366f1;
    font-weight: 600;
  }

  .result-badge {
    font-size: 9px;
    color: #5a5a7a;
    background: rgba(255, 255, 255, 0.05);
    padding: 1px 5px;
    border-radius: 3px;
    letter-spacing: 0.05em;
  }

  .search-dropdown::-webkit-scrollbar {
    width: 6px;
  }

  .search-dropdown::-webkit-scrollbar-track {
    background: transparent;
  }

  .search-dropdown::-webkit-scrollbar-thumb {
    background: #2a2a4a;
    border-radius: 3px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 16px;
  }

  .project-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    background: #1a1a2e;
    border: 1px solid #2a2a4a;
    border-radius: 12px;
    padding: 20px;
    cursor: pointer;
    transition: all 0.15s ease;
    text-align: left;
    color: inherit;
  }

  .project-card:hover {
    background: #1e1e36;
    border-color: #6366f1;
    transform: translateY(-2px);
  }

  .card-icon {
    color: #6366f1;
    margin-bottom: 12px;
  }

  .card-name {
    font-size: 16px;
    font-weight: 600;
    color: #e0e0f0;
    margin-bottom: 4px;
  }

  .card-path {
    font-size: 12px;
    color: #5a5a7a;
    margin-bottom: 12px;
    word-break: break-all;
  }

  .card-meta {
    font-size: 12px;
    color: #7a7a9a;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    color: #3a3a5a;
  }

  .empty-state {
    grid-column: 1 / -1;
    text-align: center;
    padding: 48px;
    color: #5a5a7a;
    font-size: 14px;
  }

  .project-grid-page::-webkit-scrollbar {
    width: 8px;
  }

  .project-grid-page::-webkit-scrollbar-track {
    background: transparent;
  }

  .project-grid-page::-webkit-scrollbar-thumb {
    background: #2a2a4a;
    border-radius: 4px;
  }
</style>
