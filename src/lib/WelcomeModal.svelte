<script lang="ts">
  import BrandMark from "./BrandMark.svelte";

  let { onDismiss }: { onDismiss: () => void } = $props();

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" || event.key === "Enter") {
      event.preventDefault();
      onDismiss();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onDismiss}>
  <div class="dialog" onclick={(event) => event.stopPropagation()}>
    <div class="brand-lockup">
      <BrandMark size={15} />
      <span>Claude Sessions</span>
    </div>
    <div class="hero-icon">
      <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>
    </div>

    <h2>Your Claude Code sessions expire in 30 days</h2>
    <p class="lead">
      Claude Code automatically deletes your session history after 30 days.
      Once a session is gone, its full transcript, images, and context are
      <strong>gone for good</strong> — even a bookmark can't bring them back.
    </p>

    <div class="points">
      <div class="point">
        <span class="point-icon point-icon-save">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1.5"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
        </span>
        <div class="point-text">
          <strong>Save what matters.</strong> Bookmark a message or hit
          <em>Save</em> on a session, and Claude Sessions keeps a full copy so it
          never expires.
        </div>
      </div>
      <div class="point">
        <span class="point-icon point-icon-lock">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
        </span>
        <div class="point-text">
          <strong>Stays on your machine.</strong> Saved sessions are copied to a
          local folder you control. Nothing is uploaded — no cloud, no account.
        </div>
      </div>
    </div>

    <button class="cta" onclick={onDismiss}>Got it — let me browse</button>
    <p class="footnote">You can change where saved sessions live in Settings.</p>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 300;
    background: var(--overlay-scrim, rgba(0, 0, 0, 0.6));
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    animation: overlay-fade 0.18s ease-out;
  }

  @keyframes overlay-fade {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .dialog {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.5);
    width: 100%;
    max-width: 460px;
    padding: 30px 30px 24px;
    text-align: center;
    animation: dialog-rise 0.2s ease-out;
  }

  @keyframes dialog-rise {
    from { opacity: 0; transform: translateY(10px); }
    to { opacity: 1; transform: translateY(0); }
  }

  .brand-lockup {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    margin-bottom: 18px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .hero-icon {
    width: 52px;
    height: 52px;
    margin: 0 auto 16px;
    border-radius: 14px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #fbbf24;
    background: rgba(245, 158, 11, 0.12);
    border: 1px solid rgba(245, 158, 11, 0.3);
  }

  h2 {
    font-size: 19px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 10px;
    line-height: 1.3;
  }

  .lead {
    font-size: 13.5px;
    line-height: 1.55;
    color: var(--text-secondary);
    margin: 0 0 20px;
  }

  .lead strong { color: var(--text-primary); }

  .points {
    display: flex;
    flex-direction: column;
    gap: 12px;
    text-align: left;
    margin-bottom: 22px;
  }

  .point {
    display: flex;
    gap: 11px;
    align-items: flex-start;
    padding: 11px 13px;
    background: var(--bg-app);
    border: 1px solid var(--border);
    border-radius: 9px;
  }

  .point-icon {
    flex-shrink: 0;
    width: 28px;
    height: 28px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .point-icon-save {
    color: #f4bf5f;
    background: rgba(245, 158, 11, 0.12);
  }

  .point-icon-lock {
    color: var(--accent-hover);
    background: rgba(99, 102, 241, 0.12);
  }

  .point-text {
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--text-secondary);
  }

  .point-text strong { color: var(--text-primary); }
  .point-text em { font-style: normal; font-weight: 600; color: var(--accent-hover); }

  .cta {
    width: 100%;
    padding: 11px;
    font-size: 14px;
    font-weight: 600;
    color: white;
    background: var(--accent);
    border: none;
    border-radius: 9px;
    cursor: pointer;
    transition: background 0.15s;
  }

  .cta:hover { background: var(--accent-hover); }

  .footnote {
    font-size: 11.5px;
    color: var(--text-faint);
    margin: 12px 0 0;
  }
</style>
