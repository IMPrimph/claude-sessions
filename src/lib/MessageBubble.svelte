<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import type { ConversationMessage, SessionInfo, ToolResultPayload } from "./types";
  import { prettyToolName } from "./format";
  import { copyToClipboard } from "./clipboard";
  import { isBookmarked, toggleBookmark, makeBookmarkId } from "./bookmarks.svelte";

  let {
    message,
    searchQuery = "",
    sessionId = "",
    onImageOpen,
    onAgentOpen,
    toolResults,
    bookmarkSession = null,
  }: {
    message: ConversationMessage;
    searchQuery?: string;
    sessionId?: string;
    onImageOpen?: (url: string, label: string) => void;
    onAgentOpen?: (agentId: string, description: string) => void;
    toolResults?: Record<string, ToolResultPayload>;
    // When provided, a bookmark (star) button is shown on user/assistant messages.
    // Subagent transcripts pass nothing, so they aren't bookmarkable in v1.
    bookmarkSession?: SessionInfo | null;
  } = $props();

  // Track which tool pills are expanded by tool_use_id (per-message state)
  let expandedTools = $state(new Set<string>());

  function toggleToolResult(toolUseId: string) {
    const next = new Set(expandedTools);
    if (next.has(toolUseId)) next.delete(toolUseId);
    else next.add(toolUseId);
    expandedTools = next;
  }

  // Lazy-load persisted-output sidecars on first expand for that pill
  let persistedContents = $state<Record<string, string>>({});
  let persistedLoading = $state<Record<string, boolean>>({});
  let persistedErrors = $state<Record<string, string>>({});

  async function loadPersistedFor(toolUseId: string, persistedPath: string) {
    if (persistedContents[toolUseId] || persistedLoading[toolUseId]) return;
    persistedLoading = { ...persistedLoading, [toolUseId]: true };
    try {
      const content = await invoke<string>("read_tool_output_file", {
        path: persistedPath,
      });
      persistedContents = { ...persistedContents, [toolUseId]: content };
    } catch (loadError) {
      persistedErrors = { ...persistedErrors, [toolUseId]: String(loadError) };
    } finally {
      persistedLoading = { ...persistedLoading, [toolUseId]: false };
    }
  }

  // Strip the <persisted-output>…</persisted-output> wrapper so we show just the preview
  // alongside a "Load full output" button. Keeps display tidy.
  function previewContent(content: string): string {
    const start = content.indexOf("<persisted-output>");
    const end = content.indexOf("</persisted-output>");
    if (start === -1 || end === -1) return content;
    const before = content.slice(0, start).trim();
    const inside = content.slice(start + "<persisted-output>".length, end);
    const previewMatch = inside.match(/Preview \(first[^)]*\):\s*\n([\s\S]*)/);
    const preview = previewMatch ? previewMatch[1].trim() : inside.trim();
    const after = content.slice(end + "</persisted-output>".length).trim();
    return [before, preview, after].filter(Boolean).join("\n\n");
  }

  // Truncate displayed text for very large inline results (still shows everything in
  // a separate full-screen viewer if we need one later).
  const INLINE_DISPLAY_LIMIT = 4000;
  function truncateForInline(text: string): { text: string; truncated: boolean } {
    if (text.length <= INLINE_DISPLAY_LIMIT) return { text, truncated: false };
    return {
      text: text.slice(0, INLINE_DISPLAY_LIMIT) + "\n\n[…truncated for display]",
      truncated: true,
    };
  }

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
    | { kind: "tool"; name: string; summary: string; toolUseId?: string; agentId?: string }
    | { kind: "thinking"; content: string };

  function parseAssistantSegments(text: string): Segment[] {
    const segments: Segment[] = [];
    // Marker format: {{TOOL:name|summary[|toolUseId[|agentId]]}} or {{THINKING_START}}...{{THINKING_END}}.
    // Third field = tool_use_id (most calls). Fourth field = agentId (Agent calls only).
    const markerRegex = /\{\{TOOL:([^|}]+)\|([^|}]*)(?:\|([^|}]*))?(?:\|([^}]*))?\}\}|\{\{THINKING_START\}\}\n?([\s\S]*?)\n?\{\{THINKING_END\}\}/g;
    let lastIndex = 0;
    let match;

    while ((match = markerRegex.exec(text)) !== null) {
      // Push preceding text
      if (match.index > lastIndex) {
        const preceding = text.slice(lastIndex, match.index).trim();
        if (preceding) segments.push({ kind: "text", content: preceding });
      }

      if (match[1] !== undefined) {
        segments.push({
          kind: "tool",
          name: match[1],
          summary: match[2],
          toolUseId: match[3] || undefined,
          agentId: match[4] || undefined,
        });
      } else if (match[5] !== undefined) {
        // Thinking block
        segments.push({ kind: "thinking", content: match[5] });
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
      MultiEdit: "#f59e0b",
      NotebookEdit: "#f59e0b",
      Bash: "#f97316",
      Grep: "#34d399",
      Glob: "#34d399",
      Agent: "#818cf8",
      Skill: "#ec4899",
      Workflow: "#f472b6",
      TaskCreate: "#6366f1",
      TaskUpdate: "#6366f1",
      TaskGet: "#6366f1",
      TaskList: "#6366f1",
      TaskStop: "#6366f1",
      WebSearch: "#38bdf8",
      WebFetch: "#38bdf8",
      ToolSearch: "#2dd4bf",
      AskUserQuestion: "#fbbf24",
      Monitor: "#fb923c",
      ScheduleWakeup: "#a3e635",
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

    copyToClipboard(codeElement.textContent || "");
    target.textContent = "Copied!";
    setTimeout(() => {
      target.textContent = "Copy";
    }, 1500);
  }

  // Clean, copyable text. For assistant messages, strip internal markers so the
  // clipboard gets clean prose: tool calls become bracketed labels, thinking dropped.
  function buildCopyText(): string {
    if (message.role === "assistant") {
      return assistantSegments
        .map((segment) => {
          if (segment.kind === "text") return segment.content;
          if (segment.kind === "tool") {
            const name = prettyToolName(segment.name);
            return segment.summary ? `[${name}: ${segment.summary}]` : `[${name}]`;
          }
          return "";
        })
        .filter(Boolean)
        .join("\n\n");
    }
    return message.text;
  }

  let copied = $state(false);

  async function copyText() {
    await copyToClipboard(buildCopyText());
    copied = true;
    setTimeout(() => { copied = false; }, 1500);
  }

  // ── Bookmarks ──
  let bookmarkId = $derived(
    bookmarkSession && message.role !== "compaction"
      ? makeBookmarkId(bookmarkSession.session_id, message.timestamp, message.text)
      : ""
  );
  let bookmarked = $derived(bookmarkId !== "" && isBookmarked(bookmarkId));

  function toggleBookmarkForMessage() {
    if (!bookmarkSession || bookmarkId === "") return;
    const cleanText = buildCopyText();
    toggleBookmark({
      id: bookmarkId,
      role: message.role === "assistant" ? "assistant" : "user",
      text: cleanText,
      preview: cleanText.replace(/\s+/g, " ").trim().slice(0, 160),
      project_path: bookmarkSession.project_path,
      project_name: bookmarkSession.project_name,
      session_id: bookmarkSession.session_id,
      jsonl_path: bookmarkSession.jsonl_path,
      timestamp: message.timestamp,
      created_at: Date.now(),
    });
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
    </div>
    <div class="user-actions">
      {#if bookmarkSession}
        <button class="copy-btn bookmark-btn" class:bookmarked onclick={toggleBookmarkForMessage} title={bookmarked ? "Remove bookmark" : "Save for later"} aria-label="Bookmark message">
          {#if bookmarked}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
          {:else}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
          {/if}
        </button>
      {/if}
      <button class="copy-btn" class:copied onclick={copyText} title="Copy message">
        {#if copied}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M20 6L9 17l-5-5"/></svg>
        {:else}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
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
          {@const toolResult = segment.toolUseId ? toolResults?.[segment.toolUseId] : undefined}
          {@const isExpanded = segment.toolUseId ? expandedTools.has(segment.toolUseId) : false}
          {#if segment.agentId && onAgentOpen}
            <button
              type="button"
              class="tool-pill tool-pill-clickable"
              style="--tool-color: {toolColor(segment.name)}"
              onclick={() => onAgentOpen?.(segment.agentId!, segment.summary)}
              title="Open subagent transcript"
            >
              <span class="tool-name">{prettyToolName(segment.name)}</span>
              {#if segment.summary}
                <span class="tool-summary">{segment.summary}</span>
              {/if}
              <svg class="tool-chevron" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M9 18l6-6-6-6"/></svg>
            </button>
          {:else if segment.toolUseId && toolResult}
            <button
              type="button"
              class="tool-pill tool-pill-clickable"
              class:tool-pill-error={toolResult.is_error}
              class:tool-pill-expanded={isExpanded}
              style="--tool-color: {toolColor(segment.name)}"
              onclick={() => toggleToolResult(segment.toolUseId!)}
              title={isExpanded ? "Collapse output" : "Show output"}
            >
              <span class="tool-name">{prettyToolName(segment.name)}</span>
              {#if segment.summary}
                <span class="tool-summary">{segment.summary}</span>
              {/if}
              {#if toolResult.is_error}
                <span class="tool-error-tag">error</span>
              {/if}
              <svg class="tool-chevron tool-chevron-toggle" class:rotated={isExpanded} width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="m6 9 6 6 6-6"/></svg>
            </button>
            {#if isExpanded}
              {@const visible = previewContent(toolResult.content)}
              {@const truncated = truncateForInline(visible)}
              <div class="tool-result-panel" class:tool-result-error={toolResult.is_error}>
                <pre class="tool-result-content">{truncated.text}</pre>
                {#if toolResult.persisted_path}
                  <div class="tool-result-actions">
                    {#if persistedContents[segment.toolUseId]}
                      <details class="persisted-block">
                        <summary>Full output ({persistedContents[segment.toolUseId].length.toLocaleString()} chars)</summary>
                        <pre class="tool-result-content persisted-content">{persistedContents[segment.toolUseId]}</pre>
                      </details>
                    {:else if persistedLoading[segment.toolUseId]}
                      <span class="loading-text">Loading full output...</span>
                    {:else if persistedErrors[segment.toolUseId]}
                      <span class="error-text">Failed to load: {persistedErrors[segment.toolUseId]}</span>
                    {:else}
                      <button class="tool-load-btn" onclick={() => loadPersistedFor(segment.toolUseId!, toolResult.persisted_path!)}>
                        Load full output
                      </button>
                    {/if}
                  </div>
                {/if}
              </div>
            {/if}
          {:else}
            <div class="tool-pill" style="--tool-color: {toolColor(segment.name)}">
              <span class="tool-name">{prettyToolName(segment.name)}</span>
              {#if segment.summary}
                <span class="tool-summary">{segment.summary}</span>
              {/if}
            </div>
          {/if}
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
    <div class="assistant-actions">
      {#if bookmarkSession}
        <button class="copy-btn bookmark-btn" class:bookmarked onclick={toggleBookmarkForMessage} title={bookmarked ? "Remove bookmark" : "Save for later"} aria-label="Bookmark message">
          {#if bookmarked}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
          {:else}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
          {/if}
        </button>
      {/if}
      <button class="copy-btn" class:copied onclick={copyText} title="Copy message">
        {#if copied}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M20 6L9 17l-5-5"/></svg>
        {:else}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
        {/if}
      </button>
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

  .user-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 6px;
    opacity: 0;
    transition: opacity 0.15s;
  }

  .user-row:hover .user-actions { opacity: 1; }
  .user-actions:has(.copied) { opacity: 1; }
  /* Keep the bar visible when saved, so the filled star is always shown. */
  .user-actions:has(.bookmarked) { opacity: 1; }

  .assistant-actions {
    display: flex;
    justify-content: flex-start;
    margin-top: 6px;
    padding-left: 2px;
    opacity: 0;
    transition: opacity 0.15s;
  }

  .assistant-row:hover .assistant-actions { opacity: 1; }
  .assistant-actions:has(.copied) { opacity: 1; }
  .assistant-actions:has(.bookmarked) { opacity: 1; }

  .copy-btn {
    background: transparent;
    border: none;
    color: #6a6a8a;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.15s, background 0.15s;
  }

  .copy-btn:hover { background: rgba(255, 255, 255, 0.06); color: #c0c0d8; }
  .copy-btn.copied { color: #34d399; }
  .bookmark-btn:hover { color: #fbbf24; }
  .bookmark-btn.bookmarked { color: #f59e0b; }

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

  .tool-pill-clickable {
    cursor: pointer;
    color: inherit;
    font-family: inherit;
    transition: background 0.15s, border-color 0.15s;
  }

  .tool-pill-clickable:hover {
    background: rgba(99, 102, 241, 0.1);
    border-color: rgba(99, 102, 241, 0.3);
  }

  .tool-pill-expanded {
    background: rgba(99, 102, 241, 0.08);
    border-color: rgba(99, 102, 241, 0.25);
  }

  .tool-pill-error {
    background: rgba(239, 68, 68, 0.06);
    border-color: rgba(239, 68, 68, 0.25);
  }

  .tool-pill-error:hover {
    background: rgba(239, 68, 68, 0.1);
    border-color: rgba(239, 68, 68, 0.4);
  }

  .tool-error-tag {
    font-size: 9px;
    color: #f87171;
    background: rgba(239, 68, 68, 0.15);
    padding: 1px 6px;
    border-radius: 3px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 600;
  }

  .tool-chevron {
    color: #6a6a8a;
    flex-shrink: 0;
  }

  .tool-pill-clickable:hover .tool-chevron {
    color: #a5b4fc;
  }

  .tool-chevron-toggle {
    transition: transform 0.15s;
  }

  .tool-chevron-toggle.rotated {
    transform: rotate(180deg);
  }

  .tool-result-panel {
    margin: 0 0 8px 0;
    padding: 10px 14px;
    background: #0d0d18;
    border: 1px solid #1e1e36;
    border-radius: 8px;
    border-left: 3px solid var(--tool-color, #6366f1);
  }

  .tool-result-panel.tool-result-error {
    border-left-color: #ef4444;
    background: rgba(127, 29, 29, 0.08);
  }

  .tool-result-content {
    margin: 0;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    color: #b0b0c8;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 360px;
    overflow-y: auto;
  }

  .persisted-content {
    max-height: 600px;
    margin-top: 8px;
  }

  .tool-result-actions {
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid #1e1e36;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .tool-load-btn {
    background: #1e1e36;
    border: 1px solid #2a2a4a;
    color: #a5b4fc;
    font-size: 11px;
    font-weight: 500;
    padding: 5px 12px;
    border-radius: 5px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .tool-load-btn:hover {
    background: #2a2a4a;
    border-color: #3a3a5a;
  }

  .loading-text {
    font-size: 11px;
    color: #7a7a9a;
  }

  .error-text {
    font-size: 11px;
    color: #f87171;
  }

  .persisted-block summary {
    font-size: 11px;
    color: #a0a0c0;
    cursor: pointer;
    user-select: none;
    padding: 2px 0;
  }

  .persisted-block summary:hover {
    color: #c0c0d8;
  }

  .tool-result-content::-webkit-scrollbar {
    width: 6px;
  }

  .tool-result-content::-webkit-scrollbar-thumb {
    background: #2a2a4a;
    border-radius: 3px;
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
