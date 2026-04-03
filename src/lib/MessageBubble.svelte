<script lang="ts">
  import type { ConversationMessage } from "./types";

  let { message, searchQuery = "" }: { message: ConversationMessage; searchQuery?: string } = $props();

  function highlightSearch(html: string, query: string): string {
    if (!query) return html;
    const escapedQuery = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const regex = new RegExp(escapedQuery, "gi");
    // Split by HTML tags, only highlight in text segments
    return html.replace(/(<[^>]*>)|([^<]+)/g, (segment, tag, text) => {
      if (tag) return tag;
      return text.replace(regex, (match: string) => `<mark class="search-mark">${match}</mark>`);
    });
  }

  function formatTime(isoDate: string): string {
    if (!isoDate) return "";
    return new Date(isoDate).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }

  function renderMarkdown(text: string): string {
    let html = escapeHtml(text);

    // Code blocks
    html = html.replace(
      /```(\w*)\n([\s\S]*?)```/g,
      '<pre><code class="language-$1">$2</code></pre>'
    );

    // Inline code
    html = html.replace(/`([^`]+)`/g, "<code>$1</code>");

    // Bold
    html = html.replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>");

    // Italic
    html = html.replace(/\*(.+?)\*/g, "<em>$1</em>");

    // Headers
    html = html.replace(/^### (.+)$/gm, "<h3>$1</h3>");
    html = html.replace(/^## (.+)$/gm, "<h2>$1</h2>");
    html = html.replace(/^# (.+)$/gm, "<h1>$1</h1>");

    // Tables
    html = html.replace(
      /((?:^\|.+\|$\n?)+)/gm,
      (tableBlock: string) => {
        const rows = tableBlock.trim().split("\n").filter((row: string) => row.trim());
        if (rows.length < 2) return tableBlock;
        let tableHtml = "<table>";
        let inBody = false;
        rows.forEach((row: string, rowIndex: number) => {
          // Skip separator rows (e.g. |---|--------|-------|)
          if (/^\|[\s\-:|]+$/.test(row.trim())) return;
          const cells = row.split("|").slice(1, -1).map((cell: string) => cell.trim());
          if (rowIndex === 0) {
            tableHtml += "<thead><tr>" + cells.map((cell: string) => `<th>${cell}</th>`).join("") + "</tr></thead>";
          } else {
            if (!inBody) { tableHtml += "<tbody>"; inBody = true; }
            tableHtml += "<tr>" + cells.map((cell: string) => `<td>${cell}</td>`).join("") + "</tr>";
          }
        });
        if (inBody) tableHtml += "</tbody>";
        tableHtml += "</table>";
        return tableHtml;
      }
    );

    // Numbered lists
    html = html.replace(/^\d+\.\s+(.+)$/gm, "<li>$1</li>");

    // Unordered lists
    html = html.replace(/^[-*] (.+)$/gm, "<li>$1</li>");
    html = html.replace(/((?:<li>[\s\S]*?<\/li>\n?)+)/g, "<ul>$1</ul>");

    // Horizontal rules
    html = html.replace(/^---$/gm, "<hr>");

    // Paragraphs
    html = html.replace(/\n\n/g, "</p><p>");
    html = "<p>" + html + "</p>";

    // Clean up nesting issues
    html = html.replace(/<p>\s*<\/p>/g, "");
    html = html.replace(/<p>(<(?:h[123]|table|ul|hr|pre))/g, "$1");
    html = html.replace(/(<\/(?:h[123]|table|ul|hr|pre)>)<\/p>/g, "$1");

    return html;
  }

  function escapeHtml(text: string): string {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  function cleanUserText(text: string): string {
    return text
      .replace(/['"`]?\/[\w\-./]+['"`]?\s*/g, "")
      .replace(/\s+/g, " ")
      .trim() || text;
  }

  let displayText = $derived(
    message.role === "user" ? cleanUserText(message.text) : message.text
  );

  let copied = $state(false);

  async function copyText() {
    await navigator.clipboard.writeText(message.text);
    copied = true;
    setTimeout(() => { copied = false; }, 1500);
  }
</script>

{#if message.role === "compaction"}
  <div class="compaction-row">
    <div class="compaction-header">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 6h16M4 12h16M4 18h7"/><path d="M19 15l-3 3 3 3"/></svg>
      <span class="compaction-label">Context Compacted</span>
      <span class="timestamp">{formatTime(message.timestamp)}</span>
    </div>
    <details class="compaction-details">
      <summary>View compaction summary</summary>
      <div class="compaction-content">
        {@html renderMarkdown(message.text)}
      </div>
    </details>
  </div>
{:else if message.role === "user"}
  <div class="user-row">
    <div class="user-meta">
      <span class="timestamp">{formatTime(message.timestamp)}</span>
      <span class="role-tag">You</span>
    </div>
    <div class="user-bubble">
      {#if searchQuery}
        <p>{@html highlightSearch(escapeHtml(displayText), searchQuery)}</p>
      {:else}
        <p>{displayText}</p>
      {/if}
      <button class="copy-btn" class:copied onclick={copyText} title="Copy message">
        {#if copied}
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M20 6L9 17l-5-5"/></svg>
        {:else}
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
        {/if}
      </button>
    </div>
  </div>
{:else}
  <div class="assistant-row">
    <div class="assistant-header">
      <span class="claude-icon">C</span>
      <span class="claude-label">Claude</span>
      <span class="timestamp">{formatTime(message.timestamp)}</span>
    </div>
    <div class="assistant-content">
      {@html searchQuery ? highlightSearch(renderMarkdown(message.text), searchQuery) : renderMarkdown(message.text)}
    </div>
  </div>
{/if}

<style>
  /* ── Search highlight ── */

  :global(mark.search-mark) {
    background: #f59e0b;
    color: #12121e;
    padding: 1px 2px;
    border-radius: 2px;
  }

  /* ── Compaction messages: collapsible divider ── */

  .compaction-row {
    margin: 28px 0;
    border: 1px dashed #3a3a5a;
    border-radius: 8px;
    padding: 12px 16px;
    background: #1a1a2e;
  }

  .compaction-header {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #f59e0b;
  }

  .compaction-label {
    font-size: 13px;
    font-weight: 600;
  }

  .compaction-header .timestamp {
    font-size: 11px;
    color: #5a5a7a;
    margin-left: auto;
  }

  .compaction-details {
    margin-top: 8px;
  }

  .compaction-details summary {
    font-size: 12px;
    color: #7a7a9a;
    cursor: pointer;
    user-select: none;
  }

  .compaction-details summary:hover {
    color: #a0a0c0;
  }

  .compaction-content {
    margin-top: 12px;
    padding: 12px 16px;
    background: #12121e;
    border-radius: 6px;
    color: #b0b0c8;
    font-size: 13px;
    line-height: 1.6;
    max-height: 400px;
    overflow-y: auto;
  }

  .compaction-content :global(p) {
    margin: 0 0 8px 0;
  }

  .compaction-content :global(p:last-child) {
    margin-bottom: 0;
  }

  /* ── User messages: right-aligned chat bubble ── */

  .user-row {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    margin: 20px 0;
  }

  .user-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }

  .user-meta .timestamp {
    font-size: 11px;
    color: #5a5a7a;
  }

  .role-tag {
    font-size: 11px;
    font-weight: 600;
    color: #818cf8;
  }

  .user-bubble {
    background: #1e293b;
    border-radius: 12px 12px 4px 12px;
    padding: 10px 16px;
    max-width: 70%;
    color: #d0d0e8;
    font-size: 14px;
    line-height: 1.5;
  }

  .user-bubble {
    position: relative;
    padding-right: 36px;
  }

  .user-bubble p {
    margin: 0;
  }

  .copy-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    background: transparent;
    border: none;
    color: #5a6a8a;
    width: 24px;
    height: 24px;
    border-radius: 4px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity 0.15s, color 0.15s, background 0.15s;
  }

  .user-row:hover .copy-btn {
    opacity: 1;
  }

  .copy-btn:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #c0c0d8;
  }

  .copy-btn.copied {
    opacity: 1;
    color: #34d399;
  }

  /* ── Assistant messages: full-width left-aligned ── */

  .assistant-row {
    margin: 24px 0;
  }

  .assistant-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
  }

  .claude-icon {
    width: 22px;
    height: 22px;
    background: linear-gradient(135deg, #10b981, #059669);
    border-radius: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 700;
    color: white;
  }

  .claude-label {
    font-size: 13px;
    font-weight: 600;
    color: #34d399;
  }

  .assistant-header .timestamp {
    font-size: 11px;
    color: #5a5a7a;
  }

  .assistant-content {
    background: #16162a;
    border-radius: 12px;
    padding: 16px 20px;
    color: #d0d0e8;
    font-size: 14px;
    line-height: 1.7;
    border: 1px solid #1e1e36;
  }

  .assistant-content :global(p) {
    margin: 0 0 10px 0;
  }

  .assistant-content :global(p:last-child) {
    margin-bottom: 0;
  }

  .assistant-content :global(pre) {
    background: #0d0d18;
    border-radius: 8px;
    padding: 14px 16px;
    overflow-x: auto;
    margin: 10px 0;
    border: 1px solid #1e1e36;
  }

  .assistant-content :global(code) {
    font-family: "SF Mono", "Fira Code", "Cascadia Code", monospace;
    font-size: 13px;
  }

  .assistant-content :global(:not(pre) > code) {
    background: #2a2a4a;
    padding: 2px 7px;
    border-radius: 4px;
    font-size: 13px;
    color: #e0b0ff;
  }

  .assistant-content :global(h1),
  .assistant-content :global(h2),
  .assistant-content :global(h3) {
    color: #e0e0f0;
    margin: 16px 0 8px 0;
  }

  .assistant-content :global(h1) {
    font-size: 18px;
  }
  .assistant-content :global(h2) {
    font-size: 16px;
  }
  .assistant-content :global(h3) {
    font-size: 15px;
  }

  .assistant-content :global(strong) {
    color: #e0e0f0;
  }

  .assistant-content :global(ul) {
    margin: 6px 0;
    padding-left: 22px;
  }

  .assistant-content :global(li) {
    margin: 4px 0;
  }

  .assistant-content :global(table) {
    border-collapse: collapse;
    width: 100%;
    margin: 10px 0;
    font-size: 13px;
  }

  .assistant-content :global(th),
  .assistant-content :global(td) {
    border: 1px solid #2a2a4a;
    padding: 8px 12px;
    text-align: left;
  }

  .assistant-content :global(th) {
    background: #1e1e36;
    color: #e0e0f0;
    font-weight: 600;
  }

  .assistant-content :global(td) {
    background: #12121e;
  }

  .assistant-content :global(hr) {
    border: none;
    border-top: 1px solid #2a2a4a;
    margin: 14px 0;
  }
</style>
