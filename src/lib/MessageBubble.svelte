<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import type { ConversationMessage, SessionInfo, ToolResultPayload, AnsweredQuestion, SessionArtifact } from "./types";
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
    questions,
    artifacts,
    bookmarkSession = null,
  }: {
    message: ConversationMessage;
    searchQuery?: string;
    sessionId?: string;
    onImageOpen?: (url: string, label: string) => void;
    onAgentOpen?: (agentId: string, description: string) => void;
    toolResults?: Record<string, ToolResultPayload>;
    // AskUserQuestion Q&A keyed by tool_use_id — renders inline as the chosen answer.
    questions?: Record<string, AnsweredQuestion[]>;
    // Published Artifacts keyed by tool_use_id — renders a card with a copy-link button.
    artifacts?: Record<string, SessionArtifact>;
    // When provided, a bookmark (star) button is shown on user/assistant messages.
    // Subagent transcripts pass nothing, so they aren't bookmarkable in v1.
    bookmarkSession?: SessionInfo | null;
  } = $props();

  // Track which tool pills are expanded by tool_use_id (per-message state)
  let expandedTools = $state(new Set<string>());

  // Copy-link state for artifact cards — tracks the last-copied url so the button
  // can briefly show "Copied".
  let copiedArtifactUrl = $state<string | null>(null);
  async function copyArtifactLink(url: string) {
    await copyToClipboard(url);
    copiedArtifactUrl = url;
    setTimeout(() => {
      if (copiedArtifactUrl === url) copiedArtifactUrl = null;
    }, 1500);
  }

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

  // ── Agent-notification card helpers ──

  // 122641 → "122.6k", 940 → "940"
  function formatTokens(count: number): string {
    if (count >= 1000) return `${(count / 1000).toFixed(1)}k`;
    return String(count);
  }

  // 267945 → "4m 28s", 8200 → "8.2s"
  function formatDuration(milliseconds: number): string {
    const totalSeconds = Math.round(milliseconds / 1000);
    if (totalSeconds < 60) return `${(milliseconds / 1000).toFixed(1)}s`;
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}m ${seconds}s`;
  }

  // Strip the framing verbs Claude Code wraps around the agent name so the card
  // header reads as the task itself: `Agent "Map domain model" finished` → `Map
  // domain model`. Falls back to the raw summary when no quoted name is present.
  function notificationTitle(summary: string): string {
    const quoted = summary.match(/"([^"]+)"/);
    return quoted ? quoted[1] : summary;
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
    const wasBookmarked = bookmarked;
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

    // Adding a bookmark archives the whole session so it survives Claude Code's
    // 30-day cleanup and the bookmark stays openable. Fire-and-forget.
    if (!wasBookmarked) {
      invoke("archive_session", {
        jsonlPath: bookmarkSession.jsonl_path,
        sessionId: bookmarkSession.session_id,
        projectPath: bookmarkSession.project_path,
        projectName: bookmarkSession.project_name,
        title:
          bookmarkSession.custom_title ||
          bookmarkSession.summary ||
          bookmarkSession.ai_title ||
          null,
      }).catch((archiveError) => {
        console.error("Archive on bookmark failed:", archiveError);
      });
    }
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
  <div class="user-row" class:mid-turn={message.mid_turn}>
    <div class="user-meta">
      {#if message.mid_turn}
        <span class="interject-badge" title="Sent while Claude was still responding — you pressed Esc and typed this mid-turn">
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="9 10 4 15 9 20"/><path d="M20 4v7a4 4 0 0 1-4 4H4"/></svg>
          interjected
        </span>
      {/if}
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
{:else if message.role === "agent-notification" && message.notification}
  {@const note = message.notification}
  <!-- A background subagent / workflow agent finished. Not a user message. -->
  <div class="notification-row">
    <div class="notification-card">
      <div class="notification-head">
        <span class="notification-icon" title="A background agent Claude ran finished — this is a system event, not a message you sent">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 2v4M12 18v4M4.9 4.9l2.8 2.8M16.3 16.3l2.8 2.8M2 12h4M18 12h4M4.9 19.1l2.8-2.8M16.3 7.7l2.8-2.8"/></svg>
        </span>
        <span class="notification-title">{notificationTitle(note.summary)}</span>
        <span class="notification-status" class:is-error={note.status !== "completed"}>{note.status}</span>
        <span class="timestamp">{formatTime(message.timestamp)}</span>
      </div>
      <div class="notification-meta">
        <span class="notification-kind">agent finished</span>
        {#if note.tokens != null}<span class="notification-stat">{formatTokens(note.tokens)} tok</span>{/if}
        {#if note.tool_uses != null}<span class="notification-stat">{note.tool_uses} tools</span>{/if}
        {#if note.duration_ms != null}<span class="notification-stat">{formatDuration(note.duration_ms)}</span>{/if}
        {#if note.agent_count != null}<span class="notification-stat">{note.agents_done ?? note.agent_count}/{note.agent_count} agents</span>{/if}
        {#if note.agents_error}<span class="notification-stat is-error">{note.agents_error} failed</span>{/if}
      </div>
      {#if note.result}
        <details class="notification-details">
          <summary>Show result</summary>
          <div class="notification-result">
            {#if searchQuery}
              {@html highlightSearch(escapeHtml(note.result), searchQuery)}
            {:else}
              {note.result}
            {/if}
          </div>
        </details>
      {/if}
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
          {@const questionBlock = segment.name === "AskUserQuestion" && segment.toolUseId ? questions?.[segment.toolUseId] : undefined}
          {@const artifact = segment.name === "Artifact" && segment.toolUseId ? artifacts?.[segment.toolUseId] : undefined}
          {#if artifact}
            <div class="artifact-block">
              <svg class="artifact-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/><path d="M10 13l2 2 4-4"/></svg>
              <div class="artifact-body">
                <div class="artifact-label">Published artifact</div>
                <div class="artifact-title">{artifact.title || "Untitled artifact"}</div>
                <div class="artifact-url">{artifact.url}</div>
              </div>
              <button
                class="artifact-copy"
                class:artifact-copied={copiedArtifactUrl === artifact.url}
                onclick={() => copyArtifactLink(artifact.url)}
                title="Copy artifact link"
              >
                {#if copiedArtifactUrl === artifact.url}
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M20 6L9 17l-5-5"/></svg>
                  Copied
                {:else}
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
                  Copy link
                {/if}
              </button>
            </div>
          {:else if questionBlock && questionBlock.length > 0}
            <div class="question-block">
              {#each questionBlock as qa}
                <div class="question-item">
                  <div class="question-head">
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><path d="M12 17h.01"/><circle cx="12" cy="12" r="10"/></svg>
                    <span class="question-tool">Asked you</span>
                    {#if qa.header}<span class="question-header-tag">{qa.header}</span>{/if}
                    {#if qa.multi_select}<span class="question-multi-tag">multi-select</span>{/if}
                  </div>
                  <div class="question-text">{qa.question}</div>
                  <div class="question-options">
                    {#each qa.options as option}
                      <div class="question-option" class:chosen={option.chosen}>
                        <span class="option-mark">
                          {#if option.chosen}
                            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><path d="M20 6L9 17l-5-5"/></svg>
                          {/if}
                        </span>
                        <div class="option-body">
                          <span class="option-label">
                            {option.label}
                            {#if option.custom}<span class="custom-tag">your answer</span>{/if}
                          </span>
                          {#if option.description}
                            <span class="option-desc">{option.description}</span>
                          {/if}
                        </div>
                      </div>
                    {/each}
                  </div>
                  {#if qa.notes}
                    <div class="question-note">
                      <span class="question-note-label">Your note</span>
                      <span class="question-note-text">{qa.notes}</span>
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          {:else if segment.agentId && onAgentOpen}
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
      {#if message.interrupted}
        <div class="interrupted-tag" title="The user pressed Esc — this reply was cut off mid-response">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M4.9 4.9l14.2 14.2"/></svg>
          interrupted by user
        </div>
      {/if}
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
    color: var(--bg-app);
    padding: 1px 2px;
    border-radius: 2px;
  }

  /* ── Compaction messages ── */

  .compaction-row {
    margin: 28px 0;
    border: 1px dashed var(--border-strong);
    border-radius: 8px;
    padding: 12px 16px;
    background: var(--bg-sidebar);
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
    color: var(--text-faint);
    margin-left: auto;
  }

  .compaction-details {
    margin-top: 8px;
  }

  .compaction-details summary {
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
    user-select: none;
  }

  .compaction-details summary:hover {
    color: var(--text-secondary);
  }

  .compaction-content {
    margin-top: 12px;
    padding: 12px 16px;
    background: var(--bg-app);
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.6;
    max-height: 400px;
    overflow-y: auto;
  }

  .compaction-content :global(p) { margin: 0 0 8px 0; }
  .compaction-content :global(p:last-child) { margin-bottom: 0; }

  /* ── Agent-notification card (background subagent / workflow finished) ── */

  .notification-row {
    margin: 18px 0;
  }

  .notification-card {
    border: 1px solid var(--border-strong);
    border-left: 3px solid #818cf8;
    border-radius: 8px;
    padding: 10px 14px;
    background: var(--bg-sidebar);
  }

  .notification-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .notification-icon {
    display: inline-flex;
    color: #818cf8;
    flex-shrink: 0;
  }

  .notification-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .notification-status {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    padding: 1px 6px;
    border-radius: 4px;
    color: #34d399;
    background: color-mix(in srgb, #34d399 16%, transparent);
    flex-shrink: 0;
  }

  .notification-status.is-error {
    color: #f87171;
    background: color-mix(in srgb, #f87171 16%, transparent);
  }

  .notification-head .timestamp {
    font-size: 11px;
    color: var(--text-faint);
    flex-shrink: 0;
  }

  .notification-meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 7px;
  }

  .notification-kind {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #818cf8;
    font-weight: 600;
  }

  .notification-stat {
    font-size: 11px;
    color: var(--text-muted);
    padding: 1px 7px;
    border-radius: 4px;
    background: var(--bg-app);
    border: 1px solid var(--border);
    font-variant-numeric: tabular-nums;
  }

  .notification-stat.is-error {
    color: #f87171;
  }

  .notification-details {
    margin-top: 9px;
  }

  .notification-details summary {
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
    user-select: none;
  }

  .notification-details summary:hover {
    color: var(--text-secondary);
  }

  .notification-result {
    margin-top: 10px;
    padding: 12px 14px;
    background: var(--bg-app);
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 12.5px;
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 420px;
    overflow-y: auto;
  }

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

  .user-meta .timestamp { font-size: 11px; color: var(--text-faint); }

  .role-tag {
    font-size: 11px;
    font-weight: 600;
    color: var(--accent-hover);
  }

  .user-bubble {
    background: var(--bubble-user);
    border-radius: 12px 12px 4px 12px;
    padding: 10px 16px;
    max-width: 70%;
    color: var(--text-primary);
    font-size: 14px;
    line-height: 1.5;
    position: relative;
    overflow-wrap: anywhere;
    word-break: break-word;
    white-space: pre-wrap;
  }

  /* Mid-turn interjection: a message the user fired while Claude was still
     responding (they pressed Esc and typed). Amber accent distinguishes it
     from the indigo of an ordinary turn. */
  .user-row.mid-turn .user-bubble {
    border-right: 2px solid rgba(245, 158, 11, 0.55);
    border-radius: 12px 4px 4px 12px;
    background: var(--interject-bg);
  }

  .interject-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    color: #fbbf24;
    background: rgba(245, 158, 11, 0.12);
    border: 1px solid rgba(245, 158, 11, 0.3);
    border-radius: 999px;
    padding: 2px 7px 2px 6px;
  }

  .interject-badge svg { flex-shrink: 0; }

  /* End-of-reply marker on an assistant turn that was cut off by the user. */
  .interrupted-tag {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-top: 10px;
    font-size: 11px;
    font-weight: 500;
    color: #d99a4e;
    background: rgba(245, 158, 11, 0.08);
    border: 1px dashed rgba(245, 158, 11, 0.35);
    border-radius: 6px;
    padding: 3px 9px;
  }

  .interrupted-tag svg { flex-shrink: 0; opacity: 0.85; }

  /* AskUserQuestion: the question Claude asked and the option you picked. */
  .question-block {
    margin: 4px 0;
    border: 1px solid var(--border);
    border-left: 3px solid rgba(245, 158, 11, 0.6);
    border-radius: 8px;
    background: var(--bg-panel);
    padding: 12px 14px;
  }

  .question-item + .question-item {
    margin-top: 14px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }

  .question-head {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-bottom: 7px;
  }

  .question-head svg { color: #fbbf24; flex-shrink: 0; }

  .question-tool {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #fbbf24;
  }

  .question-header-tag {
    font-size: 10px;
    font-weight: 600;
    color: #c9a35b;
    background: rgba(245, 158, 11, 0.1);
    border: 1px solid rgba(245, 158, 11, 0.25);
    border-radius: 999px;
    padding: 1px 8px;
  }

  .question-multi-tag {
    font-size: 9.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    padding: 1px 7px;
  }

  .question-note {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: 9px;
    padding: 8px 11px;
    border-left: 2px solid rgba(245, 158, 11, 0.5);
    background: var(--interject-bg);
    border-radius: 0 6px 6px 0;
  }

  .question-note-label {
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #d99a4e;
  }

  .question-note-text {
    font-size: 13px;
    line-height: 1.45;
    color: var(--text-primary);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  /* Published-artifact card (replaces the generic Artifact pill). */
  .artifact-block {
    display: flex;
    align-items: flex-start;
    gap: 11px;
    margin: 4px 0;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: 8px;
    background: var(--bg-panel);
  }

  .artifact-icon { color: var(--accent-hover); flex-shrink: 0; margin-top: 1px; }

  .artifact-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .artifact-label {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--accent-text);
  }

  .artifact-title {
    font-size: 13.5px;
    font-weight: 600;
    color: var(--text-primary);
    overflow-wrap: anywhere;
  }

  .artifact-url {
    font-size: 11.5px;
    color: var(--text-muted);
    font-family: "SF Mono", "Fira Code", monospace;
    word-break: break-all;
  }

  .artifact-copy {
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-elevated);
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 5px 10px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .artifact-copy:hover {
    color: var(--text-primary);
    border-color: var(--accent);
  }

  .artifact-copy.artifact-copied {
    color: #34d399;
    border-color: #34d399;
  }

  .question-text {
    font-size: 13.5px;
    line-height: 1.5;
    color: var(--text-primary);
    margin-bottom: 10px;
    overflow-wrap: anywhere;
  }

  .question-options {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .question-option {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 8px 10px;
    border-radius: 6px;
    border: 1px solid var(--bg-elevated);
    background: var(--bg-sidebar);
    /* Unchosen options recede so the picked answer stands out. */
    opacity: 0.55;
    transition: opacity 0.15s;
  }

  .question-option.chosen {
    opacity: 1;
    border-color: rgba(245, 158, 11, 0.5);
    background: rgba(245, 158, 11, 0.09);
  }

  .option-mark {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    margin-top: 2px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
  }

  .question-option.chosen .option-mark { color: #fbbf24; }

  .question-option:not(.chosen) .option-mark {
    border: 1.5px solid var(--border-strong);
    box-sizing: border-box;
  }

  .option-body {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .option-label {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    gap: 7px;
    flex-wrap: wrap;
  }

  .question-option.chosen .option-label { color: #f4d58d; }

  .custom-tag {
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: #fbbf24;
    background: rgba(245, 158, 11, 0.14);
    border-radius: 4px;
    padding: 1px 6px;
  }

  .option-desc {
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-muted);
    overflow-wrap: anywhere;
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
    border-color: var(--accent);
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
    color: var(--text-muted);
  }

  .image-missing svg { color: var(--text-faint); }

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
    color: var(--text-faint);
    width: 28px;
    height: 28px;
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.15s, background 0.15s;
  }

  .copy-btn:hover { background: rgba(255, 255, 255, 0.06); color: var(--text-secondary); }
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
  .assistant-header .timestamp { font-size: 11px; color: var(--text-faint); }

  .assistant-content {
    background: var(--bg-panel);
    border-radius: 12px;
    padding: 16px 20px;
    color: var(--text-primary);
    font-size: 14px;
    line-height: 1.7;
    border: 1px solid var(--bg-elevated);
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
    color: var(--text-faint);
    flex-shrink: 0;
  }

  .tool-pill-clickable:hover .tool-chevron {
    color: var(--accent-bright);
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
    background: var(--bg-app);
    border: 1px solid var(--bg-elevated);
    border-radius: 8px;
    border-left: 3px solid var(--tool-color, var(--accent));
  }

  .tool-result-panel.tool-result-error {
    border-left-color: #ef4444;
    background: rgba(127, 29, 29, 0.08);
  }

  .tool-result-content {
    margin: 0;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    color: var(--text-secondary);
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
    border-top: 1px solid var(--bg-elevated);
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .tool-load-btn {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    color: var(--accent-bright);
    font-size: 11px;
    font-weight: 500;
    padding: 5px 12px;
    border-radius: 5px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .tool-load-btn:hover {
    background: var(--border);
    border-color: var(--border-strong);
  }

  .loading-text {
    font-size: 11px;
    color: var(--text-muted);
  }

  .error-text {
    font-size: 11px;
    color: #f87171;
  }

  .persisted-block summary {
    font-size: 11px;
    color: var(--text-secondary);
    cursor: pointer;
    user-select: none;
    padding: 2px 0;
  }

  .persisted-block summary:hover {
    color: var(--text-secondary);
  }

  .tool-result-content::-webkit-scrollbar {
    width: 6px;
  }

  .tool-result-content::-webkit-scrollbar-thumb {
    background: var(--border);
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
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 11px;
  }

  /* ── Segment: thinking block ── */

  .thinking-block {
    margin: 8px 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-app);
  }

  .thinking-block summary {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-muted);
    cursor: pointer;
    user-select: none;
  }

  .thinking-block summary:hover { color: var(--text-secondary); }

  .thinking-block summary svg {
    color: var(--accent);
    flex-shrink: 0;
  }

  .thinking-content {
    padding: 0 12px 12px;
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.6;
    max-height: 400px;
    overflow-y: auto;
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }

  .thinking-content :global(p) { margin: 0 0 8px 0; }
  .thinking-content :global(p:last-child) { margin-bottom: 0; }

  /* ── Code blocks with header ── */

  .assistant-content :global(.code-block-wrapper) {
    margin: 10px 0;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid var(--bg-elevated);
  }

  .assistant-content :global(.code-block-header) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 12px;
    background: var(--code-header-bg);
    border-bottom: 1px solid rgba(0, 0, 0, 0.35);
  }

  .assistant-content :global(.code-lang) {
    font-size: 11px;
    color: var(--code-muted);
    font-family: "SF Mono", "Fira Code", monospace;
    text-transform: lowercase;
  }

  .assistant-content :global(.code-copy-btn) {
    font-size: 11px;
    color: var(--code-muted);
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 4px;
    padding: 2px 8px;
    cursor: pointer;
    font-family: inherit;
    transition: all 0.15s;
  }

  .assistant-content :global(.code-copy-btn:hover) {
    color: var(--code-text);
    border-color: rgba(255, 255, 255, 0.3);
    background: rgba(255, 255, 255, 0.05);
  }

  .assistant-content :global(.code-block-wrapper pre) {
    margin: 0;
    border: none;
    border-radius: 0;
  }

  /* Code blocks stay dark in every theme (syntax pastels need a dark ground). */
  .assistant-content :global(pre) {
    background: var(--code-bg);
    color: var(--code-text);
    border-radius: 8px;
    padding: 14px 16px;
    overflow-x: auto;
    margin: 10px 0;
    border: 1px solid var(--border);
  }

  .assistant-content :global(code) {
    font-family: "SF Mono", "Fira Code", "Cascadia Code", monospace;
    font-size: 13px;
  }

  /* Inline code = a small dark chip in every theme, matching the code blocks. */
  .assistant-content :global(:not(pre) > code) {
    background: var(--code-bg);
    padding: 2px 7px;
    border-radius: 4px;
    font-size: 13px;
    color: #d3b3f5;
  }

  /* ── Syntax highlighting ── */

  .assistant-content :global(.syn-keyword) { color: #c792ea; }
  .assistant-content :global(.syn-string) { color: #c3e88d; }
  .assistant-content :global(.syn-comment) { color: #546e7a; font-style: italic; }

  /* ── Standard markdown elements ── */

  .assistant-content :global(h1),
  .assistant-content :global(h2),
  .assistant-content :global(h3) { color: var(--text-primary); margin: 16px 0 8px 0; }

  .assistant-content :global(h1) { font-size: 18px; }
  .assistant-content :global(h2) { font-size: 16px; }
  .assistant-content :global(h3) { font-size: 15px; }
  .assistant-content :global(strong) { color: var(--text-primary); }
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
    border: 1px solid var(--border);
    padding: 8px 12px;
    text-align: left;
  }

  .assistant-content :global(th) { background: var(--bg-elevated); color: var(--text-primary); font-weight: 600; }
  .assistant-content :global(td) { background: var(--bg-app); }
  .assistant-content :global(hr) { border: none; border-top: 1px solid var(--border); margin: 14px 0; }
</style>
