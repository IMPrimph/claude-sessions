<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import type { ConversationMessage } from "./types";

  let {
    message,
    searchQuery = "",
    sessionId = "",
    onImageOpen,
  }: {
    message: ConversationMessage;
    searchQuery?: string;
    sessionId?: string;
    onImageOpen?: (url: string, label: string) => void;
  } = $props();

  // ── Image references ──

  type UserSegment =
    | { kind: "text"; content: string }
    | { kind: "image"; number: number; url: string | null };

  let cacheImageUrls = $state(new Map<number, string | null>());

  async function loadCachedImageUrl(imageNumber: number) {
    if (!sessionId || cacheImageUrls.has(imageNumber)) return;
    try {
      const path = await invoke<string | null>("get_image_path", {
        sessionId,
        imageNumber,
      });
      const assetUrl = path ? convertFileSrc(path) : null;
      cacheImageUrls = new Map(cacheImageUrls).set(imageNumber, assetUrl);
    } catch {
      cacheImageUrls = new Map(cacheImageUrls).set(imageNumber, null);
    }
  }

  function resolveImageUrl(imageNumber: number): string | null {
    // Prefer the inline base64 data URL from the message itself
    const inline = message.images?.find((image) => image.number === imageNumber);
    if (inline) return inline.data_url;
    // Fall back to the disk cache (~/.claude/image-cache/<session>/<N>.png)
    return cacheImageUrls.get(imageNumber) ?? null;
  }

  function parseUserSegments(text: string): UserSegment[] {
    const segments: UserSegment[] = [];
    const imageRefRegex = /\[Image\s*#(\d+)\]|\[Image:\s*source:\s*([^\]]+)\]/g;
    let lastIndex = 0;
    let match;

    while ((match = imageRefRegex.exec(text)) !== null) {
      if (match.index > lastIndex) {
        const preceding = text.slice(lastIndex, match.index);
        if (preceding.trim()) segments.push({ kind: "text", content: preceding });
      }

      if (match[1]) {
        const imageNumber = parseInt(match[1], 10);
        let url = resolveImageUrl(imageNumber);
        if (!url) loadCachedImageUrl(imageNumber);
        segments.push({ kind: "image", number: imageNumber, url });
      } else if (match[2]) {
        const directPath = match[2].trim();
        const match2 = directPath.match(/(\d+)\.(png|jpg|jpeg|gif|webp)$/i);
        const imageNumber = match2 ? parseInt(match2[1], 10) : -1;
        const assetUrl = convertFileSrc(directPath);
        segments.push({ kind: "image", number: imageNumber, url: assetUrl });
      }

      lastIndex = match.index + match[0].length;
    }

    if (lastIndex < text.length) {
      const trailing = text.slice(lastIndex);
      if (trailing.trim()) segments.push({ kind: "text", content: trailing });
    }

    return segments;
  }

  let userSegments = $derived(
    message.role === "user" ? parseUserSegments(message.text) : []
  );

  // Extra images attached to this user message that weren't referenced in text
  let extraImages = $derived.by(() => {
    if (message.role !== "user" || !message.images) return [];
    const refsInText = new Set<number>();
    for (const segment of userSegments) {
      if (segment.kind === "image") refsInText.add(segment.number);
    }
    return message.images.filter((image) => !refsInText.has(image.number));
  });

  // ── Segment types for structured assistant rendering ──

  type Segment =
    | { kind: "text"; content: string }
    | { kind: "tool"; name: string; summary: string }
    | { kind: "thinking"; content: string };

  function parseAssistantSegments(text: string): Segment[] {
    const segments: Segment[] = [];
    const markerRegex = /\{\{TOOL:([^|}]+)\|([^}]*)\}\}|\{\{THINKING_START\}\}\n?([\s\S]*?)\n?\{\{THINKING_END\}\}/g;
    let lastIndex = 0;
    let match;

    while ((match = markerRegex.exec(text)) !== null) {
      // Push preceding text
      if (match.index > lastIndex) {
        const preceding = text.slice(lastIndex, match.index).trim();
        if (preceding) segments.push({ kind: "text", content: preceding });
      }

      if (match[1] !== undefined) {
        // Tool marker
        segments.push({ kind: "tool", name: match[1], summary: match[2] });
      } else if (match[3] !== undefined) {
        // Thinking block
        segments.push({ kind: "thinking", content: match[3] });
      }

      lastIndex = match.index + match[0].length;
    }

    // Push trailing text
    if (lastIndex < text.length) {
      const trailing = text.slice(lastIndex).trim();
      if (trailing) segments.push({ kind: "text", content: trailing });
    }

    return segments;
  }

  let assistantSegments = $derived(
    message.role === "assistant" ? parseAssistantSegments(message.text) : []
  );

  // ── Tool colors ──

  function toolColor(name: string): string {
    const colors: Record<string, string> = {
      Read: "#22d3ee",
      Write: "#a78bfa",
      Edit: "#f59e0b",
      Bash: "#f97316",
      Grep: "#34d399",
      Glob: "#34d399",
      Agent: "#818cf8",
      Skill: "#ec4899",
      TaskCreate: "#6366f1",
      TaskUpdate: "#6366f1",
      TaskGet: "#6366f1",
      TaskList: "#6366f1",
    };
    return colors[name] || "#7a7a9a";
  }

  // ── Helpers ──

  function highlightSearch(html: string, query: string): string {
    if (!query) return html;
    const escapedQuery = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const regex = new RegExp(escapedQuery, "gi");
    return html.replace(/(<[^>]*>)|([^<]+)/g, (segment, tag, text) => {
      if (tag) return tag;
      return text.replace(regex, (matched: string) => `<mark class="search-mark">${matched}</mark>`);
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

  function escapeHtml(text: string): string {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  // ── Markdown rendering ──

  function renderMarkdown(text: string): string {
    let html = escapeHtml(text);

    // Code blocks — wrapper with language label and copy button. The copy button
    // finds its sibling <code> via DOM traversal (handleContentClick) so there
    // are no IDs to collide across messages or re-renders.
    html = html.replace(
      /```(\w*)\n([\s\S]*?)```/g,
      (_match: string, lang: string, code: string) => {
        const langLabel = lang || "code";
        const highlighted = lang ? highlightSyntax(code, lang) : code;
        return `<div class="code-block-wrapper"><div class="code-block-header"><span class="code-lang">${langLabel}</span><button type="button" class="code-copy-btn" title="Copy code">Copy</button></div><pre><code class="language-${lang}">${highlighted}</code></pre></div>`;
      }
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
    html = html.replace(/<p>(<(?:h[123]|table|ul|hr|pre|div))/g, "$1");
    html = html.replace(/(<\/(?:h[123]|table|ul|hr|pre|div)>)<\/p>/g, "$1");

    return html;
  }

  // ── Basic syntax highlighting ──

  function highlightSyntax(code: string, lang: string): string {
    // Comments
    if (["js", "ts", "javascript", "typescript", "jsx", "tsx", "rust", "go", "java", "c", "cpp", "swift"].includes(lang)) {
      code = code.replace(/(\/\/[^\n]*)/g, '<span class="syn-comment">$1</span>');
      code = code.replace(/(\/\*[\s\S]*?\*\/)/g, '<span class="syn-comment">$1</span>');
    } else if (["py", "python", "ruby", "bash", "sh", "zsh", "yaml", "yml"].includes(lang)) {
      code = code.replace(/(#[^\n]*)/g, '<span class="syn-comment">$1</span>');
    }

    // Strings (double and single quoted) — skip if already inside a span
    code = code.replace(/(?<!<span[^>]*>.*?)(&quot;[^&]*?&quot;|&#x27;[^&]*?&#x27;|&amp;quot;.*?&amp;quot;)/g, '<span class="syn-string">$1</span>');
    // Backtick template strings for JS/TS
    if (["js", "ts", "javascript", "typescript", "jsx", "tsx"].includes(lang)) {
      code = code.replace(/(`)([^`]*?)(`)/g, '<span class="syn-string">$1$2$3</span>');
    }

    // Keywords per language family
    let keywords: string[] = [];
    if (["js", "ts", "javascript", "typescript", "jsx", "tsx"].includes(lang)) {
      keywords = ["const", "let", "var", "function", "return", "if", "else", "for", "while", "import", "export", "from", "class", "extends", "new", "async", "await", "try", "catch", "throw", "typeof", "interface", "type", "enum", "default", "switch", "case", "break", "continue", "true", "false", "null", "undefined", "this", "super"];
    } else if (["py", "python"].includes(lang)) {
      keywords = ["def", "class", "return", "if", "elif", "else", "for", "while", "import", "from", "as", "try", "except", "raise", "with", "yield", "lambda", "True", "False", "None", "in", "not", "and", "or", "is", "pass", "break", "continue", "self", "async", "await"];
    } else if (["rust"].includes(lang)) {
      keywords = ["fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod", "if", "else", "for", "while", "loop", "match", "return", "self", "super", "crate", "where", "async", "await", "move", "true", "false", "Some", "None", "Ok", "Err"];
    } else if (["bash", "sh", "zsh"].includes(lang)) {
      keywords = ["if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "function", "return", "export", "local", "echo", "exit", "cd", "ls", "rm", "cp", "mv", "mkdir", "cat", "grep", "sed", "awk"];
    }

    if (keywords.length > 0) {
      // Match either an existing <span>...</span> (to skip) or a bare keyword to wrap.
      const combined = new RegExp(
        `(<span[^>]*>[\\s\\S]*?<\\/span>)|\\b(${keywords.join("|")})\\b`,
        "g"
      );
      code = code.replace(combined, (fullMatch, insideSpan, keyword) => {
        if (insideSpan) return insideSpan;
        if (keyword) return `<span class="syn-keyword">${keyword}</span>`;
        return fullMatch;
      });
    }

    return code;
  }

  // ── Code copy handler (delegated click) ──

  function handleContentClick(event: MouseEvent) {
    const target = event.target as HTMLElement;
    if (!target.classList.contains("code-copy-btn")) return;

    const wrapper = target.closest(".code-block-wrapper");
    const codeElement = wrapper?.querySelector("pre code");
    if (!codeElement) return;

    navigator.clipboard.writeText(codeElement.textContent || "");
    target.textContent = "Copied!";
    setTimeout(() => {
      target.textContent = "Copy";
    }, 1500);
  }

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
      {#each userSegments as segment}
        {#if segment.kind === "text"}
          {#if searchQuery}
            <p>{@html highlightSearch(escapeHtml(segment.content), searchQuery)}</p>
          {:else}
            <p>{segment.content}</p>
          {/if}
        {:else if segment.kind === "image"}
          {@const resolvedUrl = segment.url ?? resolveImageUrl(segment.number)}
          {#if resolvedUrl}
            <button
              type="button"
              class="user-image-link"
              title="Open image #{segment.number}"
              onclick={() => onImageOpen?.(resolvedUrl, `Image #${segment.number}`)}
            >
              <img src={resolvedUrl} alt="Image #{segment.number}" class="user-image" loading="lazy" />
            </button>
          {:else}
            <span class="image-missing" title="Image not found in cache">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/></svg>
              Image #{segment.number}
            </span>
          {/if}
        {/if}
      {/each}
      {#each extraImages as extra}
        <button
          type="button"
          class="user-image-link"
          title="Image #{extra.number}"
          onclick={() => onImageOpen?.(extra.data_url, `Image #${extra.number}`)}
        >
          <img src={extra.data_url} alt="Image #{extra.number}" class="user-image" loading="lazy" />
        </button>
      {/each}
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
  <!-- Assistant message: structured segments -->
  <div class="assistant-row">
    <div class="assistant-header">
      <span class="claude-icon">C</span>
      <span class="claude-label">Claude</span>
      <span class="timestamp">{formatTime(message.timestamp)}</span>
    </div>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="assistant-content" onclick={handleContentClick}>
      {#each assistantSegments as segment}
        {#if segment.kind === "text"}
          <div class="segment-text">
            {@html searchQuery ? highlightSearch(renderMarkdown(segment.content), searchQuery) : renderMarkdown(segment.content)}
          </div>
        {:else if segment.kind === "tool"}
          <div class="tool-pill" style="--tool-color: {toolColor(segment.name)}">
            <span class="tool-name">{segment.name}</span>
            {#if segment.summary}
              <span class="tool-summary">{segment.summary}</span>
            {/if}
          </div>
        {:else if segment.kind === "thinking"}
          <details class="thinking-block">
            <summary>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/></svg>
              Thinking...
            </summary>
            <div class="thinking-content">
              {@html renderMarkdown(segment.content)}
            </div>
          </details>
        {/if}
      {/each}
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

  /* ── Compaction messages ── */

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

  .compaction-content :global(p) { margin: 0 0 8px 0; }
  .compaction-content :global(p:last-child) { margin-bottom: 0; }

  /* ── User messages ── */

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

  .user-meta .timestamp { font-size: 11px; color: #5a5a7a; }

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
    position: relative;
    padding-right: 36px;
    overflow-wrap: anywhere;
    word-break: break-word;
    white-space: pre-wrap;
  }

  .user-bubble p {
    margin: 0;
    overflow-wrap: anywhere;
    word-break: break-word;
  }
  .user-bubble p + p { margin-top: 6px; }

  .user-image-link {
    display: inline-block;
    margin: 6px 6px 0 0;
    padding: 0;
    background: transparent;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.08);
    cursor: pointer;
    transition: border-color 0.15s, transform 0.15s;
  }

  .user-image-link:hover {
    border-color: #6366f1;
    transform: scale(1.02);
  }

  .user-image {
    display: block;
    max-width: 180px;
    max-height: 180px;
    object-fit: cover;
  }

  .image-missing {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin: 6px 6px 0 0;
    padding: 4px 8px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px dashed rgba(255, 255, 255, 0.12);
    border-radius: 6px;
    font-size: 11px;
    color: #8a8aaa;
  }

  .image-missing svg { color: #6a6a8a; }

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

  .user-row:hover .copy-btn { opacity: 1; }
  .copy-btn:hover { background: rgba(255, 255, 255, 0.1); color: #c0c0d8; }
  .copy-btn.copied { opacity: 1; color: #34d399; }

  /* ── Assistant messages ── */

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

  .claude-label { font-size: 13px; font-weight: 600; color: #34d399; }
  .assistant-header .timestamp { font-size: 11px; color: #5a5a7a; }

  .assistant-content {
    background: #16162a;
    border-radius: 12px;
    padding: 16px 20px;
    color: #d0d0e8;
    font-size: 14px;
    line-height: 1.7;
    border: 1px solid #1e1e36;
  }

  /* ── Segment: text ── */

  .segment-text :global(p) { margin: 0 0 10px 0; }
  .segment-text :global(p:last-child) { margin-bottom: 0; }
  .segment-text + .segment-text { margin-top: 6px; }

  /* ── Segment: tool pill ── */

  .tool-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin: 4px 0;
    padding: 4px 10px;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: 6px;
    font-size: 12px;
    line-height: 1.4;
    max-width: 100%;
  }

  .tool-name {
    font-weight: 600;
    color: var(--tool-color);
    white-space: nowrap;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 11px;
    background: rgba(255, 255, 255, 0.05);
    padding: 1px 6px;
    border-radius: 3px;
  }

  .tool-summary {
    color: #8a8aaa;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 11px;
  }

  /* ── Segment: thinking block ── */

  .thinking-block {
    margin: 8px 0;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    background: #12121e;
  }

  .thinking-block summary {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    font-size: 12px;
    font-weight: 500;
    color: #7a7a9a;
    cursor: pointer;
    user-select: none;
  }

  .thinking-block summary:hover { color: #a0a0c0; }

  .thinking-block summary svg {
    color: #6366f1;
    flex-shrink: 0;
  }

  .thinking-content {
    padding: 0 12px 12px;
    font-size: 13px;
    color: #9a9ab8;
    line-height: 1.6;
    max-height: 400px;
    overflow-y: auto;
    border-top: 1px solid #2a2a4a;
    padding-top: 10px;
  }

  .thinking-content :global(p) { margin: 0 0 8px 0; }
  .thinking-content :global(p:last-child) { margin-bottom: 0; }

  /* ── Code blocks with header ── */

  .assistant-content :global(.code-block-wrapper) {
    margin: 10px 0;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid #1e1e36;
  }

  .assistant-content :global(.code-block-header) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 12px;
    background: #1a1a2e;
    border-bottom: 1px solid #1e1e36;
  }

  .assistant-content :global(.code-lang) {
    font-size: 11px;
    color: #5a5a7a;
    font-family: "SF Mono", "Fira Code", monospace;
    text-transform: lowercase;
  }

  .assistant-content :global(.code-copy-btn) {
    font-size: 11px;
    color: #5a5a7a;
    background: transparent;
    border: 1px solid #2a2a4a;
    border-radius: 4px;
    padding: 2px 8px;
    cursor: pointer;
    font-family: inherit;
    transition: all 0.15s;
  }

  .assistant-content :global(.code-copy-btn:hover) {
    color: #c0c0d8;
    border-color: #4a4a6a;
    background: rgba(255, 255, 255, 0.05);
  }

  .assistant-content :global(.code-block-wrapper pre) {
    margin: 0;
    border: none;
    border-radius: 0;
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

  /* ── Syntax highlighting ── */

  .assistant-content :global(.syn-keyword) { color: #c792ea; }
  .assistant-content :global(.syn-string) { color: #c3e88d; }
  .assistant-content :global(.syn-comment) { color: #546e7a; font-style: italic; }

  /* ── Standard markdown elements ── */

  .assistant-content :global(h1),
  .assistant-content :global(h2),
  .assistant-content :global(h3) { color: #e0e0f0; margin: 16px 0 8px 0; }

  .assistant-content :global(h1) { font-size: 18px; }
  .assistant-content :global(h2) { font-size: 16px; }
  .assistant-content :global(h3) { font-size: 15px; }
  .assistant-content :global(strong) { color: #e0e0f0; }
  .assistant-content :global(ul) { margin: 6px 0; padding-left: 22px; }
  .assistant-content :global(li) { margin: 4px 0; }

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

  .assistant-content :global(th) { background: #1e1e36; color: #e0e0f0; font-weight: 600; }
  .assistant-content :global(td) { background: #12121e; }
  .assistant-content :global(hr) { border: none; border-top: 1px solid #2a2a4a; margin: 14px 0; }
</style>
