<script lang="ts">
  import BrandMark from "./BrandMark.svelte";
  import { closeDialog, modal } from "./dialog";

  let { onDismiss }: { onDismiss: () => void } = $props();
</script>

<dialog class="app-dialog welcome" style="--dialog-width: 460px" aria-labelledby="welcome-title" use:modal={onDismiss}>
  <div class="dialog-body">
    <div class="brand-lockup">
      <BrandMark size={15} />
      <span>Evertusk</span>
    </div>
    <div class="hero-icon">
      <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>
    </div>

    <h2 id="welcome-title">Everything your agents did, in one place</h2>
    <p class="lead">
      Evertusk reads the history Claude Code and Codex keep on this Mac. Find any past
      session, see what it changed and what failed, and keep copies of Claude Code
      sessions past its cleanup. <strong>Nothing leaves your Mac.</strong>
    </p>

    <div class="points">
      <div class="point">
        <span class="point-icon point-icon-save">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1.5"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
        </span>
        <div class="point-text">
          <strong>Save what matters.</strong> Bookmark a message or hit
          <em>Save</em> on a session, and Evertusk keeps a full copy so it
          never expires.
        </div>
      </div>
      <div class="point">
        <span class="point-icon point-icon-lock">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
        </span>
        <div class="point-text">
          <strong>Stays on your machine.</strong> Saved sessions are copied to a
          local folder you control. Nothing is uploaded: no cloud, no account.
        </div>
      </div>
    </div>

    <!-- svelte-ignore a11y_autofocus -->
    <button class="cta" onclick={closeDialog} autofocus>Got it, let me browse</button>
    <p class="footnote">You can change where saved sessions live in Settings. Evertusk is not affiliated with Anthropic or OpenAI.</p>
  </div>
</dialog>

<style>
  .dialog-body {
    padding: 30px 30px 24px;
    text-align: center;
  }

  .brand-lockup {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    margin-bottom: 18px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
  }

  .hero-icon {
    width: 52px;
    height: 52px;
    margin: 0 auto 16px;
    border-radius: 14px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent-text);
    background: var(--accent-soft);
    border: 1px solid var(--accent);
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
    color: var(--accent-text);
    background: var(--accent-soft);
  }

  .point-icon-lock {
    color: var(--accent-text);
    background: var(--accent-soft);
  }

  .point-text {
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--text-secondary);
  }

  .point-text strong { color: var(--text-primary); }
  .point-text em { font-style: normal; font-weight: 600; color: var(--accent-text); }

  .cta {
    width: 100%;
    padding: 11px;
    font-size: 14px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    border: none;
    border-radius: 9px;
    cursor: pointer;
    transition: background 0.15s;
  }

  .cta:hover { background: var(--accent-hover); }

  .footnote {
    font-size: 12px;
    color: var(--text-muted);
    margin: 12px 0 0;
  }
</style>
