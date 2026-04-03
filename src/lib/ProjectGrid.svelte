<script lang="ts">
  import type { ProjectInfo } from "./types";

  let {
    projects,
    onSelect,
  }: {
    projects: ProjectInfo[];
    onSelect: (project: ProjectInfo) => void;
  } = $props();

  let searchQuery = $state("");

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
</script>

<div class="project-grid-page">
  <div class="project-header">
    <h1>Claude Sessions</h1>
    <p class="subtitle">Select a project to view sessions</p>
  </div>

  <div class="search-container">
    <svg class="search-icon" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <circle cx="11" cy="11" r="8" />
      <path d="m21 21-4.35-4.35" />
    </svg>
    <input
      type="text"
      placeholder="Search projects..."
      bind:value={searchQuery}
    />
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

  .search-container {
    position: relative;
    max-width: 480px;
    margin-bottom: 32px;
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
    padding: 12px 16px 12px 44px;
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
