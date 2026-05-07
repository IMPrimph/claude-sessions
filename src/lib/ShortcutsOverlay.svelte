<script lang="ts">
  let { onClose }: { onClose: () => void } = $props();

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }

  // Standard meta key on macOS, Ctrl elsewhere (close enough for label purposes)
  const meta = typeof navigator !== "undefined" && navigator.platform.startsWith("Mac") ? "⌘" : "Ctrl";

  const shortcuts: { keys: string[]; description: string }[] = [
    { keys: [meta, "K"], description: "Focus the global search (project grid)" },
    { keys: [meta, "F"], description: "Search inside the current conversation" },
    { keys: ["Enter"], description: "Jump to next match (in conversation search)" },
    { keys: ["Shift", "Enter"], description: "Jump to previous match" },
    { keys: ["↑", "↓"], description: "Navigate between sessions in the sidebar" },
    { keys: [meta, ","], description: "Open settings" },
    { keys: ["Esc"], description: "Close panels, dialogs, and search" },
    { keys: ["?"], description: "Show this help" },
  ];
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onClose}>
  <div class="dialog" onclick={(event) => event.stopPropagation()}>
    <header>
      <h2>Keyboard shortcuts</h2>
      <button class="close-btn" onclick={onClose} title="Close (Esc)">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    </header>
    <ul>
      {#each shortcuts as shortcut}
        <li>
          <span class="keys">
            {#each shortcut.keys as key, index}
              {#if index > 0}<span class="plus">+</span>{/if}
              <kbd>{key}</kbd>
            {/each}
          </span>
          <span class="description">{shortcut.description}</span>
        </li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 250;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    animation: overlay-fade 0.15s ease-out;
  }

  @keyframes overlay-fade {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .dialog {
    background: #16162a;
    border: 1px solid #2a2a4a;
    border-radius: 12px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.6);
    width: 100%;
    max-width: 480px;
    padding: 20px 24px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  h2 {
    font-size: 14px;
    font-weight: 600;
    color: #e0e0f0;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    margin: 0;
  }

  .close-btn {
    background: transparent;
    border: none;
    color: #7a7a9a;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .close-btn:hover {
    background: #2a2a4a;
    color: #e0e0f0;
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  li {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 9px 0;
    border-top: 1px solid #1e1e36;
  }

  li:first-child {
    border-top: none;
  }

  .keys {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
    min-width: 110px;
  }

  .plus {
    color: #5a5a7a;
    font-size: 11px;
  }

  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 11px;
    font-weight: 600;
    color: #c0c0d8;
    background: #12121e;
    border: 1px solid #2a2a4a;
    border-bottom-width: 2px;
    border-radius: 5px;
    padding: 3px 8px;
    min-width: 22px;
  }

  .description {
    font-size: 13px;
    color: #a0a0c0;
    line-height: 1.45;
  }
</style>
