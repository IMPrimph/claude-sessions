<script lang="ts">
  import {
    preferences,
    setDateFormat,
    setDefaultSearchScope,
    type DateFormat,
    type SearchScope,
  } from "./preferences.svelte";

  let { onClose }: { onClose: () => void } = $props();

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }

  const dateFormats: { value: DateFormat; label: string; description: string }[] = [
    { value: "relative", label: "Relative", description: "“2 hours ago”, “Yesterday”" },
    { value: "absolute", label: "Absolute", description: "“Today 14:32”, “7 May, 14:32”" },
  ];

  const scopes: { value: SearchScope; label: string; description: string }[] = [
    { value: "all", label: "All", description: "Match every message" },
    { value: "user", label: "Your prompts", description: "Only search your messages" },
    { value: "assistant", label: "Claude's responses", description: "Only search assistant messages" },
  ];
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onClose}>
  <div class="dialog" onclick={(event) => event.stopPropagation()}>
    <header>
      <h2>Settings</h2>
      <button class="close-btn" onclick={onClose} title="Close (Esc)">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    </header>

    <section>
      <div class="section-label">Display</div>
      <div class="setting-row">
        <div class="setting-info">
          <div class="setting-name">Date format</div>
          <div class="setting-help">How timestamps render in lists and cards</div>
        </div>
        <div class="segmented" role="radiogroup" aria-label="Date format">
          {#each dateFormats as option}
            <button
              class="segment"
              class:segment-active={preferences.dateFormat === option.value}
              onclick={() => setDateFormat(option.value)}
              title={option.description}
            >
              {option.label}
            </button>
          {/each}
        </div>
      </div>
    </section>

    <section>
      <div class="section-label">Search</div>
      <div class="setting-row">
        <div class="setting-info">
          <div class="setting-name">Default scope</div>
          <div class="setting-help">Which messages the in-conversation search starts with</div>
        </div>
        <div class="segmented" role="radiogroup" aria-label="Default search scope">
          {#each scopes as option}
            <button
              class="segment"
              class:segment-active={preferences.defaultSearchScope === option.value}
              onclick={() => setDefaultSearchScope(option.value)}
              title={option.description}
            >
              {option.label}
            </button>
          {/each}
        </div>
      </div>
    </section>

    <footer>
      <span class="footer-hint">Preferences are saved locally.</span>
    </footer>
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
    max-width: 540px;
    padding: 22px 26px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 18px;
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

  section {
    padding: 14px 0;
    border-top: 1px solid #1e1e36;
  }

  section:first-of-type {
    border-top: none;
    padding-top: 0;
  }

  .section-label {
    font-size: 10px;
    font-weight: 600;
    color: #7a7a9a;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    margin-bottom: 12px;
  }

  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .setting-info {
    flex: 1;
    min-width: 0;
  }

  .setting-name {
    font-size: 13px;
    font-weight: 500;
    color: #e0e0f0;
    margin-bottom: 2px;
  }

  .setting-help {
    font-size: 11px;
    color: #7a7a9a;
    line-height: 1.4;
  }

  .segmented {
    display: inline-flex;
    background: #12121e;
    border: 1px solid #2a2a4a;
    border-radius: 7px;
    padding: 2px;
    gap: 1px;
    flex-shrink: 0;
  }

  .segment {
    background: transparent;
    border: none;
    color: #8a8aaa;
    font-size: 12px;
    font-weight: 500;
    padding: 5px 12px;
    border-radius: 5px;
    cursor: pointer;
    transition: all 0.12s;
  }

  .segment:hover {
    color: #d0d0e8;
  }

  .segment-active {
    background: rgba(99, 102, 241, 0.2);
    color: #c7d2fe;
  }

  footer {
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid #1e1e36;
  }

  .footer-hint {
    font-size: 11px;
    color: #5a5a7a;
  }
</style>
