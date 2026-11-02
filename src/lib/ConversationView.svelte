<script lang="ts">
  import { tick, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { save } from "@tauri-apps/plugin-dialog";
  import type {
    ConversationMessage,
    FileChange,
    SessionInfo,
    SessionStats,
    SubagentInfo,
    ToolResultPayload,
    AnsweredQuestion,
    SessionArtifact,
  } from "./types";
  import { copyToClipboard } from "./clipboard";
  import { closeDialog, dialogIsOpen, modal } from "./dialog";
  import { exportFileName, prettyToolName, formatTokenCompact, formatModel, formatDuration, formatToolName, searchWords, sessionTitle, timeMs } from "./format";
  import type { SearchScope } from "./preferences.svelte";
  import MessageBubble from "./MessageBubble.svelte";
  import ToolIcon from "./ToolIcon.svelte";
  import SubagentPanel from "./SubagentPanel.svelte";
  import FileChangesPanel from "./FileChangesPanel.svelte";

  let {
    session,
    messages,
    loading,
    scrollToTimestamp = null,
    forkParent = null,
    onOpenParent,
    saved = false,
    saveLocked = false,
    onToggleSave,
    onSessionArchived,
    saveError = "",
    loadError = "",
    hasSessions = true,
    readingHistory = false,
    initialSearch = "",
    navigation = 0,
    messageError = "",
    hasNewActivity = false,
    refreshing = false,
    onRefresh,
    reloadProblem = "",
    onDismissReloadProblem,
    onReturnToList,
    chatOnly = false,
  }: {
    session: SessionInfo | null;
    messages: ConversationMessage[];
    loading: boolean;
    // When set (e.g. arriving from a bookmark), scroll to the message with this
    // timestamp and briefly highlight it.
    scrollToTimestamp?: string | null;
    // The parent session this one was forked from, if any (resolved by App).
    forkParent?: SessionInfo | null;
    onOpenParent?: () => void;
    // Whether this session has a local archived copy, and whether that copy is
    // pinned by an existing bookmark (so it can't be un-saved from here).
    saved?: boolean;
    saveLocked?: boolean;
    onToggleSave?: () => void;
    // A bookmark archived this session, so the saved list needs a reload.
    onSessionArchived?: (archiveError?: string) => void;
    // Why the last save fell short.
    saveError?: string;
    // Why the session list couldn't load, and whether there's anything to pick.
    loadError?: string;
    hasSessions?: boolean;
    // The first read of every session on this Mac is still running.
    readingHistory?: boolean;
    // Arriving from a search hit: these words start highlighted.
    initialSearch?: string;
    // Goes up each time a session is opened on purpose, as opposed to refreshed.
    navigation?: number;
    // Why this session's messages couldn't be read.
    messageError?: string;
    // The transcript has changed on disk since it was opened.
    hasNewActivity?: boolean;
    refreshing?: boolean;
    onRefresh?: () => void;
    // Why a reload failed. The messages already read stay on screen.
    reloadProblem?: string;
    onDismissReloadProblem?: () => void;
    // Esc from the conversation goes back to the session's row.
    onReturnToList?: () => void;
    // Both side panels are hidden, so this is all the window shows.
    chatOnly?: boolean;
  } = $props();

  let stats = $state<SessionStats | null>(null);
  let statsLoaderGeneration = 0;

  const TABS = ["conversation", "changes", "agents", "stats"] as const;
  let tab: (typeof TABS)[number] = $state("conversation");
  let failureCursor = $state(-1);
  let currentFailure: Element | null = null;

  // A count is only useful if it leads to the thing it counts. Failed tool calls are rendered
  // only once their fold opens, so they're listed from the data rather than the page.
  let failures = $derived.by(() => {
    const found: { messageIndex: number; ordinal: number }[] = [];
    messages.forEach((message, messageIndex) => {
      let ordinal = 0;
      for (const block of message.blocks ?? []) {
        if (block.kind === "tool" && block.tool_use_id && toolResults[block.tool_use_id]?.is_error) {
          found.push({ messageIndex, ordinal });
          ordinal += 1;
        }
      }
    });
    return found;
  });

  async function jumpToNextFailure() {
    if (failures.length === 0) return;
    tab = "conversation";
    failureCursor = (failureCursor + 1) % failures.length;
    const { messageIndex, ordinal } = failures[failureCursor];
    await tick();
    const slot = scrollContainer?.querySelector(`[data-msg-index="${messageIndex}"]`);
    if (!slot) return;
    for (const fold of slot.querySelectorAll<HTMLDetailsElement>("details.activity")) {
      if (fold.querySelector(".activity-failed")) fold.open = true;
    }
    // Opening a fold renders its tools a moment later, so this keeps looking until they're there.
    settle(() => {
      const pills = slot.querySelectorAll(".tool-pill-error");
      const target = pills[Math.min(ordinal, pills.length - 1)] ?? slot;
      if (target !== currentFailure) {
        currentFailure?.classList.remove("failure-current");
        target.classList.add("failure-current");
        currentFailure = target;
      }
      target.scrollIntoView({ block: "center" });
    });
  }

  async function handleTabKeys(event: KeyboardEvent) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    // currentTarget is gone once the handler has awaited.
    const tabList = event.currentTarget as HTMLElement;
    const step = event.key === "ArrowRight" ? 1 : -1;
    tab = TABS[(TABS.indexOf(tab) + step + TABS.length) % TABS.length];
    await tick();
    tabList.querySelector<HTMLElement>("button.active")?.focus();
  }

  // The scroll buttons depend on the content's height, which changes once messages load.
  $effect(() => {
    if (messages.length > 0 && tab === "conversation") tick().then(() => scrollContainer && handleScroll());
  });
  // Tool runs start folded so your prompts and decisions stand out.
  let showAllActivity = $state(false);
  // With both panels hidden the chat is for reading, so the search bar waits until ⌘F asks for it.
  let searchOpen = $state(false);
  let toolName = $derived(session ? formatToolName(session.tool) : "");
  let hookFailures = $derived(stats ? stats.hooks.reduce((sum, hook) => sum + hook.failures, 0) : 0);
  // Saving copies a Claude Code transcript past its cleanup. No such cleanup is known for
  // Codex, and its compressed files aren't archived yet.
  let canSave = $derived(session?.tool === "claude");

  let fileChanges: FileChange[] = $state([]);
  let fileChangesLoaderGeneration = 0;
  // Until these arrive, "no changes" and "no agents" would be claims nothing has checked.
  let fileChangesLoaded = $state(false);
  let subagentsLoaded = $state(false);
  let statsFailed = $state(false);
  // Badge in the header should reflect the same number the panel shows.
  // Edited only — read-only files are excluded.
  let editedFileCount = $derived(
    fileChanges.filter((change) => change.edit_count > 0).length
  );

  let subagents: SubagentInfo[] = $state([]);
  let openSubagent: SubagentInfo | null = $state(null);
  let subagentLoaderGeneration = 0;

  let toolResults: Record<string, ToolResultPayload> = $state({});
  let toolResultsLoaderGeneration = 0;

  $effect(() => {
    const currentSession = session;
    if (!currentSession) {
      toolResults = {};
      return;
    }
    void navigation; // Opening the same session again clears this, so it has to load again.
    toolResultsLoaderGeneration += 1;
    const myGeneration = toolResultsLoaderGeneration;
    invoke<Record<string, ToolResultPayload>>("get_tool_results", {
      jsonlPath: currentSession.jsonl_path,
    })
      .then((result) => {
        if (myGeneration === toolResultsLoaderGeneration) toolResults = result;
      })
      .catch(() => {
        if (myGeneration === toolResultsLoaderGeneration) toolResults = {};
      });
  });

  // AskUserQuestion Q&A, keyed by the asking tool's tool_use_id. Loaded in the
  // background like tool results; MessageBubble renders it inline under the pill.
  let sessionQuestions: Record<string, AnsweredQuestion[]> = $state({});
  let questionsLoaderGeneration = 0;

  $effect(() => {
    const currentSession = session;
    if (!currentSession) {
      sessionQuestions = {};
      return;
    }
    void navigation; // Opening the same session again clears this, so it has to load again.
    questionsLoaderGeneration += 1;
    const myGeneration = questionsLoaderGeneration;
    invoke<Record<string, AnsweredQuestion[]>>("get_session_questions", {
      jsonlPath: currentSession.jsonl_path,
    })
      .then((result) => {
        if (myGeneration === questionsLoaderGeneration) sessionQuestions = result;
      })
      .catch(() => {
        if (myGeneration === questionsLoaderGeneration) sessionQuestions = {};
      });
  });

  // Published Artifacts, keyed by the Artifact tool's tool_use_id. Rendered as a
  // clickable card (title + copy-link) in place of the generic tool pill.
  let sessionArtifacts: Record<string, SessionArtifact> = $state({});
  let artifactsLoaderGeneration = 0;

  $effect(() => {
    const currentSession = session;
    if (!currentSession) {
      sessionArtifacts = {};
      return;
    }
    void navigation; // Opening the same session again clears this, so it has to load again.
    artifactsLoaderGeneration += 1;
    const myGeneration = artifactsLoaderGeneration;
    invoke<Record<string, SessionArtifact>>("get_session_artifacts", {
      jsonlPath: currentSession.jsonl_path,
    })
      .then((result) => {
        if (myGeneration === artifactsLoaderGeneration) sessionArtifacts = result;
      })
      .catch(() => {
        if (myGeneration === artifactsLoaderGeneration) sessionArtifacts = {};
      });
  });

  $effect(() => {
    const currentSession = session;
    if (!currentSession) {
      subagents = [];
      return;
    }
    void navigation; // Opening the same session again clears this, so it has to load again.
    subagentLoaderGeneration += 1;
    const myGeneration = subagentLoaderGeneration;
    invoke<SubagentInfo[]>("list_subagents", { jsonlPath: currentSession.jsonl_path })
      .then((result) => {
        if (myGeneration === subagentLoaderGeneration) subagents = result;
      })
      .catch(() => {
        if (myGeneration === subagentLoaderGeneration) subagents = [];
      })
      .finally(() => {
        if (myGeneration === subagentLoaderGeneration) subagentsLoaded = true;
      });
  });

  // Closing a transcript puts focus back on the row it was opened from.
  let returnToAgent = "";

  function openAgent(agent: SubagentInfo) {
    returnToAgent = agent.agent_id;
    openSubagent = agent;
  }

  async function closeAgent() {
    openSubagent = null;
    await tick();
    document.querySelector<HTMLElement>(`.agents button[data-agent="${CSS.escape(returnToAgent)}"]`)?.focus();
  }

  function handleAgentOpen(agentId: string, description: string) {
    tab = "agents";
    const found = subagents.find((subagent) => subagent.agent_id === agentId);
    if (found) {
      openAgent(found);
      return;
    }
    // A Codex helper's transcript lives elsewhere, so if it isn't listed there's nothing to open.
    if (session?.tool === "codex") return;
    // Fallback: derive the path from the parent session's jsonl_path. Layout is
    // <project_dir>/<session_id>.jsonl + <project_dir>/<session_id>/subagents/agent-<id>.jsonl.
    if (!session) return;
    const parentJsonlPath = session.jsonl_path;
    const parentDir = parentJsonlPath.substring(0, parentJsonlPath.lastIndexOf("/"));
    const sessionId = session.session_id;
    returnToAgent = agentId;
    openSubagent = {
      agent_id: agentId,
      agent_type: null,
      description,
      jsonl_path: `${parentDir}/${sessionId}/subagents/agent-${agentId}.jsonl`,
      tool_use_id: null,
    };
  }

  let topLevelAgents = $derived(subagents.filter((agent) => !agent.workflow_run));
  let workflowRuns = $derived.by(() => {
    const runs = new Map<string, SubagentInfo[]>();
    for (const agent of subagents) {
      if (agent.workflow_run) runs.set(agent.workflow_run, [...(runs.get(agent.workflow_run) ?? []), agent]);
    }
    return [...runs.entries()];
  });

  // Reload stats whenever the session changes; race-guard with a generation counter
  // so a slow stats fetch can't overwrite results from a newer selection.
  $effect(() => {
    const currentSession = session;
    if (!currentSession) {
      stats = null;
      return;
    }
    void navigation; // Opening the same session again clears this, so it has to load again.
    statsLoaderGeneration += 1;
    const myGeneration = statsLoaderGeneration;
    invoke<SessionStats>("get_session_stats", {
      jsonlPath: currentSession.jsonl_path,
    })
      .then((result) => {
        if (myGeneration === statsLoaderGeneration) stats = result;
      })
      .catch(() => {
        if (myGeneration === statsLoaderGeneration) statsFailed = true;
      });
  });

  // File changes follow the same pattern — race-guarded fetch
  $effect(() => {
    const currentSession = session;
    if (!currentSession) {
      fileChanges = [];
      return;
    }
    void navigation; // Opening the same session again clears this, so it has to load again.
    fileChangesLoaderGeneration += 1;
    const myGeneration = fileChangesLoaderGeneration;
    invoke<FileChange[]>("get_session_file_changes", {
      jsonlPath: currentSession.jsonl_path,
    })
      .then((result) => {
        if (myGeneration === fileChangesLoaderGeneration) fileChanges = result;
      })
      .catch(() => {
        // The tab then says nothing was recorded, the same as an empty session.
      })
      .finally(() => {
        if (myGeneration === fileChangesLoaderGeneration) fileChangesLoaded = true;
      });
  });

  let totalTokens = $derived(stats ? stats.input_tokens + stats.output_tokens : 0);
  let costLabel = $derived(stats?.cost_usd != null ? `${stats.cost_is_partial ? "at least " : ""}$${stats.cost_usd.toFixed(2)}` : "");
  let activeLabel = $derived(stats && stats.active_ms > 0 ? `${stats.active_is_partial ? "at least " : ""}${formatDuration(stats.active_ms)}` : "");

  // Starting a session again needs the folder it ran in as well as its id.
  let resumeCommand = $derived.by(() => {
    if (!session) return "";
    const folder = `'${session.project_path.replaceAll("'", `'\\''`)}'`;
    const resume = session.tool === "codex" ? "codex resume" : "claude --resume";
    return `cd ${folder} && ${resume} ${session.session_id}`;
  });
  let sessionDetails = $derived(
    session
      ? [
          { label: "Folder", value: session.project_path },
          { label: "Session ID", value: session.session_id },
          { label: "Resume command", value: resumeCommand },
          { label: "Transcript file", value: session.jsonl_path },
        ]
      : []
  );
  let showDetails = $state(false);
  let copiedDetail = $state("");

  async function copyDetail(label: string, value: string) {
    try {
      await copyToClipboard(value);
      copiedDetail = `${label} copied`;
    } catch (copyError) {
      copiedDetail = `Couldn't copy: ${copyError}`;
    }
  }

  let scrollContainer: HTMLDivElement | undefined = $state();
  // What's typed, and the query actually applied: re-highlighting a long session on every
  // keystroke is too slow, so typing is debounced.
  let messageSearchQuery = $state("");
  let appliedSearchQuery = $state("");
  const SEARCH_DEBOUNCE_MS = 150;

  $effect(() => {
    const typed = messageSearchQuery.trim();
    if (!typed) {
      appliedSearchQuery = "";
      return;
    }
    const timer = setTimeout(() => (appliedSearchQuery = typed), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });
  let showScrollTop = $state(false);
  let showScrollBottom = $state(false);
  let searchScope: SearchScope = $state("all");

  function setSearchScope(value: SearchScope) {
    searchScope = value;
  }

  // ── Lightbox for full-size image viewing ──
  let lightboxUrl: string | null = $state(null);
  let lightboxLabel = $state("");

  function openLightbox(url: string, label: string) {
    lightboxUrl = url;
    lightboxLabel = label;
  }

  function closeLightbox() {
    lightboxUrl = null;
    lightboxLabel = "";
  }

  // Opening a session, even the one already open, starts from a clean slate. A refresh
  // doesn't count, so it keeps your tab, scroll position, search and open agent.
  $effect(() => {
    if (navigation === 0) return;
    untrack(() => {
      tab = "conversation";
      openSubagent = null;
      failureCursor = -1;
      currentFailure = null;
      exportedPath = "";
      exportError = "";
      stats = null;
      statsFailed = false;
      fileChanges = [];
      fileChangesLoaded = false;
      subagents = [];
      subagentsLoaded = false;
      toolResults = {};
      lastScrolledKey = "";
      searchScope = "all";
      searchOpen = false;
      messageSearchQuery = initialSearch;
      appliedSearchQuery = initialSearch;
      if (scrollContainer) scrollContainer.scrollTop = 0;
    });
  });

  // Bookmarks made by older versions can hold a timestamp from the middle of a turn,
  // so fall back to the last message that started at or before it.
  function messageIndexAt(targetTimestamp: string): number {
    const exact = messages.findIndex((message) => message.timestamp === targetTimestamp);
    if (exact !== -1) return exact;
    const targetTime = timeMs(targetTimestamp);
    return messages.reduce((latest, message, index) => (timeMs(message.timestamp) <= targetTime ? index : latest), -1);
  }

  // Jump-to-message (from a bookmark): once messages are loaded, find the one with
  // the target timestamp, scroll it into view, and flash it. Guarded so it fires
  // once per (session, timestamp) and never re-triggers on later manual scrolls.
  // A search hit carries words to find, so the search below does the scrolling.
  let flashIndex = $state<number | null>(null);
  let lastScrolledKey = "";
  $effect(() => {
    const targetTimestamp = scrollToTimestamp;
    if (!targetTimestamp || initialSearch || !session || messages.length === 0 || !scrollContainer) return;
    const key = `${session.session_id}::${targetTimestamp}`;
    if (key === lastScrolledKey) return;
    const targetIndex = messageIndexAt(targetTimestamp);
    if (targetIndex === -1) return;
    lastScrolledKey = key;
    requestAnimationFrame(() => {
      const target = scrollContainer!.querySelector(`[data-msg-index="${targetIndex}"]`);
      if (!target) return;
      centerMessage(target);
      flashIndex = targetIndex;
      setTimeout(() => {
        if (flashIndex === targetIndex) flashIndex = null;
      }, 2200);
    });
  });

  // Off-screen messages have estimated heights (content-visibility), so one scroll can
  // land well off target once they lay out. Keep aiming until the scroll position holds
  // still, or the person scrolls for themselves.
  let settleGeneration = 0;

  function settle(aim: () => void) {
    settleGeneration += 1;
    const myGeneration = settleGeneration;
    const STILL_FRAMES_NEEDED = 3;
    const MAX_FRAMES = 120;
    let stillFrames = 0;
    let framesUsed = 0;
    let lastPosition = -1;
    const step = () => {
      if (myGeneration !== settleGeneration || !scrollContainer) return;
      aim();
      stillFrames = scrollContainer.scrollTop === lastPosition ? stillFrames + 1 : 0;
      lastPosition = scrollContainer.scrollTop;
      framesUsed += 1;
      if (stillFrames < STILL_FRAMES_NEEDED && framesUsed < MAX_FRAMES) requestAnimationFrame(step);
    };
    step();
  }

  function centerMessage(target: Element) {
    settle(() => target.scrollIntoView({ block: "center" }));
  }

  function cancelSettle() {
    settleGeneration += 1;
  }

  function handleScroll() {
    if (!scrollContainer) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollContainer;
    showScrollTop = scrollTop > 300;
    showScrollBottom = scrollTop < scrollHeight - clientHeight - 300;
  }

  function scrollToTop() {
    const behavior = matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
    scrollContainer?.scrollTo({ top: 0, behavior });
  }

  function scrollToBottom() {
    if (!scrollContainer) return;
    // Off-screen messages only have estimated heights until they're laid out, so the
    // real bottom keeps moving as they appear.
    const container = scrollContainer;
    settle(() => {
      container.scrollTop = container.scrollHeight;
    });
  }

  // Which messages hold every searched word, within the chosen scope. Only they get highlighted.
  let matchedMessages = $derived.by(() => {
    const words = searchWords(appliedSearchQuery);
    const matched = new Set<number>();
    if (words.length === 0) return matched;
    messages.forEach((message, index) => {
      if (searchScope === "user" && message.role !== "user") return;
      if (searchScope === "assistant" && message.role !== "assistant") return;
      const text = message.text.toLowerCase();
      if (words.every((word) => text.includes(word))) matched.add(index);
    });
    return matched;
  });

  // Search steps through the highlighted words themselves, so a step always lands on one and
  // the count is the number you can actually see.
  let hits: Element[] = [];
  let currentHitElement: Element | null = null;
  let hitCount = $state(0);
  let currentHit = $state(0);
  let lastSearchKey = "";

  function collectHits() {
    hits = [...(scrollContainer?.querySelectorAll("mark.search-mark") ?? [])];
    hitCount = hits.length;
  }

  function goToHit(index: number) {
    currentHit = index;
    const hit = hits[index];
    if (!hit) return;
    currentHitElement?.classList.remove("search-current");
    hit.classList.add("search-current");
    currentHitElement = hit;
    settle(() => hit.scrollIntoView({ block: "center", inline: "nearest" }));
  }

  function stepHit(step: number) {
    // Opening or closing folds can change what's on the page since the last step.
    collectHits();
    if (hits.length === 0) return;
    const position = currentHitElement ? hits.indexOf(currentHitElement) : -1;
    goToHit(((position === -1 ? currentHit : position) + step + hits.length) % hits.length);
  }

  // Arriving from a search result, start at the hit in the message that result named.
  function firstHitFor(targetTimestamp: string | null): number {
    if (!targetTimestamp || !session) return 0;
    const key = `${session.session_id}::${targetTimestamp}`;
    if (key === lastScrolledKey) return 0;
    lastScrolledKey = key;
    const targetIndex = messageIndexAt(targetTimestamp);
    const hitIndex = hits.findIndex((hit) => Number(hit.closest("[data-msg-index]")?.getAttribute("data-msg-index")) >= targetIndex);
    return Math.max(hitIndex, 0);
  }

  $effect(() => {
    const query = appliedSearchQuery;
    const searchKey = `${navigation}|${query}|${searchScope}`;
    const hasMessages = matchedMessages.size > 0 || messages.length > 0;
    const targetTimestamp = query === initialSearch ? scrollToTimestamp : null;
    tick().then(() => {
      collectHits();
      // A reload replaces the messages but must leave you on the hit you were reading.
      if (!query || !hasMessages || searchKey === lastSearchKey) return;
      lastSearchKey = searchKey;
      currentHitElement = null;
      goToHit(firstHitFor(targetTimestamp));
    });
  });

  let searchInput: HTMLInputElement | undefined = $state();

  async function handleSearchKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      // Enter straight after typing applies the search, which lands on its first hit.
      const typed = messageSearchQuery.trim();
      if (appliedSearchQuery !== typed) {
        appliedSearchQuery = typed;
        return;
      }
      stepHit(event.shiftKey ? -1 : 1);
    } else if (event.key === "Escape") {
      messageSearchQuery = "";
      searchOpen = false;
      scrollContainer?.focus({ preventScroll: true });
    }
  }

  function clearSessionSearch() {
    messageSearchQuery = "";
    searchInput?.focus();
  }

  function handleScrollerKeydown(event: KeyboardEvent) {
    cancelSettle();
    if (event.key === "Escape") onReturnToList?.();
  }

  async function handleGlobalKeydown(event: KeyboardEvent) {
    if (!(event.metaKey || event.ctrlKey) || !session || dialogIsOpen()) return;
    if (event.key === "f") {
      event.preventDefault();
      tab = "conversation";
      searchOpen = true;
      await tick();
      searchInput?.focus();
    }
    if (event.key === "3") {
      tab = "conversation";
      await tick();
      scrollContainer?.focus({ preventScroll: true });
    }
  }

  // What's said out loud when a count or status changes on screen.
  let announcement = $derived.by(() => {
    if (exportError) return `Export failed: ${exportError}`;
    if (exportedPath) return `Exported to ${exportedPath}`;
    if (!appliedSearchQuery) return "";
    return hitCount > 0 ? `Match ${currentHit + 1} of ${hitCount}` : "No matches";
  });

  function messageLabel(message: ConversationMessage): string {
    const speaker = message.role === "user" ? "You" : message.role === "assistant" ? toolName : "Event";
    const time = formatTimeOnly(message.timestamp);
    return time ? `${speaker}, ${time}` : speaker;
  }

  function formatSessionDate(time: number): string {
    return new Date(time).toLocaleString([], {
      month: "short",
      day: "numeric",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function formatTimeOnly(isoDate: string | null): string {
    const time = timeMs(isoDate);
    return time ? new Date(time).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "";
  }

  // Empty rather than "Invalid Date" when a transcript's timestamps can't be read.
  let sessionDates = $derived.by(() => {
    const created = timeMs(session?.created);
    const modified = timeMs(session?.modified);
    if (!created) return modified ? formatSessionDate(modified) : "";
    if (!modified || modified === created) return formatSessionDate(created);
    const sameDay = new Date(created).toDateString() === new Date(modified).toDateString();
    return `${formatSessionDate(created)} → ${sameDay ? formatTimeOnly(session!.modified) : formatSessionDate(modified)}`;
  });

  let exporting = $state(false);
  // Where the last export went, or why it failed. Both stay up until dismissed or you move on.
  let exportedPath = $state("");
  let exportError = $state("");

  // startIndex omitted → whole session; a value → export from that message
  // (0-based) through the end. The end is always the session end.
  async function exportToMarkdown(startIndex?: number) {
    if (!session || exporting || messages.length === 0) return;
    // Guard against a stray non-number (e.g. an event object) reaching the Rust
    // command, which expects Option<usize>.
    const start = typeof startIndex === "number" ? startIndex : undefined;
    const partial = start != null && start > 0;
    // A slow export can finish after you've moved to another session.
    const startedAt = navigation;
    exporting = true;
    exportedPath = "";
    exportError = "";
    try {
      const title = sessionTitle(session);
      const path = await save({
        title: partial ? "Export from this message onward" : "Export session",
        defaultPath: `${exportFileName(title, session.session_id)}${partial ? "-partial" : ""}.md`,
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (!path) return;
      await invoke("export_session_markdown", {
        jsonlPath: session.jsonl_path,
        savePath: path,
        title,
        startIndex: start ?? null,
      });
      if (startedAt === navigation) exportedPath = path;
    } catch (exportFailure) {
      console.error("Export failed:", exportFailure);
      if (startedAt === navigation) exportError = String(exportFailure);
    } finally {
      exporting = false;
    }
  }
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

{#snippet closeIcon()}
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12"/></svg>
{/snippet}

<div class="sr-only" role="status">{announcement}</div>

<div class="conversation-view" class:chat-only={chatOnly}>
  {#if !session}
    <div class="empty-state">
      {#if readingHistory}
        <h2>Reading your session history</h2>
        <div class="progress empty-progress" role="progressbar" aria-label="Reading sessions"></div>
        <p>Evertusk is going through every Claude Code and Codex session on this Mac. The first load takes the longest.</p>
      {:else if loadError}
        <h2>Couldn't read your session history</h2>
        <p>{loadError}</p>
      {:else if !hasSessions}
        <h2>No sessions yet</h2>
        <p>Evertusk reads the history Claude Code and Codex keep on this Mac. Run either one, and its sessions appear here.</p>
      {:else}
        <h2>Pick a session</h2>
        <p>Choose one from the list, or press <kbd>⌘K</kbd> to search everything you and your agents have written.</p>
      {/if}
    </div>
  {:else}
    <div class="conversation-main">
    <header class="session-header">
      {#if saveError}
        <div class="header-notice notice-error" role="alert">{saveError}</div>
      {/if}
      {#if reloadProblem}
        <div class="header-notice notice-error" role="alert">
          <span>{reloadProblem}</span>
          <button class="nav-btn" onclick={() => onDismissReloadProblem?.()} aria-label="Dismiss">{@render closeIcon()}</button>
        </div>
      {/if}
      {#if exportError}
        <div class="header-notice notice-error" role="alert">
          <span>Couldn't export: {exportError}</span>
          <button class="nav-btn" onclick={() => (exportError = "")} aria-label="Dismiss">{@render closeIcon()}</button>
        </div>
      {/if}
      {#if exportedPath}
        <div class="header-notice">
          <span>Exported to <span class="notice-path">{exportedPath}</span></span>
          <button class="notice-action" onclick={() => invoke("reveal_in_finder", { path: exportedPath })}>Show in Finder</button>
          <button class="notice-action" onclick={() => copyToClipboard(exportedPath)}>Copy location</button>
          <button class="nav-btn" onclick={() => (exportedPath = "")} aria-label="Dismiss">{@render closeIcon()}</button>
        </div>
      {/if}
      {#if !chatOnly}
      <div class="session-info">
        <strong class="tool-line"><ToolIcon tool={session.tool} size={13} />{toolName}</strong>
        <span title={session.project_path}>{session.project_name || session.project_path}</span>
        {#if sessionDates}<span>{sessionDates}</span>{/if}
        {#if session.forked_from_session_id}
          {#if forkParent && onOpenParent}
            <button class="fork-line" onclick={onOpenParent} title="Open the session this was forked from">
              Forked from <span class="fork-line-parent">{sessionTitle(forkParent, 40)}</span>
            </button>
          {:else}
            <span>Forked, parent gone</span>
          {/if}
        {/if}
        {#if session.git_branch}<span>{session.git_branch}</span>{/if}
        {#if stats && stats.models.length > 0}<span>{stats.models.map(formatModel).join(", ")}</span>{/if}
        {#if session.archived_in_tool}<span>Archived in {toolName}</span>{/if}
      </div>
      {/if}
      {#if stats && !chatOnly}
        <div class="summary">
          {#if failures.length > 0}<button class="bad" onclick={jumpToNextFailure} title="Jump to the next failed tool call"><b>{failureCursor >= 0 ? `${failureCursor + 1} of ${failures.length}` : failures.length}</b> tool failure{failures.length === 1 ? "" : "s"}</button>{/if}
          {#if hookFailures > 0}<button class="bad" onclick={() => (tab = "stats")} title="See which hooks failed"><b>{hookFailures.toLocaleString()}</b> hook{hookFailures === 1 ? "" : "s"} failed</button>{/if}
          <div><b>{stats.user_prompt_count}</b> prompt{stats.user_prompt_count === 1 ? "" : "s"}</div>
          <div><b>{fileChangesLoaded ? editedFileCount : "…"}</b> file{editedFileCount === 1 && fileChangesLoaded ? "" : "s"} changed</div>
          {#if costLabel}
            <div title="Claude Code's own estimate, added up across restarts"><b>{costLabel}</b> est. cost</div>
          {:else}
            <div title="{toolName} doesn't record a cost in its session files"><b>{formatTokenCompact(totalTokens)}</b> tokens</div>
          {/if}
          {#if activeLabel}
            <div title="Time the agent spent working, excluding idle time"><b>{activeLabel}</b> active</div>
          {/if}
          {#if stats.inherited_prompts > 0}
            <div title="Copied from the parent session, so not counted above"><b>{stats.inherited_prompts}</b> inherited</div>
          {/if}
        </div>
      {/if}
      {#if !stats && !statsFailed && !chatOnly}
        <div class="summary summary-loading" role="status">Adding up this session…</div>
      {/if}
      <div class="tabs-row">
      <div class="tabs" role="tablist" tabindex="-1" aria-label="Session views" onkeydown={handleTabKeys}>
        <button role="tab" aria-selected={tab === "conversation"} tabindex={tab === "conversation" ? 0 : -1} class:active={tab === "conversation"} onclick={() => (tab = "conversation")}>Conversation</button>
        <button role="tab" aria-selected={tab === "changes"} tabindex={tab === "changes" ? 0 : -1} class:active={tab === "changes"} onclick={() => (tab = "changes")}>Changes{#if editedFileCount > 0}<span>{editedFileCount}</span>{/if}</button>
        <button role="tab" aria-selected={tab === "agents"} tabindex={tab === "agents" ? 0 : -1} class:active={tab === "agents"} onclick={() => (tab = "agents")}>Agents{#if subagents.length > 0}<span>{subagents.length}</span>{/if}</button>
        <button role="tab" aria-selected={tab === "stats"} tabindex={tab === "stats" ? 0 : -1} class:active={tab === "stats"} onclick={() => (tab = "stats")}>Stats</button>
      </div>
        <div class="session-actions">
          {#if hasNewActivity}
            <button class="header-action-btn new-activity" onclick={() => onRefresh?.()} disabled={refreshing}>
              <i class="lamp" aria-hidden="true"></i>New activity
            </button>
          {:else}
            <button class="header-action-btn icon-only" onclick={() => onRefresh?.()} disabled={refreshing} title="Reload this session" aria-label="Reload this session">
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M21 12a9 9 0 0 1-15.74 6.26L3 16M3 12a9 9 0 0 1 15.74-6.26L21 8M21 3v5h-5M3 21v-5h5" /></svg>
            </button>
          {/if}
          <button class="header-action-btn icon-only" onclick={() => { copiedDetail = ""; showDetails = true; }} title="Session details: folder, ID, resume command" aria-label="Session details">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M12 16v-5M12 8h.01" /></svg>
          </button>
          {#if canSave}
            <button
              class="header-action-btn save-btn"
              class:save-btn-saved={saved}
              onclick={() => onToggleSave?.()}
              title={saved
                ? saveLocked
                  ? "Saved on this Mac. It stays saved while you have bookmarks in it."
                  : "Saved on this Mac. Click to remove the copy."
                : "Keep a copy that survives Claude Code's cleanup"}
            >{saved ? "Saved" : "Save"}</button>
          {/if}
          <button class="header-action-btn" onclick={() => exportToMarkdown()} disabled={exporting || messages.length === 0} title={messages.length === 0 ? "Nothing to export yet" : "Export as Markdown"}>
            {exporting ? "Exporting…" : "Export"}
          </button>
        </div>
      </div>
    </header>
    {#if refreshing}<div class="progress" role="progressbar" aria-label="Reloading this session"></div>{/if}

    <div class="tab-panel" role="tabpanel" aria-label={tab}>
    <!-- Stays mounted behind the other tabs, so coming back keeps your place and open folds. -->
    <div class="conversation-pane" class:pane-hidden={tab !== "conversation"}>
    {#if messages.length > 0 && (!chatOnly || searchOpen || messageSearchQuery)}
      <div class="message-search-bar">
      <div class="search-column">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
        <input
          type="text"
          placeholder="Search this session (⌘F)"
          autocomplete="off"
          spellcheck="false"
          aria-label="Search this session"
          bind:value={messageSearchQuery}
          bind:this={searchInput}
          onkeydown={handleSearchKeydown}
        />
        {#if messageSearchQuery}
        <div class="scope-toggle" role="group" aria-label="Search only in">
          <button
            class="scope-chip"
            class:scope-active={searchScope === "all"} aria-pressed={searchScope === "all"}
            onclick={() => setSearchScope("all")}
            title="Search all messages"
          >All</button>
          <button
            class="scope-chip"
            class:scope-active={searchScope === "user"} aria-pressed={searchScope === "user"}
            onclick={() => setSearchScope("user")}
            title="Search only your prompts"
          >You</button>
          <button
            class="scope-chip"
            class:scope-active={searchScope === "assistant"} aria-pressed={searchScope === "assistant"}
            onclick={() => setSearchScope("assistant")}
            title="Search only {toolName}'s responses"
          >{toolName}</button>
        </div>
        {/if}
        <button class="scope-chip activity-toggle" class:scope-active={showAllActivity} aria-pressed={showAllActivity} onclick={() => (showAllActivity = !showAllActivity)} title="Show every tool call without opening each group">{showAllActivity ? "Fold tools" : "Expand tools"}</button>
        {#if messageSearchQuery}
          <span class="match-count">{hitCount > 0 ? `${currentHit + 1} of ${hitCount}` : "No matches"}</span>
          <button class="nav-btn" onclick={() => stepHit(-1)} title="Previous match (Shift+Enter)" aria-label="Previous match">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="m18 15-6-6-6 6"/></svg>
          </button>
          <button class="nav-btn" onclick={() => stepHit(1)} title="Next match (Enter)" aria-label="Next match">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>
          </button>
          <button class="nav-btn" onclick={clearSessionSearch} title="Clear search (Esc)" aria-label="Clear search">{@render closeIcon()}</button>
        {/if}
      </div>
      </div>
    {/if}

    <div class="messages-wrapper">
      <!-- A focusable scroller, so PageDown, Home, End and the arrows read the conversation. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="messages-container" role="region" aria-label="Conversation" tabindex="0" bind:this={scrollContainer} onscroll={handleScroll} onwheel={cancelSettle} onkeydown={handleScrollerKeydown}>
        {#if loading}
          <div class="progress" role="progressbar" aria-label="Reading this session"></div>
          <div class="loading-state" role="status">Reading this session…</div>
        {:else if messages.length === 0}
          <div class="empty-messages" class:message-error={!!messageError}>{messageError ? `Couldn't read this session: ${messageError}` : "No messages found in this session"}</div>
        {:else}
          {#each messages as message, index (index)}
            <article
              data-msg-index={index}
              aria-label={messageLabel(message)}
              class:bookmark-flash={flashIndex === index}
              class="message-slot"
            >
              <MessageBubble
                {message}
                searchQuery={matchedMessages.has(index) ? appliedSearchQuery : ""}
                sessionId={session.session_id}
                onImageOpen={openLightbox}
                onAgentOpen={handleAgentOpen}
                onExportFrom={() => exportToMarkdown(index)}
                {toolResults}
                {subagents}
                transcriptPath={session.jsonl_path}
                questions={sessionQuestions}
                artifacts={sessionArtifacts}
                bookmarkSession={session}
                {onSessionArchived}
                assistantLabel={toolName}
                saveOnBookmark={canSave}
                expandActivity={showAllActivity}
              />
            </article>
          {/each}
        {/if}
      </div>

      {#if showScrollTop}
        <button class="scroll-fab scroll-fab-top" onclick={scrollToTop} title="Scroll to top" aria-label="Scroll to top">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="m18 15-6-6-6 6"/></svg>
        </button>
      {/if}
      {#if showScrollBottom}
        <button class="scroll-fab scroll-fab-bottom" onclick={scrollToBottom} title="Scroll to bottom" aria-label="Scroll to bottom">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>
        </button>
      {/if}
    </div>
    </div>
    {#if tab === "changes" && !fileChangesLoaded}
      <div class="progress" role="progressbar" aria-label="Reading changes"></div>
      <div class="tab-empty" role="status">Reading what changed…</div>
    {:else if tab === "changes"}
      <div class="changes-note"><p class="scope-note">Includes what agents and shell commands wrote, which Stats doesn't count.</p></div>
      <FileChangesPanel changes={fileChanges} {toolName} />
    {:else if tab === "agents"}
      {#if openSubagent}
        <SubagentPanel
          subagent={openSubagent}
          sessionId={session.session_id}
          onClose={closeAgent}
          onImageOpen={openLightbox}
          {toolName}
        />
      {:else if !subagentsLoaded}
        <div class="progress" role="progressbar" aria-label="Looking for agents"></div>
        <div class="tab-empty" role="status">Looking for agents…</div>
      {:else if subagents.length === 0}
        <div class="tab-empty"><strong>No agents in this session.</strong> Helpers the agent starts appear here with their own transcripts.</div>
      {:else}
        <div class="agents">
          {#if topLevelAgents.length > 0}
            <ul>
              {#each topLevelAgents as agent (agent.jsonl_path)}
                <li><button data-agent={agent.agent_id} onclick={() => openAgent(agent)}><b>{agent.description ?? agent.agent_id}</b><span>{agent.agent_type ?? "agent"}</span></button></li>
              {/each}
            </ul>
          {/if}
          {#each workflowRuns as [runId, runAgents], runIndex (runIndex)}
            <details open={workflowRuns.length === 1 && topLevelAgents.length === 0}>
              <summary>Workflow {runId} · {runAgents.length} agents</summary>
              <ul>
                {#each runAgents as agent (agent.jsonl_path)}
                  <li><button data-agent={agent.agent_id} onclick={() => openAgent(agent)}><b>{agent.description ?? agent.agent_id}</b><span>{agent.agent_type ?? "agent"}</span></button></li>
                {/each}
              </ul>
            </details>
          {/each}
        </div>
      {/if}
    {:else if tab === "stats"}
      <div class="stats-tab">
        {#if stats}
          <div class="stats-grid">
            <div class="stat-cell"><div class="stat-label">Input</div><div class="stat-value">{formatTokenCompact(stats.input_tokens)}</div></div>
            <div class="stat-cell"><div class="stat-label">Output</div><div class="stat-value">{formatTokenCompact(stats.output_tokens)}</div></div>
            <div class="stat-cell"><div class="stat-label">Cache read</div><div class="stat-value">{formatTokenCompact(stats.cache_read_tokens)}</div></div>
            {#if stats.cache_creation_tokens > 0}
              <div class="stat-cell"><div class="stat-label">Cache write</div><div class="stat-value">{formatTokenCompact(stats.cache_creation_tokens)}</div></div>
            {/if}
            <div class="stat-cell"><div class="stat-label">API requests</div><div class="stat-value">{stats.assistant_count}</div></div>
            <div class="stat-cell"><div class="stat-label">Thinking sections</div><div class="stat-value">{stats.thinking_block_count}</div></div>
            <div class="stat-cell"><div class="stat-label">Active time</div><div class="stat-value">{activeLabel || "Not recorded"}</div></div>
            <div class="stat-cell"><div class="stat-label">Est. cost</div><div class="stat-value">{costLabel || `Not recorded by ${toolName}`}</div></div>
            {#if stats.lines_added != null}
              <div class="stat-cell"><div class="stat-label">Lines changed</div><div class="stat-value">+{stats.lines_added} −{stats.lines_removed ?? 0}</div></div>
            {/if}
            {#if stats.agent_input_tokens > 0 || stats.agent_output_tokens > 0}
              <div class="stat-cell"><div class="stat-label">Agents' input</div><div class="stat-value">{formatTokenCompact(stats.agent_input_tokens)}</div></div>
              <div class="stat-cell"><div class="stat-label">Agents' output</div><div class="stat-value">{formatTokenCompact(stats.agent_output_tokens)}</div></div>
            {/if}
            {#if stats.inherited_prompts > 0}
              <div class="stat-cell"><div class="stat-label">Prompts from parent</div><div class="stat-value">{stats.inherited_prompts}</div></div>
            {/if}
          </div>
          <p class="scope-note">
            Tokens, API requests and tools count the main conversation only{stats.agent_input_tokens > 0 || stats.agent_output_tokens > 0 ? "; agents' tokens are listed on their own" : ""}. Changes also includes what agents and shell commands wrote.
            {#if stats.inherited_prompts > 0}This is a fork, so everything here excludes what it inherited from its parent.{/if}
            {#if costLabel}Cost is Claude Code's own estimate.{/if}
            {#if stats.cost_is_partial || stats.active_is_partial}The session's record is incomplete, so “at least” marks a lower bound.{/if}
          </p>
          {#if stats.tool_counts.length > 0}
            <h2 class="stats-section-label">Tools</h2>
            <table class="stats-table">
              <thead><tr><th>Tool</th><th>Calls</th></tr></thead>
              <tbody>
                {#each stats.tool_counts as tool (tool.name)}
                  <tr><td>{prettyToolName(tool.name)}</td><td>{tool.count.toLocaleString()}</td></tr>
                {/each}
              </tbody>
            </table>
          {/if}
          {#if stats.hooks.length > 0}
            <h2 class="stats-section-label">Hooks</h2>
            <table class="stats-table">
              <thead><tr><th>Event</th><th>Runs</th><th>Failed</th></tr></thead>
              <tbody>
                {#each stats.hooks as hook (hook.event)}
                  <tr><td>{hook.event}</td><td>{hook.runs.toLocaleString()}</td><td class:bad={hook.failures > 0}>{hook.failures.toLocaleString()}</td></tr>
                {/each}
              </tbody>
            </table>
          {/if}
        {:else}
          {#if statsFailed}
            <div class="tab-empty message-error">Couldn't read this session's stats.</div>
          {:else}
            <div class="progress" role="progressbar" aria-label="Adding up stats"></div>
            <div class="tab-empty" role="status">Adding up this session…</div>
          {/if}
        {/if}
      </div>
    {/if}
    </div>
    </div>
  {/if}
</div>

{#if showDetails && session}
  <dialog class="app-dialog" aria-labelledby="details-title" use:modal={() => (showDetails = false)}>
    <div class="dialog-body details-body">
      <header class="details-head">
        <h2 id="details-title">Session details</h2>
        <button class="nav-btn" onclick={closeDialog} title="Close (Esc)" aria-label="Close">{@render closeIcon()}</button>
      </header>
      <dl>
        {#each sessionDetails as detail (detail.label)}
          <dt>{detail.label}</dt>
          <dd>
            <code>{detail.value}</code>
            <button class="notice-action" onclick={() => copyDetail(detail.label, detail.value)} aria-label="Copy {detail.label}">Copy</button>
          </dd>
        {/each}
      </dl>
      <p class="copied-note" role="status">{copiedDetail}</p>
    </div>
  </dialog>
{/if}

<!-- Image lightbox -->
{#if lightboxUrl}
  <dialog class="lightbox" aria-label={lightboxLabel || "Image"} use:modal={closeLightbox}>
    <button class="lightbox-close" onclick={closeDialog} title="Close (Esc)" aria-label="Close">
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12"/></svg>
    </button>
    {#if lightboxLabel}
      <div class="lightbox-label">{lightboxLabel}</div>
    {/if}
    <img src={lightboxUrl} alt={lightboxLabel} class="lightbox-image" />
  </dialog>
{/if}

<style>
  .conversation-view {
    display: flex;
    height: 100%;
    background: var(--bg-app);
    overflow: hidden;
  }

  .conversation-main {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .empty-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 6px;
    max-width: 460px;
    margin: 0 auto;
    padding: 24px;
    font-size: 13.5px;
    line-height: 1.5;
    color: var(--text-muted);
  }

  .empty-state h2 {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
  }

  kbd {
    padding: 1px 5px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 4px;
    font: inherit;
    font-size: 12px;
  }

  /* Every part of the session shares one column and one gutter. The scrolling panes keep room on
     the right for the scroll buttons; the fixed rows give it back once the window is too narrow
     to centre anything. */
  .session-header {
    flex-shrink: 0;
    padding: 8px clamp(24px, calc(100% - 898px), 56px) 0 18px;
    border-bottom: 1px solid var(--border);
  }

  .session-header > * {
    max-width: 880px;
    margin-inline: auto;
    padding-left: 6px;
  }

  .chat-only .session-header { padding-top: 0; }

  .session-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .header-action-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 26px;
    padding: 4px 10px;
    background: transparent;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .header-action-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .header-action-btn:disabled { opacity: 0.6; cursor: not-allowed; }
  .icon-only { padding: 4px 7px; }

  .new-activity {
    border-color: var(--accent);
    color: var(--accent-text);
  }

  .lamp {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }

  .save-btn-saved {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent-text);
  }

  .header-notice {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
    font-size: 12px;
    line-height: 1.4;
    overflow-wrap: anywhere;
    color: var(--text-secondary);
  }

  .header-notice > span { flex: 1; min-width: 0; }
  .notice-error { color: var(--danger); }
  .notice-path { font-family: "SF Mono", "Fira Code", monospace; }

  .notice-action {
    flex-shrink: 0;
    min-height: 24px;
    padding: 2px 8px;
    background: transparent;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .notice-action:hover { color: var(--text-primary); }

  /* One line each. Whatever doesn't fit wraps onto a hidden line and drops out whole, least
     important last in the markup. The Stats tab always has all of it. */
  .session-info,
  .summary {
    display: flex;
    flex-wrap: wrap;
    gap: 0 10px;
    overflow: hidden;
    white-space: nowrap;
    font-size: 12px;
    line-height: 18px;
    color: var(--text-muted);
  }

  .session-info { height: 18px; }

  .session-info span {
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tool-line {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .session-info strong {
    font-weight: 600;
    color: var(--text-secondary);
  }

  .fork-line {
    padding: 0;
    background: none;
    border: none;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }

  .fork-line:hover .fork-line-parent { text-decoration: underline; }
  .fork-line-parent { font-weight: 500; color: var(--accent-text); }

  .summary {
    height: 20px;
    margin-top: 2px;
    line-height: 20px;
  }


  .empty-progress { margin: 4px 0; }

  .summary button {
    padding: 0;
    background: none;
    border: none;
    font: inherit;
    cursor: pointer;
  }

  .summary button:focus-visible { outline-offset: -2px; }

  .summary button:hover { text-decoration: underline; }

  .summary b {
    margin-right: 2px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--text-primary);
  }

  .summary .bad,
  .summary .bad b,
  td.bad { color: var(--danger); }

  .tabs-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 3px 12px;
    margin-top: 2px;
  }

  .tabs {
    display: flex;
    gap: 14px;
    min-width: 0;
  }

  /* At 200% zoom the messages need the height more than the summary does. */
  @media (max-height: 500px) {
    .session-info, .summary { display: none; }
  }

  .tab-panel {
    position: relative;
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }

  .conversation-pane {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }

  /* Not display:none, which would lose the scroll position. */
  .conversation-pane.pane-hidden {
    position: absolute;
    inset: 0;
    visibility: hidden;
  }

  .message-error { color: var(--danger); overflow-wrap: anywhere; }

  .tabs button {
    display: flex;
    gap: 5px;
    padding: 7px 0;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    font-size: 13px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .tabs button:hover { color: var(--text-primary); }

  .tabs button.active {
    border-bottom-color: var(--accent);
    color: var(--text-primary);
    font-weight: 500;
  }

  .tabs span {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }

  .message-search-bar {
    flex-shrink: 0;
    padding: 6px clamp(24px, calc(100% - 898px), 56px) 6px 18px;
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
  }

  .search-column {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
    /* Holds its height when the scope chips appear, so the messages don't shift. */
    min-height: 30px;
    max-width: 880px;
    margin-inline: auto;
    padding-left: 6px;
  }

  .message-search-bar svg { flex-shrink: 0; }

  .message-search-bar input {
    flex: 1 1 120px;
    min-width: 0;
    padding: 5px 9px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 5px;
    outline: none;
    font-size: 12.5px;
    color: var(--text-primary);
  }

  .message-search-bar input:focus { border-color: var(--accent); }
  .message-search-bar input::placeholder { color: var(--text-muted); }

  .scope-toggle {
    display: inline-flex;
    flex-shrink: 0;
    padding: 2px;
    gap: 1px;
    background: var(--bg-elevated);
    border-radius: 5px;
  }

  .scope-chip {
    flex-shrink: 0;
    min-height: 24px;
    padding: 3px 8px;
    background: transparent;
    border: none;
    border-radius: 4px;
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .scope-chip:hover { color: var(--text-primary); }

  .scope-chip.scope-active {
    background: var(--bg-selected);
    color: var(--text-primary);
  }

  .activity-toggle { border: 1px solid var(--border); }

  .match-count {
    flex-shrink: 0;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    color: var(--text-muted);
  }

  .nav-btn {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: var(--bg-elevated);
    border: none;
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .nav-btn:hover { background: var(--bg-hover); color: var(--text-primary); }

  /* The hit you're on, among all the highlighted ones. */
  :global(mark.search-mark.search-current) {
    outline: 2px solid var(--text-primary);
    outline-offset: 1px;
  }

  :global(.failure-current) {
    outline: 2px solid var(--danger);
    outline-offset: 2px;
  }

  /* Long sessions: the browser skips layout and paint for off-screen messages. */
  /* Off-screen messages skip layout. This also clips to the slot, so the padding gives
     wrapped inline code room for its background. */
  .message-slot {
    content-visibility: auto;
    contain-intrinsic-size: auto 120px;
    padding-inline: 6px;
  }

  .bookmark-flash {
    animation: bookmark-flash-fade 2.2s ease-out;
  }

  @keyframes bookmark-flash-fade {
    0% { background: var(--accent-soft); }
    100% { background: transparent; }
  }

  .messages-wrapper {
    flex: 1;
    min-height: 0;
    position: relative;
    overflow: hidden;
  }

  .messages-container,
  .agents,
  .stats-tab {
    overflow-y: auto;
    /* Reserved even when nothing overflows, so switching tabs doesn't move the column. */
    scrollbar-gutter: stable;
    /* The right gutter keeps the scroll buttons off the text. */
    padding: 12px 48px 40px 18px;
  }

  .messages-container {
    height: 100%;
  }

  /* A ring round a whole pane is loud, so focus here is a bar down its edge. */
  .messages-container:focus-visible {
    outline: 1px solid var(--border-strong);
    outline-offset: -1px;
  }

  /* A comfortable reading width, centred, so spare room never piles up on one side. */
  .messages-container > :global(*),
  .agents > :global(*),
  .stats-tab > :global(*) {
    max-width: 880px;
    margin-inline: auto;
    padding-left: 6px;
  }

  .scope-note {
    margin-top: 14px;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-muted);
  }

  .changes-note {
    flex-shrink: 0;
    padding: 8px clamp(24px, calc(100% - 898px), 56px) 0 18px;
  }

  .changes-note p {
    max-width: 880px;
    margin: 0 auto;
    padding-left: 6px;
  }

  /* Changes keeps its own markup, so it joins the shared column from here. */
  .tab-panel :global(.changes-header) {
    padding-inline: 18px clamp(24px, calc(100% - 898px), 56px);
  }

  .tab-panel :global(.changes-header > *) {
    max-width: 880px;
    margin-inline: auto;
    padding-left: 6px;
  }

  .tab-panel :global(.changes-scroll) {
    scrollbar-gutter: stable;
    padding: 8px 48px 40px 24px;
  }

  .tab-panel :global(.changes-scroll > *) {
    max-width: 874px;
    margin-inline: auto;
  }

  .scroll-fab {
    position: absolute;
    right: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-strong);
    border-radius: 50%;
    color: var(--text-secondary);
    cursor: pointer;
    z-index: 10;
  }

  .scroll-fab:hover { color: var(--text-primary); border-color: var(--accent); }
  .scroll-fab-top { top: 10px; }
  .scroll-fab-bottom { bottom: 12px; }

  .loading-state,
  .empty-messages,
  .tab-empty {
    padding: 40px 24px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-muted);
  }

  .tab-empty strong {
    display: block;
    font-weight: 500;
    color: var(--text-primary);
  }

  .agents {
    flex: 1;
    min-height: 0;
  }

  .agents ul { list-style: none; }

  .agents li button {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    width: 100%;
    padding: 9px 4px;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border);
    text-align: left;
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
  }

  .agents li button:hover { background: var(--bg-hover); }
  .agents b { flex: 1; min-width: 0; font-weight: 500; overflow-wrap: anywhere; }
  .agents li span { flex-shrink: 0; font-size: 12px; color: var(--text-muted); }

  .agents summary {
    padding: 12px 4px 6px;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .stats-tab {
    flex: 1;
    min-height: 0;
  }

  .stats-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 16px 12px;
  }

  .stat-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .stat-value {
    margin-top: 1px;
    font-size: 15px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    color: var(--text-primary);
  }

  .stats-section-label {
    margin: 24px 0 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .stats-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }

  .stats-table th {
    padding: 5px 4px;
    border-bottom: 1px solid var(--border-strong);
    text-align: right;
    font-weight: 500;
    color: var(--text-muted);
  }

  .stats-table td {
    padding: 5px 4px;
    border-bottom: 1px solid var(--border);
    text-align: right;
    color: var(--text-secondary);
  }

  .stats-table th:first-child,
  .stats-table td:first-child {
    padding-left: 6px;
    text-align: left;
    overflow-wrap: anywhere;
  }

  .details-body { padding: 16px 20px 18px; }

  .details-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .details-head h2 { font-size: 14px; font-weight: 600; }

  dt {
    margin-top: 10px;
    font-size: 12px;
    color: var(--text-muted);
  }

  dd {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-top: 3px;
  }

  dd code {
    flex: 1;
    min-width: 0;
    padding: 5px 8px;
    background: var(--bg-app);
    border: 1px solid var(--border);
    border-radius: 5px;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    line-height: 1.4;
    overflow-wrap: anywhere;
    color: var(--text-primary);
  }

  .copied-note {
    min-height: 18px;
    margin-top: 10px;
    font-size: 12px;
    color: var(--text-muted);
  }

  /* Always dark, like a photo viewer, whichever theme is on. */
  .lightbox {
    margin: 0;
    padding: 40px;
    width: 100vw;
    height: 100vh;
    max-width: none;
    max-height: none;
    background: rgba(0, 0, 0, 0.88);
    border: none;
    overflow: hidden;
    cursor: zoom-out;
  }

  .lightbox[open] {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .lightbox-image {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: 6px;
    cursor: default;
  }

  .lightbox-close {
    position: absolute;
    top: 16px;
    right: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.16);
    border-radius: 50%;
    color: #fff;
    cursor: pointer;
  }

  .lightbox-label {
    position: absolute;
    top: 22px;
    left: 22px;
    font-size: 13px;
    color: #e5e5e5;
  }
</style>
