<script lang="ts">
  import type { ProjectInfo } from "./types";
  import { tick } from "svelte";
  import BrandMark from "./BrandMark.svelte";
  import ToolIcon from "./ToolIcon.svelte";
  import { formatToolName, projectLabels } from "./format";
  import { dialogIsOpen } from "./dialog";

  export type Scope = { kind: "all" } | { kind: "saved" } | { kind: "project"; path: string };

  let {
    projects,
    tools,
    scope,
    toolFilter,
    projectCounts,
    loading,
    savedCount,
    searchQuery = $bindable(),
    onScope,
    onToolFilter,
    onOpenSettings,
    onOpenShortcuts,
    onRevealSearch,
    onSearchDismissed,
  }: {
    projects: ProjectInfo[];
    // Every tool with sessions on this machine.
    tools: string[];
    scope: Scope;
    toolFilter: string;
    // Sessions per folder for the current tool filter, including saved copies.
    projectCounts: Map<string, number>;
    loading: boolean;
    savedCount: number;
    searchQuery: string;
    onScope: (scope: Scope) => void;
    onToolFilter: (tool: string) => void;
    onOpenSettings: () => void;
    onOpenShortcuts: () => void;
    onRevealSearch: () => void;
    // Search was dismissed with Esc, so focus and the small-window pane go back where they were.
    onSearchDismissed: () => void;
  } = $props();

  let searchInput: HTMLInputElement | undefined = $state();

  let visibleProjects = $derived(
    toolFilter === "all" ? projects : projects.filter((project) => project.tools.includes(toolFilter))
  );

  let labels = $derived(projectLabels(projects));
  let activeProject = $derived(scope.kind === "project" ? scope.path : null);
  // One Tab stop for the whole list: arrow keys move inside it.
  let projectTabStop = $derived(
    visibleProjects.find((project) => project.project_path === activeProject)?.project_path ?? visibleProjects[0]?.project_path
  );

  function handleProjectKeys(event: KeyboardEvent) {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const rows = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLElement>("button.row")];
    rows[rows.indexOf(event.target as HTMLElement) + (event.key === "ArrowDown" ? 1 : -1)]?.focus();
  }

  // The rail may be hidden when this is pressed, so wait for it to come back.
  async function handleWindowKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "k" && !dialogIsOpen()) {
      event.preventDefault();
      onRevealSearch();
      await tick();
      searchInput?.focus();
      searchInput?.select();
    }
  }

  function handleSearchKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      searchQuery = "";
      searchInput?.blur();
      onSearchDismissed();
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      document.querySelector<HTMLElement>(".board .scroll button.row")?.focus();
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<nav class="sidebar" aria-label="Projects and search">
  <div class="brand">
    <BrandMark size={20} />
    <span>Evertusk</span>
  </div>

  <div class="search">
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="8" /><path d="m21 21-4.35-4.35" /></svg>
    <input
      type="text"
      placeholder="Search everything"
      aria-label="Search all sessions"
      autocomplete="off"
      spellcheck="false"
      bind:value={searchQuery}
      bind:this={searchInput}
      onkeydown={handleSearchKeydown}
    />
    {#if searchQuery}
      <button class="clear" onclick={() => { searchQuery = ""; searchInput?.focus(); }} aria-label="Clear search" title="Clear search (Esc)">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
      </button>
    {:else}
      <kbd>⌘K</kbd>
    {/if}
  </div>

  {#if tools.length > 1}
    <div class="tool-filter" role="group" aria-label="Filter by tool">
      <button class:active={toolFilter === "all"} aria-pressed={toolFilter === "all"} onclick={() => onToolFilter("all")}>All</button>
      {#each tools as tool (tool)}
        <button class:active={toolFilter === tool} aria-pressed={toolFilter === tool} onclick={() => onToolFilter(tool)}><ToolIcon {tool} size={13} />{formatToolName(tool)}</button>
      {/each}
    </div>
  {:else if loading}
    <!-- Holds the filter's place, so the rail doesn't jump when the second tool turns up. -->
    <div class="tool-filter tool-filter-reserved" aria-hidden="true"></div>
  {/if}

  <div class="views">
    <button class="row" class:active={scope.kind === "all" && !searchQuery} aria-current={scope.kind === "all" && !searchQuery} onclick={() => onScope({ kind: "all" })}>
      <span class="name">All activity</span>
    </button>
    <button class="row" class:active={scope.kind === "saved" && !searchQuery} aria-current={scope.kind === "saved" && !searchQuery} onclick={() => onScope({ kind: "saved" })}>
      <span class="name">Saved</span>
      {#if savedCount > 0}<span class="count">{savedCount}</span>{/if}
    </button>
  </div>

  <h2>Projects</h2>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="projects" onkeydown={handleProjectKeys}>
    {#if loading}
      <!-- Saved sessions' projects show first, so a partial list must say more is coming. -->
      <p class="empty" role="status">Reading projects…</p>
    {/if}
    {#each visibleProjects as project, index (index)}
      {@const isActive = scope.kind === "project" && scope.path === project.project_path && !searchQuery}
      <button
        class="row"
        class:active={isActive}
        class:missing={!project.folder_exists}
        aria-current={isActive}
        tabindex={project.project_path === projectTabStop ? 0 : -1}
        title={project.folder_exists ? project.project_path : `${project.project_path} (folder no longer on disk)`}
        onclick={() => onScope({ kind: "project", path: project.project_path })}
      >
        <span class="name">{labels.get(project.project_path)}</span>
        {#if !project.folder_exists}
          <span class="tools">not on disk</span>
        {/if}
        <span class="count">{projectCounts.get(project.project_path) ?? project.session_count}</span>
      </button>
    {:else}
      {#if !loading}<p class="empty">No projects{toolFilter === "all" ? "" : ` with ${formatToolName(toolFilter)} sessions`}.</p>{/if}
    {/each}
  </div>

  <div class="footer">
    <button onclick={onOpenSettings} title="Settings (⌘,)">
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M4 7h10M18 7h2M4 17h2M10 17h10" /><circle cx="16" cy="7" r="2" /><circle cx="8" cy="17" r="2" /></svg>
      Settings
    </button>
    <button onclick={onOpenShortcuts} title="Keyboard shortcuts (?)">
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01" /></svg>
      Shortcuts
    </button>
  </div>
</nav>

<style>
  .sidebar {
    width: 224px;
    flex-shrink: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 14px 10px 8px;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border);
    min-height: 0;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 6px;
    margin-bottom: 12px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .search {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 0 9px;
    margin-bottom: 8px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-muted);
  }

  .search:focus-within {
    border-color: var(--accent);
  }

  .search > svg { flex-shrink: 0; }

  .clear {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    margin-right: -6px;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .clear:hover { background: var(--bg-hover); color: var(--text-primary); }

  .search input {
    flex: 1;
    min-width: 0;
    padding: 7px 0;
    background: transparent;
    border: none;
    outline: none;
    font-size: 13px;
    color: var(--text-primary);
  }

  .search input::placeholder { color: var(--text-muted); }

  kbd {
    font: inherit;
    font-size: 12px;
    color: var(--text-faint);
  }

  .tool-filter {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    padding: 2px;
    margin-bottom: 10px;
    background: var(--bg-elevated);
    border-radius: 6px;
  }

  .tool-filter-reserved { height: 28px; }

  .tool-filter button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    flex: 1;
    min-height: 24px;
    padding: 4px 6px;
    background: transparent;
    border: none;
    border-radius: 4px;
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .tool-filter button:hover { color: var(--text-primary); }

  .tool-filter button.active {
    background: var(--bg-selected);
    color: var(--text-primary);
    font-weight: 500;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 8px;
    background: transparent;
    border: none;
    border-radius: 5px;
    text-align: left;
    font-size: 13px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .row:hover { background: var(--bg-hover); }
  /* .projects clips anything outside the row, so the focus ring sits inside it. */
  .row:focus-visible { outline-offset: -2px; }

  .row.active {
    background: var(--bg-selected);
    color: var(--text-primary);
  }

  .row.missing .name { color: var(--text-muted); }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tools {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-faint);
  }

  .count {
    flex-shrink: 0;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }

  h2 {
    margin: 16px 8px 5px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
  }

  .projects {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .empty {
    padding: 6px 8px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .footer {
    display: flex;
    gap: 2px;
    padding-top: 6px;
  }

  .footer button {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    font-size: 12.5px;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .footer button:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  @media (max-width: 1000px) {
    .sidebar { width: 190px; }
    /* The placeholder needs the room here, and the shortcut is listed under Shortcuts. */
    kbd { display: none; }
  }
</style>
