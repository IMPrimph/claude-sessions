<script lang="ts">
  import { closeDialog, modal } from "./dialog";

  let { onClose }: { onClose: () => void } = $props();

  // Standard meta key on macOS, Ctrl elsewhere (close enough for label purposes)
  const meta = typeof navigator !== "undefined" && navigator.platform.startsWith("Mac") ? "⌘" : "Ctrl";

  const shortcuts: { keys: string[]; description: string }[] = [
    { keys: [meta, "K"], description: "Search everything" },
    { keys: [meta, "F"], description: "Search this session" },
    { keys: ["Enter"], description: "Next match in this session" },
    { keys: ["Shift", "Enter"], description: "Previous match" },
    { keys: ["↑ ↓"], description: "Move through the list while it has focus" },
    { keys: [meta, "1"], description: "Toggle projects" },
    { keys: [meta, "2"], description: "Toggle sessions" },
    { keys: [meta, "3"], description: "Go to the conversation, to scroll it with the keyboard" },
    { keys: [meta, "+"], description: "Make everything larger (" + meta + "− for smaller)" },
    { keys: [meta, ","], description: "Open settings" },
    { keys: ["B"], description: "Open or close Saved" },
    { keys: ["Esc"], description: "Close panels, dialogs, and search" },
    { keys: ["?"], description: "Show this help" },
  ];
</script>

<dialog class="app-dialog" style="--dialog-width: 560px" aria-labelledby="shortcuts-title" use:modal={onClose}>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="dialog-body" tabindex="0">
    <header>
      <h2 id="shortcuts-title">Keyboard shortcuts</h2>
      <button class="close-btn" onclick={closeDialog} title="Close (Esc)" aria-label="Close">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12"/></svg>
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
</dialog>

<style>
  .dialog-body { padding: 16px 24px; }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  h2 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    width: 28px;
    height: 28px;
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .close-btn:hover {
    background: var(--border);
    color: var(--text-primary);
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  li {
    display: grid;
    grid-template-columns: 124px minmax(0, 1fr);
    align-items: center;
    gap: 12px;
    padding: 6px 0;
    border-top: 1px solid var(--bg-elevated);
  }

  li:first-child {
    border-top: none;
  }

  .keys {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .plus {
    color: var(--text-faint);
    font-size: 12px;
  }

  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    background: var(--bg-app);
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 5px;
    padding: 2px 8px;
    min-width: 22px;
  }

  .description {
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.45;
  }
</style>
