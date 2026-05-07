<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { ConversationMessage, SubagentInfo, ToolResultPayload } from "./types";
  import MessageBubble from "./MessageBubble.svelte";

  let {
    subagent,
    sessionId,
    onClose,
  }: {
    subagent: SubagentInfo;
    sessionId: string;
    onClose: () => void;
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
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<aside class="subagent-panel">
  <header class="subagent-header">
    <div class="subagent-meta">
      <span class="subagent-tag">SUBAGENT</span>
      {#if subagent.agent_type}
        <span class="subagent-type">{subagent.agent_type}</span>
      {/if}
    </div>
    <button class="subagent-close" onclick={onClose} title="Close (Esc)">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
    </button>
  </header>

  {#if subagent.description}
    <div class="subagent-description">{subagent.description}</div>
  {/if}

  <div class="subagent-scroll">
    {#if loading}
      <div class="subagent-status">Loading subagent transcript...</div>
    {:else if loadError}
      <div class="subagent-status subagent-error">Failed to load: {loadError}</div>
    {:else if messages.length === 0}
      <div class="subagent-status">No messages found in this subagent's log</div>
    {:else}
      {#each messages as message, index (index)}
        <MessageBubble {message} {sessionId} {toolResults} />
      {/each}
    {/if}
  </div>
</aside>

<style>
  .subagent-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 540px;
    min-width: 380px;
    background: #0e0e1a;
    border-left: 1px solid #2a2a4a;
    flex-shrink: 0;
  }

  .subagent-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid #2a2a4a;
    background: #16162a;
    flex-shrink: 0;
  }

  .subagent-meta {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .subagent-tag {
    font-size: 10px;
    font-weight: 700;
    color: #818cf8;
    background: rgba(99, 102, 241, 0.12);
    border: 1px solid rgba(99, 102, 241, 0.28);
    padding: 2px 8px;
    border-radius: 999px;
    letter-spacing: 0.06em;
  }

  .subagent-type {
    font-size: 13px;
    font-weight: 600;
    color: #c0c0d8;
  }

  .subagent-close {
    background: #2a2a4a;
    border: none;
    color: #a0a0c0;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.15s;
  }

  .subagent-close:hover {
    background: #3a3a5a;
    color: #e0e0e0;
  }

  .subagent-description {
    padding: 10px 18px;
    font-size: 12px;
    color: #9090b0;
    line-height: 1.5;
    border-bottom: 1px solid #1e1e36;
    background: #12121e;
    flex-shrink: 0;
  }

  .subagent-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 12px 18px;
  }

  .subagent-status {
    padding: 60px 16px;
    text-align: center;
    color: #5a5a7a;
    font-size: 13px;
  }

  .subagent-error {
    color: #f87171;
  }

  .subagent-scroll::-webkit-scrollbar {
    width: 8px;
  }

  .subagent-scroll::-webkit-scrollbar-track {
    background: transparent;
  }

  .subagent-scroll::-webkit-scrollbar-thumb {
    background: #2a2a4a;
    border-radius: 4px;
  }
</style>
