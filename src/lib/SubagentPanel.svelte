<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { ConversationMessage, SubagentInfo, ToolResultPayload } from "./types";
  import MessageBubble from "./MessageBubble.svelte";
  import { dialogIsOpen } from "./dialog";

  let {
    subagent,
    sessionId,
    onClose,
    onImageOpen,
    toolName = "Claude",
  }: {
    subagent: SubagentInfo;
    sessionId: string;
    onClose: () => void;
    onImageOpen?: (url: string, label: string) => void;
    toolName?: string;
  } = $props();

  let messages: ConversationMessage[] = $state([]);
  let loading = $state(true);
  let loadError = $state("");
  let toolResults: Record<string, ToolResultPayload> = $state({});

  // Reload whenever subagent identity changes; race-guard with a generation counter.
  let generation = 0;
  $effect(() => {
    const currentSubagent = subagent;
    generation += 1;
    const myGeneration = generation;
    loading = true;
    loadError = "";
    messages = [];
    toolResults = {};
    Promise.all([
      invoke<ConversationMessage[]>("get_subagent_messages", {
        jsonlPath: currentSubagent.jsonl_path,
      }),
      invoke<Record<string, ToolResultPayload>>("get_tool_results", {
        jsonlPath: currentSubagent.jsonl_path,
      }).catch(() => ({} as Record<string, ToolResultPayload>)),
    ])
      .then(([msgs, results]) => {
        if (myGeneration === generation) {
          messages = msgs;
          toolResults = results;
          loading = false;
        }
      })
      .catch((loadErrorValue) => {
        if (myGeneration === generation) {
          loadError = String(loadErrorValue);
          loading = false;
        }
      });
  });

  function handleKeydown(event: KeyboardEvent) {
    // Esc belongs to an open dialog first, and in a search box to the search.
    if (event.key !== "Escape" || dialogIsOpen() || (event.target as HTMLElement).tagName === "INPUT") return;
    event.preventDefault();
    onClose();
  }

  // So PageDown and the arrow keys read the transcript straight away.
  function focusOnOpen(element: HTMLElement) {
    element.focus({ preventScroll: true });
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<aside class="subagent-panel">
  <header class="subagent-header">
    <div class="subagent-column">
      <button class="subagent-close" onclick={onClose} title="Back to all agents (Esc)">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
        All agents
      </button>
      {#if subagent.agent_type}<span class="subagent-type">{subagent.agent_type}</span>{/if}
    </div>
    {#if subagent.description}
      <p class="subagent-column subagent-description">{subagent.description}</p>
    {/if}
  </header>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="subagent-scroll" role="region" aria-label="Agent transcript" tabindex="0" use:focusOnOpen>
    {#if loading}
      <div class="progress" role="progressbar" aria-label="Reading this agent"></div>
      <div class="subagent-status" role="status">Reading this agent's transcript…</div>
    {:else if loadError}
      <div class="subagent-status subagent-error">Couldn't read this agent's transcript: {loadError}</div>
    {:else if messages.length === 0}
      <div class="subagent-status">This agent's transcript is empty</div>
    {:else}
      {#each messages as message, index (index)}
        <MessageBubble assistantLabel={toolName} {message} {sessionId} {toolResults} transcriptPath={subagent.jsonl_path} userLabel="Task" {onImageOpen} />
      {/each}
    {/if}
  </div>
</aside>

<style>
  .subagent-panel {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    width: 100%;
    background: var(--bg-app);
  }

  /* The same column and gutter as the conversation. */
  .subagent-header {
    flex-shrink: 0;
    padding: 10px clamp(24px, calc(100% - 898px), 56px) 10px 18px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-panel);
  }

  .subagent-column {
    display: flex;
    align-items: center;
    justify-content: space-between;
    max-width: 880px;
    margin-inline: auto;
    padding-left: 6px;
  }

  .subagent-type {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .subagent-close {
    gap: 4px;
    padding: 4px 8px 4px 5px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    font-size: 12px;
    color: var(--text-secondary);
    border-radius: 5px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.15s;
  }

  .subagent-close:hover {
    background: var(--border-strong);
    color: var(--text-primary);
  }

  .subagent-description {
    margin-top: 6px;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.5;
  }

  .subagent-scroll {
    flex: 1;
    overflow-y: auto;
    scrollbar-gutter: stable;
    padding: 12px 48px 40px 18px;
  }

  .subagent-scroll:focus-visible {
    outline: none;
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .subagent-scroll > :global(*) {
    max-width: 880px;
    margin-inline: auto;
    padding-left: 6px;
  }

  .subagent-status {
    padding: 60px 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }

  .subagent-error {
    color: var(--danger);
  }
</style>
