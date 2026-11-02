<script lang="ts">
  import ToolIcon from "./ToolIcon.svelte";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import DOMPurify from "dompurify";
  import { Marked, Renderer } from "marked";
  import type {
    ConversationMessage,
    SessionInfo,
    SubagentInfo,
    ToolResultPayload,
    AnsweredQuestion,
    SessionArtifact,
    PlanItem,
  } from "./types";
  import { prettyToolName, formatTokenCompact, formatDuration, searchWords } from "./format";
  import { copyToClipboard } from "./clipboard";
  import { isBookmarked, toggleBookmark, makeBookmarkId } from "./bookmarks.svelte";

  let {
    message,
    searchQuery = "",
    sessionId = "",
    transcriptPath = "",
    userLabel = "You",
    assistantLabel = "Claude",
    saveOnBookmark = true,
    expandActivity = false,
    onImageOpen,
    onAgentOpen,
    onExportFrom,
    onSessionArchived,
    toolResults,
    subagents = [],
    questions,
    artifacts,
    bookmarkSession = null,
  }: {
    message: ConversationMessage;
    searchQuery?: string;
    sessionId?: string;
    // The JSONL this message came from, for images fetched on demand.
    transcriptPath?: string;
    // Who a "user" message is from. In a subagent's log that's its task, not the person.
    userLabel?: string;
    assistantLabel?: string;
    // Whether bookmarking also archives the session. Only Claude Code sessions are archived.
    saveOnBookmark?: boolean;
    // Show every tool run unfolded.
    expandActivity?: boolean;
    onImageOpen?: (url: string, label: string) => void;
    onAgentOpen?: (agentId: string, description: string) => void;
    // When provided, an "Export from here" action exports this message → end.
    onExportFrom?: () => void;
    // A bookmark just archived this session.
    onSessionArchived?: (archiveError?: string) => void;
    toolResults?: Record<string, ToolResultPayload>;
    // Used to list the agents a Workflow call spawned.
    subagents?: SubagentInfo[];
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

  // Screenshots in tool results are fetched once their pill is open: their data can be huge.
  // An effect, because a pill can be opened before the tool results have arrived.
  let resultImages = $state<Record<string, string[]>>({});
  const requestedImages = new Set<string>();
  $effect(() => {
    for (const toolUseId of expandedTools) {
      if (toolResults?.[toolUseId]?.image_count) loadResultImages(toolUseId);
    }
  });
  async function loadResultImages(toolUseId: string) {
    if (requestedImages.has(toolUseId) || !transcriptPath) return;
    requestedImages.add(toolUseId);
    try {
      const images = await invoke<string[]>("get_tool_result_images", {
        jsonlPath: transcriptPath,
        toolUseId,
      });
      resultImages = { ...resultImages, [toolUseId]: images };
    } catch {
      resultImages = { ...resultImages, [toolUseId]: [] };
    }
  }

  // Very long tool input arrives capped; "Show all" fetches the rest from the transcript.
  let fullInputs = $state<Record<string, string>>({});
  let fullInputErrors = $state<Record<string, string>>({});
  async function loadFullInput(toolUseId: string) {
    try {
      const input = await invoke<string>("get_tool_input", { jsonlPath: transcriptPath, toolUseId });
      fullInputs = { ...fullInputs, [toolUseId]: input };
    } catch (loadError) {
      fullInputErrors = { ...fullInputErrors, [toolUseId]: String(loadError) };
    }
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
        sessionId: sessionId || null,
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

  // Very large results render slowly, so they start capped, with a way to see the rest.
  const INLINE_DISPLAY_LIMIT = 4000;
  let fullyShownResults = $state(new Set<string>());
  function showFullResult(toolUseId: string) {
    fullyShownResults = new Set(fullyShownResults).add(toolUseId);
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

  // Text beside an image keeps its indentation but not the blank lines around it, which
  // would otherwise show as a gap in the bubble.
  function trimBlankLines(text: string): string {
    return text.replace(/^(?:[ \t]*\n)+/, "").replace(/\s+$/, "");
  }

  function parseUserSegments(text: string): UserSegment[] {
    const segments: UserSegment[] = [];
    // `[Image #1]` is what the person typed. `[Image: source: /tmp/…/1.png]` is Claude
    // Code's note of where it put the same image, in a temp folder that won't last.
    const imageRefRegex = /\[Image\s*#(\d+)\]|\[Image:\s*source:\s*([^\]]+)\]/g;
    let lastIndex = 0;
    let match;

    while ((match = imageRefRegex.exec(text)) !== null) {
      if (match.index > lastIndex) {
        const preceding = lastIndex > 0 ? text.slice(lastIndex, match.index).replace(/^ +/, "") : text.slice(0, match.index);
        if (preceding.trim()) segments.push({ kind: "text", content: trimBlankLines(preceding) });
      }

      const fileNumber = match[2]?.trim().match(/(\d+)\.(png|jpg|jpeg|gif|webp)$/i)?.[1];
      const imageNumber = parseInt(match[1] ?? fileNumber ?? "", 10);
      const alreadyShown = segments.some(
        (segment) => segment.kind === "image" && segment.number === imageNumber
      );
      if (!Number.isNaN(imageNumber) && !alreadyShown) {
        const url = resolveImageUrl(imageNumber);
        if (!url) loadCachedImageUrl(imageNumber);
        segments.push({ kind: "image", number: imageNumber, url });
      }

      lastIndex = match.index + match[0].length;
    }

    if (lastIndex < text.length) {
      // The space after an image reference only separated it from the text.
      const trailing = lastIndex > 0 ? text.slice(lastIndex).replace(/^ +/, "") : text;
      if (trailing.trim()) segments.push({ kind: "text", content: trimBlankLines(trailing) });
    }

    return segments;
  }

  // ── Search ──

  function textMatches(text: string | null | undefined): boolean {
    if (!searchQuery || !text) return false;
    const lowered = text.toLowerCase();
    return searchWords(searchQuery).some((word) => lowered.includes(word));
  }

  // ── The prompt ──

  // Only this much of a prompt reaches layout: a pasted bundle or base64 blob with no spaces
  // can hold the window for a minute. Copy still gives the whole message.
  const PROMPT_PREVIEW_CHARS = 20_000;
  // ponytail: a search hit past this point isn't marked; window around the hit if that matters.
  const PROMPT_FULL_CHARS = 1_000_000;

  // A pasted brief can run for pages, and it shouldn't push the answer off the screen.
  let promptExpanded = $state(false);
  let promptHasMatch = $derived(textMatches(message.text));
  let promptOpen = $derived(promptExpanded || promptHasMatch);
  let isLongPrompt = $derived(message.text.length > 800 || message.text.split("\n").length > 8);
  let promptClamped = $derived(isLongPrompt && !promptOpen);
  let promptCut = $derived(promptOpen && message.text.length > PROMPT_FULL_CHARS);
  let promptImageCount = $derived(message.images?.length ?? 0);

  let userSegments = $derived(
    message.role === "user"
      ? parseUserSegments(message.text.slice(0, promptOpen ? PROMPT_FULL_CHARS : PROMPT_PREVIEW_CHARS))
      : []
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

  let assistantBlocks = $derived(message.blocks ?? []);

  // ── Folding tool activity ──

  type Block = NonNullable<ConversationMessage["blocks"]>[number];
  type Group =
    | { kind: "single"; block: Block; index: number }
    | { kind: "activity"; items: { block: Block; index: number }[]; index: number };

  const EDIT_TOOLS = new Set(["Edit", "Write", "MultiEdit", "NotebookEdit"]);
  const COMMAND_TOOLS = new Set(["Bash", "Shell"]);
  const AGENT_TOOLS = new Set(["Agent", "Task", "Workflow"]);
  const SEARCH_TOOLS = new Set(["Grep", "Glob", "WebSearch", "WebFetch", "ToolSearch"]);

  // These answer "what did we decide?", so they stay visible rather than folded away.
  function staysVisible(block: Block): boolean {
    if (block.kind === "text" || block.kind === "note") return true;
    if (block.kind !== "tool" || !block.tool_use_id) return false;
    if (block.name === "AskUserQuestion") return !!questions?.[block.tool_use_id]?.length;
    if (block.name === "Artifact") return !!artifacts?.[block.tool_use_id];
    return false;
  }

  let groups = $derived.by(() => {
    const grouped: Group[] = [];
    assistantBlocks.forEach((block, index) => {
      if (staysVisible(block)) {
        grouped.push({ kind: "single", block, index });
        return;
      }
      const last = grouped[grouped.length - 1];
      if (last?.kind === "activity") last.items.push({ block, index });
      else grouped.push({ kind: "activity", items: [{ block, index }], index });
    });
    return grouped;
  });

  function plural(count: number, one: string, many = `${one}s`): string {
    return `${count} ${count === 1 ? one : many}`;
  }

  // One honest line for a fold: what happened, in plain words.
  function activitySummary(items: { block: Block }[]): string {
    const counts = { commands: 0, reads: 0, edits: 0, searches: 0, agents: 0, others: 0, thinking: 0, images: 0, generated: 0 };
    let plan = "";
    for (const { block } of items) {
      if (block.kind === "thinking") counts.thinking += 1;
      else if (block.kind === "plan") plan = planProgress(block.items);
      else if (block.kind === "tool") {
        if (COMMAND_TOOLS.has(block.name)) counts.commands += 1;
        else if (block.name === "Read") counts.reads += 1;
        else if (EDIT_TOOLS.has(block.name)) counts.edits += 1;
        else if (SEARCH_TOOLS.has(block.name)) counts.searches += 1;
        else if (AGENT_TOOLS.has(block.name)) counts.agents += 1;
        else if (block.name === "ImageGeneration") counts.generated += 1;
        else counts.others += 1;
        if (block.tool_use_id && block.name !== "ImageGeneration") counts.images += toolResults?.[block.tool_use_id]?.image_count ?? 0;
      }
    }
    const parts: string[] = [];
    if (counts.commands) parts.push(`ran ${plural(counts.commands, "command")}`);
    if (counts.reads) parts.push(`read ${plural(counts.reads, "file")}`);
    if (counts.edits) parts.push(`made ${plural(counts.edits, "edit")}`);
    if (counts.searches) parts.push(plural(counts.searches, "search", "searches"));
    if (counts.agents) parts.push(`started ${plural(counts.agents, "agent")}`);
    if (counts.generated) parts.push(`generated ${plural(counts.generated, "image")}`);
    if (counts.others) parts.push(plural(counts.others, "tool call"));
    if (counts.images) parts.push(plural(counts.images, "image"));
    if (plan) parts.push(`plan ${plan}`);
    if (parts.length === 0 && counts.thinking) parts.push("thinking");
    if (parts.length === 0) parts.push("hook output");
    const line = parts.join(", ");
    return line.charAt(0).toUpperCase() + line.slice(1);
  }

  function toolFailures(items: { block: Block }[]): number {
    return items.filter(({ block }) => block.kind === "tool" && !!block.tool_use_id && !!toolResults?.[block.tool_use_id]?.is_error).length;
  }

  function hookFailures(items: { block: Block }[]): number {
    return items.filter(({ block }) => block.kind === "hook" && block.is_error).length;
  }

  // A search hit inside a fold opens it, or you'd see a match with nothing highlighted.
  function activityMatchesSearch(items: { block: Block }[]): boolean {
    if (!searchQuery) return false;
    return items.some(({ block }) => {
      if (block.kind === "tool") return textMatches(`${block.name} ${block.summary}`);
      if (block.kind === "thinking") return textMatches(block.text);
      if (block.kind === "plan") return block.items.some((item) => textMatches(item.content));
      if (block.kind === "hook") return textMatches(`${block.label} ${block.text}`);
      return false;
    });
  }

  // ── Helpers ──

  // A query character as it can appear in rendered HTML. Sanitizing leaves quotes literal
  // but keeps `&` and `<` as entities, and our own escaping encodes all of them.
  const HTML_FORMS: Record<string, string> = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "(?:>|&gt;)",
    '"': '(?:"|&quot;)',
    "'": "(?:'|&#39;)",
  };

  // Marks matches in rendered HTML, leaving tags and entities whole, so `"` finds `&quot;`
  // and searching "quot" can't match inside one.
  function highlightSearch(html: string, query: string): string {
    if (!query) return html;
    const queryPattern = searchWords(query)
      .map((word) => [...word].map((character) => HTML_FORMS[character] ?? character.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(""))
      .join("|");
    const regex = new RegExp(`(${queryPattern})|&(?:[a-z]+|#\\d+|#x[\\da-f]+);`, "gi");
    // A code block's header (language, Copy) is chrome, not message text.
    return html.replace(/(<div class="code-block-header">.*?<\/div>|<[^>]*>)|([^<]+)/g, (segment, tag, text) => {
      if (tag) return tag;
      return text.replace(regex, (matched: string, hit: string) =>
        hit ? `<mark class="search-mark">${matched}</mark>` : matched
      );
    });
  }

  // Codex rewrites an inherited message's time to the moment of the fork, so it would be wrong.
  let messageTime = $derived(message.inherited ? "" : formatTime(message.timestamp));

  function formatTime(isoDate: string): string {
    const date = new Date(isoDate);
    if (Number.isNaN(date.getTime())) return "";
    return date.toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  // Transcripts carry untrusted text (fetched web pages, tool output). The renderer
  // escapes first, but every {@html} still goes through DOMPurify as a second layer.
  function safeHtml(html: string): string {
    return DOMPurify.sanitize(html);
  }

  function escapeHtml(text: string): string {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  // Plain text as HTML, with search marks. For text that's matched but isn't Markdown.
  function markedText(text: string): string {
    return searchQuery ? safeHtml(highlightSearch(escapeHtml(text), searchQuery)) : escapeHtml(text);
  }

  function markedMarkdown(text: string): string {
    return searchQuery ? safeHtml(highlightSearch(renderMarkdown(text), searchQuery)) : renderMarkdown(text);
  }

  function markedInlineMarkdown(text: string): string {
    return safeHtml(highlightSearch(markdown.parseInline(text, { async: false }), searchQuery));
  }

  // ── Notification, plan and event labels ──

  // Strip the framing verbs Claude Code wraps around a name so the card header reads as
  // the task itself: `Agent "Map domain model" finished` → `Map domain model`. Falls back
  // to the raw summary when no quoted name is present.
  function notificationTitle(summary: string): string {
    const quoted = summary.match(/"([^"]+)"/);
    return quoted ? quoted[1] : summary;
  }

  // The agent a report came from, so its card can use the agent's name and open its transcript.
  // Reports name their agent by raw id, description or type.
  let notificationAgent = $derived.by(() => {
    const note = message.notification;
    if (!note) return undefined;
    const title = notificationTitle(note.summary);
    const named = subagents.find((subagent) => note.summary.includes(subagent.agent_id) || subagent.description === title);
    if (named) return named;
    // Some reports name only the agent's type, which identifies it only when there's one of them.
    const ofType = subagents.filter((subagent) => subagent.agent_type === title);
    return ofType.length === 1 ? ofType[0] : undefined;
  });

  let notificationHeading = $derived.by(() => {
    const note = message.notification;
    if (!note) return "";
    const title = notificationTitle(note.summary);
    return notificationAgent?.description || (/^[0-9a-f-]{12,}$/i.test(title) ? (NOTIFICATION_KINDS[note.kind] ?? note.kind) : title);
  });

  const NOTIFICATION_KINDS: Record<string, string> = {
    agent: "Agent finished",
    monitor: "Monitor event",
    shell: "Background command",
    "hand-back": "Subagent report",
    message: "Message from another session",
    task: "Background task",
  };

  const FAILED_STATUSES = new Set(["failed", "error", "killed", "timeout"]);

  const EVENT_LABELS: Record<string, string> = {
    hook: "Hook",
    "api-error": "API error",
    recap: "Recap",
    command: "Command",
    files: "Files",
    notice: "Notice",
    fork: "Forked",
  };

  function planProgress(items: PlanItem[]): string {
    const done = items.filter((item) => item.status === "completed").length;
    const current = items.find((item) => item.status === "in_progress");
    return `${done}/${items.length} done${current ? ` · ${current.content}` : ""}`;
  }

  // ── Markdown rendering ──

  const KEYWORDS_BY_FAMILY: Record<string, string[]> = {
    js: ["const", "let", "var", "function", "return", "if", "else", "for", "while", "import", "export", "from", "class", "extends", "new", "async", "await", "try", "catch", "throw", "typeof", "interface", "type", "enum", "default", "switch", "case", "break", "continue", "true", "false", "null", "undefined", "this", "super"],
    py: ["def", "class", "return", "if", "elif", "else", "for", "while", "import", "from", "as", "try", "except", "raise", "with", "yield", "lambda", "True", "False", "None", "in", "not", "and", "or", "is", "pass", "break", "continue", "self", "async", "await"],
    rust: ["fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod", "if", "else", "for", "while", "loop", "match", "return", "self", "super", "crate", "where", "async", "await", "move", "true", "false", "Some", "None", "Ok", "Err"],
    sh: ["if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "function", "return", "export", "local", "echo", "exit", "cd", "ls", "rm", "cp", "mv", "mkdir", "cat", "grep", "sed", "awk"],
  };
  const LANGUAGE_FAMILIES: Record<string, string> = {
    js: "js", ts: "js", javascript: "js", typescript: "js", jsx: "js", tsx: "js",
    py: "py", python: "py",
    rust: "rust", rs: "rust",
    bash: "sh", sh: "sh", zsh: "sh", shell: "sh",
  };
  const SLASH_COMMENT_LANGUAGES = new Set(["js", "ts", "javascript", "typescript", "jsx", "tsx", "rust", "rs", "go", "java", "c", "cpp", "swift"]);
  const HASH_COMMENT_LANGUAGES = new Set(["py", "python", "ruby", "bash", "sh", "zsh", "shell", "yaml", "yml"]);

  // One pass over the raw code: whatever matches first (comment, string or keyword) wins,
  // so a keyword inside a string stays a string. Everything is escaped on the way out.
  function highlightSyntax(code: string, language: string): string {
    const comment = SLASH_COMMENT_LANGUAGES.has(language)
      ? String.raw`\/\/[^\n]*|\/\*[\s\S]*?\*\/`
      : HASH_COMMENT_LANGUAGES.has(language)
        ? String.raw`#[^\n]*`
        : "";
    const family = LANGUAGE_FAMILIES[language];
    const strings = String.raw`"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'` + (family === "js" ? "|`[^`]*`" : "");
    const keywords = family ? KEYWORDS_BY_FAMILY[family].join("|") : "";
    const pattern = new RegExp(
      `(${comment || "(?!)"})|(${strings})|\\b(${keywords || "(?!)"})\\b`,
      "g"
    );
    let html = "";
    let cursor = 0;
    for (const match of code.matchAll(pattern)) {
      html += escapeHtml(code.slice(cursor, match.index));
      const className = match[1] ? "syn-comment" : match[2] ? "syn-string" : "syn-keyword";
      html += `<span class="${className}">${escapeHtml(match[0])}</span>`;
      cursor = match.index + match[0].length;
    }
    return html + escapeHtml(code.slice(cursor));
  }

  // ── Images written in Markdown ──

  // A local path loads through the backend, which allows only image files of a sane size.
  // Anything else (a missing file, or a web image, which the app's CSP blocks) stays a
  // labelled placeholder, so there is never a broken-image icon.
  let localImages = $state<Record<string, string | null>>({});
  const requestedLocalImages = new Set<string>();

  async function loadLocalImage(path: string) {
    if (requestedLocalImages.has(path)) return;
    requestedLocalImages.add(path);
    let dataUrl: string | null = null;
    try {
      dataUrl = await invoke<string | null>("read_local_image", { path });
    } catch {
      dataUrl = null;
    }
    if (!dataUrl) return;
    renderedMarkdown.clear();
    localImages = { ...localImages, [path]: dataUrl };
  }

  const IMAGE_ICON = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/></svg>`;

  function markdownImage(href: string, alt: string): string {
    const localPath = href.startsWith("file://") ? decodeURI(href.slice("file://".length)) : href;
    const fileName = localPath.split(/[?#]/)[0].split("/").pop() || "";
    const label = escapeHtml(alt || fileName || "Image");
    let source: string | null = /^data:image\//i.test(href) ? href : null;
    if (localPath.startsWith("/")) {
      source = localImages[localPath] ?? null;
      if (!source) loadLocalImage(localPath);
    }
    if (source) {
      return `<button type="button" class="md-image" title="Open image"><img src="${escapeHtml(source)}" alt="${label}"></button>`;
    }
    const detail = alt && fileName && !/^data:/i.test(href) ? ` <span class="md-image-file">${escapeHtml(fileName)}</span>` : "";
    const placeholder = `<span class="md-image-missing" title="${escapeHtml(localPath.slice(0, 500))}">${IMAGE_ICON}Image not shown: ${label}${detail}</span>`;
    return /^https?:\/\//i.test(href) ? `<a href="${escapeHtml(href)}">${placeholder}</a>` : placeholder;
  }

  // An image that can't be decoded after all turns into the same placeholder.
  function replaceBrokenImage(event: Event) {
    const image = event.target;
    if (!(image instanceof HTMLImageElement)) return;
    const placeholder = document.createElement("span");
    placeholder.className = "md-image-missing";
    placeholder.textContent = `Image not shown: ${image.alt}`;
    image.closest(".md-image")?.replaceWith(placeholder);
  }

  const markdown = new Marked({
    gfm: true,
    tokenizer: {
      // A line starting with a tag would otherwise swallow everything up to the next blank
      // line as one raw block, losing its line breaks and Markdown. As ordinary text, tags
      // reach the inline `html` renderer below and the rest renders normally.
      html() {
        return undefined;
      },
      // GFM also strikes through on one tilde, but in prose that means "about": ~$18/mo.
      del(source) {
        return source.startsWith("~~") ? false : undefined;
      },
    },
    renderer: {
      // Code blocks — wrapper with language label and copy button. The copy button
      // finds its sibling <code> via DOM traversal (handleContentClick) so there
      // are no IDs to collide across messages or re-renders.
      code({ text, lang }) {
        const language = (lang ?? "").trim().split(/\s+/)[0].toLowerCase();
        const label = escapeHtml(language || "code");
        return `<div class="code-block-wrapper"><div class="code-block-header"><span class="code-lang">${label}</span><button type="button" class="code-copy-btn" title="Copy code">Copy</button></div><pre><code>${highlightSyntax(text, language)}</code></pre></div>`;
      },
      // Claude writes things like <Entity> in prose: show them, don't parse them as tags.
      // <br> is the one tag people rely on, since a table cell can't hold a line break.
      html({ text }) {
        return /^<br\s*\/?>$/i.test(text.trim()) ? "<br>" : escapeHtml(text);
      },
      // A message's headings sit below the session title and the message itself.
      heading({ tokens, depth }) {
        const level = Math.min(depth + 2, 6);
        return `<h${level} class="md-h${depth}">${this.parser.parseInline(tokens)}</h${level}>`;
      },
      image({ href, text }) {
        return markdownImage(href, text);
      },
      // A wide table scrolls by itself; a message slot clips whatever overflows it.
      table(token) {
        return `<div class="table-scroll" tabindex="0">${Renderer.prototype.table.call(this, token)}</div>`;
      },
      // Claude often links to files in the repo. Those can't be opened from here, so
      // only web links are links; a file reference shows its path on hover.
      link({ href, tokens }) {
        if (/^https?:\/\//i.test(href)) return false;
        return `<span class="file-ref" title="${escapeHtml(href)}">${this.parser.parseInline(tokens)}</span>`;
      },
    },
  });

  // Rendering and sanitizing is the slow part, and only needs to happen once per text.
  const renderedMarkdown = new Map<string, string>();

  function renderMarkdown(text: string): string {
    // Read so that a local image finishing its load re-renders the text that shows it.
    void localImages;
    let html = renderedMarkdown.get(text);
    if (html === undefined) {
      html = safeHtml(markdown.parse(text, { async: false }));
      renderedMarkdown.set(text, html);
    }
    return html;
  }

  // Collapsed sections cost nothing until opened: long sessions have thousands of them.
  // Only a person's click is remembered. The toggle event also fires when "Expand tools" or a
  // search opens a section, and remembering those would leave it open afterwards.
  let openSections = $state(new Set<number>());
  function trackSection(sectionIndex: number, event: Event, shownOpen: boolean) {
    const isOpen = (event.currentTarget as HTMLDetailsElement).open;
    if (isOpen === shownOpen) return;
    const next = new Set(openSections);
    if (isOpen) next.add(sectionIndex);
    else next.delete(sectionIndex);
    openSections = next;
  }

  // ── Delegated clicks: code copy buttons (links are handled in App) ──

  function handleContentClick(event: MouseEvent) {
    const target = event.target as HTMLElement;

    const imageButton = target.closest(".md-image");
    if (imageButton) {
      const image = imageButton.querySelector("img");
      if (image) onImageOpen?.(image.src, image.alt);
      return;
    }

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

  // Clean, copyable text. For assistant messages: prose, with tool calls as bracketed
  // labels. Thinking, plans and hook output are left out.
  function buildCopyText(): string {
    if (message.role === "assistant") {
      return assistantBlocks
        .map((block) => {
          if (block.kind === "text") return block.text;
          if (block.kind === "note") return `${block.label}: ${block.text}`;
          if (block.kind === "tool") {
            const name = prettyToolName(block.name);
            return block.summary ? `[${name}: ${block.summary}]` : `[${name}]`;
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
    if (!wasBookmarked && saveOnBookmark) {
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
      })
        .then(() => onSessionArchived?.())
        .catch((archiveError) => onSessionArchived?.(String(archiveError)));
    }
  }
</script>

{#if message.role === "compaction"}
  {@const summaryOpen = openSections.has(-1) || textMatches(message.text)}
  <div class="compaction-row" class:inherited={message.inherited}>
    <div class="compaction-header">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 6h16M4 12h16M4 18h7"/><path d="M19 15l-3 3 3 3"/></svg>
      <span class="compaction-label">Context Compacted</span>
      {#if message.compaction_detail}<span class="compaction-tokens">{message.compaction_detail}</span>{/if}
      <span class="timestamp">{messageTime}</span>
    </div>
    <details class="compaction-details" open={summaryOpen} ontoggle={(event) => trackSection(-1, event, summaryOpen)}>
      <summary>View compaction summary</summary>
      {#if summaryOpen}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="compaction-content markdown-body" onclick={handleContentClick} onerrorcapture={replaceBrokenImage}>
          {@html markedMarkdown(message.text)}
        </div>
      {/if}
    </details>
  </div>
{:else if message.role === "system-event" && message.event && message.event.kind === "hook" && !message.event.is_error && !expandActivity && !searchQuery}
  <!-- A hook that worked is background noise. "Expand tools" brings these back. -->
{:else if message.role === "system-event" && message.event}
  {@const event = message.event}
  <div class="event-row" class:event-error={event.is_error} class:event-recap={event.kind === "recap"} class:event-fork={event.kind === "fork"} class:inherited={message.inherited}>
    <div class="event-line">
      <span class="event-kind">{EVENT_LABELS[event.kind] ?? event.kind}</span>
      <span class="event-text" class:markdown-body={event.kind === "recap"}>{@html event.kind === "recap" ? markedInlineMarkdown(message.text) : markedText(message.text)}</span>
      <span class="timestamp">{messageTime}</span>
    </div>
    {#if event.detail}
      <details class="event-details" open={textMatches(event.detail)}>
        <summary>Details</summary>
        <pre class="tool-result-content">{@html markedText(event.detail)}</pre>
      </details>
    {/if}
  </div>
{:else if message.role === "shell" && message.shell}
  {@const run = message.shell}
  <div class="shell-row" class:inherited={message.inherited}>
    <div class="user-meta">
      <span class="timestamp">{messageTime}</span>
      <span class="role-tag">You ran</span>
    </div>
    <div class="shell-card">
      {#if run.command}<div class="shell-command"><span class="shell-prompt">$</span> {@html markedText(run.command)}</div>{/if}
      {#if run.stdout}<pre class="shell-output">{@html markedText(run.stdout)}</pre>{/if}
      {#if run.stderr}<pre class="shell-output shell-stderr">{@html markedText(run.stderr)}</pre>{/if}
    </div>
  </div>
{:else if message.role === "user"}
  <article class="user-row" class:mid-turn={message.mid_turn} class:inherited={message.inherited} aria-label="{userLabel} {messageTime}">
    <div class="user-meta">
      {#if message.inherited}<span class="inherited-tag">From the original session</span>{/if}
      {#if message.mid_turn}
        <span class="interject-badge">
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><polyline points="9 10 4 15 9 20"/><path d="M20 4v7a4 4 0 0 1-4 4H4"/></svg>
          Sent while the agent was working
        </span>
      {/if}
      <span class="timestamp">{messageTime}</span>
      <span class="role-tag">{userLabel}</span>
    </div>
    <div class="user-bubble" class:clamped={promptClamped}>
      {#each userSegments as segment}
        {#if segment.kind === "text"}
          <p>{@html markedText(segment.content)}</p>
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
            <span class="image-missing">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/></svg>
              Image #{segment.number} not found
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
      {#if promptCut}
        <p class="prompt-cut">Showing the first {PROMPT_FULL_CHARS.toLocaleString()} of {message.text.length.toLocaleString()} characters. Copy the message to get all of it.</p>
      {/if}
    </div>
    {#if isLongPrompt && !promptHasMatch}
      <button class="show-prompt" aria-expanded={promptExpanded} onclick={() => (promptExpanded = !promptExpanded)}>
        {promptExpanded ? "Show less" : "Show full prompt"}{#if !promptExpanded && promptImageCount > 0}&nbsp;· {plural(promptImageCount, "image")}{/if}
      </button>
    {/if}
    <div class="user-actions">
      <span class="sr-only" role="status">{copied ? "Copied" : ""}</span>
      {#if bookmarkSession}
        <button class="copy-btn bookmark-btn" class:bookmarked onclick={toggleBookmarkForMessage} title={bookmarked ? "Remove bookmark" : "Save for later"} aria-label="Bookmark message" aria-pressed={bookmarked}>
          {#if bookmarked}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
          {:else}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
          {/if}
        </button>
      {/if}
      <button class="copy-btn" class:copied onclick={copyText} title="Copy message" aria-label="Copy message">
        {#if copied}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M20 6L9 17l-5-5"/></svg>
        {:else}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
        {/if}
      </button>
      {#if onExportFrom}
        <button class="copy-btn" onclick={() => onExportFrom?.()} title="Export from here to the end (Markdown)" aria-label="Export from this message onward">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 3v12"/><path d="m8 11 4 4 4-4"/><path d="M4 21h16"/></svg>
        </button>
      {/if}
    </div>
  </article>
{:else if message.role === "agent-notification" && message.notification}
  {@const note = message.notification}
  <!-- A background task finished or a subagent reported back. Not a user message. -->
  <div class="notification-row" class:inherited={message.inherited}>
    <div class="notification-card">
      <div class="notification-head">
        <span class="notification-icon" aria-hidden="true">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/><path d="M8 13h8M8 17h5"/></svg>
        </span>
        {#if notificationAgent && onAgentOpen}
          {@const agent = notificationAgent}
          <button type="button" class="notification-title notification-link" onclick={() => onAgentOpen?.(agent.agent_id, agent.description ?? "")}>
            <span class="notification-link-text">{@html markedText(notificationHeading)}</span>
            <span class="notification-link-hint">Open transcript</span>
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="M9 18l6-6-6-6"/></svg>
          </button>
        {:else}
          <span class="notification-title">{@html markedText(notificationHeading)}</span>
        {/if}
        {#if note.status}<span class="notification-status" class:is-neutral={note.status !== "completed" && !FAILED_STATUSES.has(note.status)} class:is-error={FAILED_STATUSES.has(note.status)}>{note.status}</span>{/if}
        <span class="timestamp">{messageTime}</span>
      </div>
      <div class="notification-meta">
        {#if (NOTIFICATION_KINDS[note.kind] ?? note.kind) !== notificationHeading}
          <span class="notification-kind">{NOTIFICATION_KINDS[note.kind] ?? note.kind}</span>
        {/if}
        {#if note.tokens != null}<span class="notification-stat">{formatTokenCompact(note.tokens)} tok</span>{/if}
        {#if note.tool_uses != null}<span class="notification-stat">{note.tool_uses} tools</span>{/if}
        {#if note.duration_ms != null}<span class="notification-stat">{formatDuration(note.duration_ms)}</span>{/if}
        {#if note.agent_count != null}<span class="notification-stat">{note.agents_done ?? note.agent_count}/{note.agent_count} agents</span>{/if}
        {#if note.agents_error}<span class="notification-stat is-error">{note.agents_error} failed</span>{/if}
      </div>
      {#if note.result}
        {@const resultOpen = openSections.has(-1) || textMatches(note.result)}
        <details class="notification-details" open={resultOpen} ontoggle={(event) => trackSection(-1, event, resultOpen)}>
          <summary>Show result</summary>
          {#if resultOpen}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="notification-result markdown-body" onclick={handleContentClick} onerrorcapture={replaceBrokenImage}>
              {@html markedMarkdown(note.result)}
            </div>
          {/if}
        </details>
      {/if}
    </div>
  </div>
{:else}
  <!-- Assistant message: structured segments -->
  <article class="assistant-row" class:inherited={message.inherited} aria-label="{assistantLabel} {messageTime}">
    <div class="assistant-header">
      <ToolIcon tool={assistantLabel.toLowerCase()} size={15} /><span class="claude-label">{assistantLabel}</span>
      {#if message.inherited}<span class="inherited-tag">From the original session</span>{/if}
      <span class="timestamp">{messageTime}</span>
    </div>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="assistant-content markdown-body" onclick={handleContentClick} onerrorcapture={replaceBrokenImage}>
      {#snippet renderBlock(block: Block, blockIndex: number)}
        {#if block.kind === "text"}
          <div class="segment-text">
            {@html markedMarkdown(block.text)}
          </div>
        {:else if block.kind === "plan"}
          <details class="plan-block" open={block.items.some((item) => textMatches(item.content))}>
            <summary><span class="plan-label">Plan</span> <span class="plan-progress">{planProgress(block.items)}</span></summary>
            <ul class="plan-items">
              {#each block.items as item}
                <li class="plan-item plan-{item.status}">
                  <span class="plan-mark" role="img" aria-label={item.status.replace("_", " ")}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true">
                      {#if item.status === "completed"}<path d="M20 6 9 17l-5-5" />{:else if item.status === "in_progress"}<path d="m9 18 6-6-6-6" />{:else}<circle cx="12" cy="12" r="8" />{/if}
                    </svg>
                  </span>
                  <span>{@html markedText(item.content)}</span>
                </li>
              {/each}
            </ul>
          </details>
        {:else if block.kind === "note"}
          <div class="note-block">
            <div class="note-label">{@html markedText(block.label)}</div>
            {#if block.text}<div class="segment-text">{@html markedMarkdown(block.text)}</div>{/if}
          </div>
        {:else if block.kind === "hook"}
          <details class="hook-block" class:hook-error={block.is_error} open={textMatches(block.text)}>
            <summary>{@html markedText(block.label)}</summary>
            {#if block.text}<pre class="tool-result-content">{@html markedText(block.text)}</pre>{/if}
          </details>
        {:else if block.kind === "tool"}
          {@const toolUseId = block.tool_use_id}
          {@const toolResult = toolUseId ? toolResults?.[toolUseId] : undefined}
          {@const isExpanded = toolUseId ? expandedTools.has(toolUseId) : false}
          {@const questionBlock = block.name === "AskUserQuestion" && toolUseId ? questions?.[toolUseId] : undefined}
          {@const artifact = block.name === "Artifact" && toolUseId ? artifacts?.[toolUseId] : undefined}
          {@const hasPanel = !!block.input || !!toolResult?.content.trim() || !!toolResult?.image_count || !!toolResult?.persisted_path}
          {@const workflowAgents = block.name === "Workflow" && block.agent_id ? subagents.filter((subagent) => subagent.workflow_run === block.agent_id) : []}
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
            <div class="question-block" class:tool-pill-error={toolResult?.is_error}>
              {#each questionBlock as qa}
                <div class="question-item">
                  <div class="question-head">
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><path d="M12 17h.01"/><circle cx="12" cy="12" r="10"/></svg>
                    <span class="question-tool">Asked you</span>
                    {#if qa.header}<span class="question-header-tag">{qa.header}</span>{/if}
                    {#if qa.multi_select}<span class="question-multi-tag">multi-select</span>{/if}
                    {#if toolResult?.is_error}<span class="tool-error-tag">error</span>{/if}
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
          {:else if block.agent_id && block.name !== "Workflow" && onAgentOpen}
            <button
              type="button"
              class="tool-pill tool-pill-clickable"
              class:tool-pill-error={toolResult?.is_error}
              onclick={() => onAgentOpen?.(block.agent_id!, block.summary)}
              title="Open subagent transcript"
            >
              <span class="tool-name">{@html markedText(prettyToolName(block.name))}</span>
              {#if block.summary}
                <span class="tool-summary">{@html markedText(block.summary)}</span>
              {/if}
              {#if toolResult?.is_error}
                <span class="tool-error-tag">error</span>
              {/if}
              <svg class="tool-chevron" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M9 18l6-6-6-6"/></svg>
            </button>
          {:else if toolUseId && hasPanel}
            <button
              type="button"
              class="tool-pill tool-pill-clickable"
              class:tool-pill-error={toolResult?.is_error}
              class:tool-pill-expanded={isExpanded}
              aria-expanded={isExpanded}
              onclick={() => toggleToolResult(toolUseId)}
              title={isExpanded ? "Collapse" : "Show input and output"}
            >
              <span class="tool-name">{@html markedText(prettyToolName(block.name))}</span>
              {#if block.summary}
                <span class="tool-summary">{@html markedText(block.summary)}</span>
              {/if}
              {#if toolResult?.is_error}
                <span class="tool-error-tag">error</span>
              {/if}
              {#if toolResult && toolResult.image_count > 0}
                <span class="tool-image-tag">{toolResult.image_count} image{toolResult.image_count === 1 ? "" : "s"}</span>
              {/if}
              <svg class="tool-chevron tool-chevron-toggle" class:rotated={isExpanded} width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="m6 9 6 6 6-6"/></svg>
            </button>
            {#if isExpanded}
              <div class="tool-result-panel" class:tool-result-error={toolResult?.is_error}>
                {#if block.input}
                  <div class="tool-section-label">Input</div>
                  <pre class="tool-result-content">{fullInputs[toolUseId] ?? block.input}</pre>
                  {#if block.input_truncated && !fullInputs[toolUseId]}
                    {#if fullInputErrors[toolUseId]}
                      <span class="error-text">Couldn't load the full input: {fullInputErrors[toolUseId]}</span>
                    {:else}
                      <button class="tool-load-btn tool-show-all" onclick={() => loadFullInput(toolUseId)}>Show all</button>
                    {/if}
                  {/if}
                {/if}
                {#if toolResult}
                  {@const visible = previewContent(toolResult.content)}
                  {@const showAll = fullyShownResults.has(toolUseId) || visible.length <= INLINE_DISPLAY_LIMIT}
                  {#if visible}
                    {#if block.input}<div class="tool-section-label">Output</div>{/if}
                    <pre class="tool-result-content">{showAll ? visible : visible.slice(0, INLINE_DISPLAY_LIMIT)}</pre>
                    {#if !showAll}
                      <button class="tool-load-btn tool-show-all" onclick={() => showFullResult(toolUseId)}>
                        Show all {visible.length.toLocaleString()} characters
                      </button>
                    {/if}
                  {/if}
                  {#if resultImages[toolUseId]?.length}
                    <div class="tool-images">
                      {#each resultImages[toolUseId] as imageUrl, imageIndex}
                        <button type="button" class="user-image-link" title="Open image" onclick={() => onImageOpen?.(imageUrl, `${prettyToolName(block.name)} image ${imageIndex + 1}`)}>
                          <img src={imageUrl} alt="Tool result {imageIndex + 1}" class="user-image" loading="lazy" />
                        </button>
                      {/each}
                    </div>
                  {/if}
                  {#if toolResult.image_count > 0 && resultImages[toolUseId]?.length === 0}
                    <div class="loading-text">The image data in this result couldn't be read.</div>
                  {/if}
                  {#if toolResult.persisted_path}
                    <div class="tool-result-actions">
                      {#if persistedContents[toolUseId]}
                        <details class="persisted-block">
                          <summary>Full output ({persistedContents[toolUseId].length.toLocaleString()} chars)</summary>
                          <pre class="tool-result-content persisted-content">{persistedContents[toolUseId]}</pre>
                        </details>
                      {:else if persistedLoading[toolUseId]}
                        <span class="loading-text">Loading full output...</span>
                      {:else if persistedErrors[toolUseId]}
                        <span class="error-text">Failed to load: {persistedErrors[toolUseId]}</span>
                      {:else}
                        <button class="tool-load-btn" onclick={() => loadPersistedFor(toolUseId, toolResult.persisted_path!)}>
                          Load full output
                        </button>
                      {/if}
                    </div>
                  {/if}
                {/if}
              </div>
            {/if}
          {:else}
            <div class="tool-pill" class:tool-pill-error={toolResult?.is_error}>
              <span class="tool-name">{@html markedText(prettyToolName(block.name))}</span>
              {#if block.summary}
                <span class="tool-summary">{@html markedText(block.summary)}</span>
              {/if}
              {#if toolResult?.is_error}
                <span class="tool-error-tag">error</span>
              {/if}
            </div>
          {/if}
          {#if workflowAgents.length > 0 && onAgentOpen}
            <details class="workflow-agents">
              <summary>{workflowAgents.length} agent{workflowAgents.length === 1 ? "" : "s"} in this workflow</summary>
              <div class="workflow-agent-list">
                {#each workflowAgents as workflowAgent (workflowAgent.agent_id)}
                  <button
                    type="button"
                    class="tool-pill tool-pill-clickable"
                    onclick={() => onAgentOpen?.(workflowAgent.agent_id, workflowAgent.description ?? "")}
                    title="Open this agent's transcript"
                  >
                    <span class="tool-name">{workflowAgent.agent_type ?? "agent"}</span>
                    <span class="tool-summary">{workflowAgent.description ?? workflowAgent.agent_id}</span>
                    <svg class="tool-chevron" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M9 18l6-6-6-6"/></svg>
                  </button>
                {/each}
              </div>
            </details>
          {/if}
        {:else if block.kind === "thinking"}
          {@const thinkingOpen = openSections.has(-1 - blockIndex) || textMatches(block.text)}
          <details class="thinking-block" open={thinkingOpen} ontoggle={(event) => trackSection(-1 - blockIndex, event, thinkingOpen)}>
            <summary>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/></svg>
              Thinking...
            </summary>
            {#if thinkingOpen}
              <div class="thinking-content">
                {@html markedMarkdown(block.text)}
              </div>
            {/if}
          </details>
        {/if}
      {/snippet}
      {#each groups as group (group.index)}
        {#if group.kind === "single"}
          {@render renderBlock(group.block, group.index)}
        {:else}
          {@const failedTools = toolFailures(group.items)}
          {@const failedHooks = hookFailures(group.items)}
          {@const sectionKey = group.index + 1}
          {@const isOpen = expandActivity || openSections.has(sectionKey) || activityMatchesSearch(group.items)}
          <details class="activity" open={isOpen} ontoggle={(event) => trackSection(sectionKey, event, isOpen)}>
            <summary>
              <svg class="activity-chevron" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
              <span>{activitySummary(group.items)}</span>
              {#if failedTools > 0}<span class="activity-failed">{failedTools} failed</span>{/if}
              {#if failedHooks > 0}<span class="activity-hook-failed">{plural(failedHooks, "hook failure")}</span>{/if}
            </summary>
            {#if isOpen}
              <div class="activity-items">
                {#each group.items as item (item.index)}
                  {@render renderBlock(item.block, item.index)}
                {/each}
              </div>
            {/if}
          </details>
        {/if}
      {/each}
      {#if message.interrupted}
        <div class="interrupted-tag">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="12" cy="12" r="10"/><path d="M4.9 4.9l14.2 14.2"/></svg>
          interrupted by user
        </div>
      {/if}
    </div>
    <div class="assistant-actions">
      <span class="sr-only" role="status">{copied ? "Copied" : ""}</span>
      {#if bookmarkSession}
        <button class="copy-btn bookmark-btn" class:bookmarked onclick={toggleBookmarkForMessage} title={bookmarked ? "Remove bookmark" : "Save for later"} aria-label="Bookmark message" aria-pressed={bookmarked}>
          {#if bookmarked}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
          {:else}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
          {/if}
        </button>
      {/if}
      <button class="copy-btn" class:copied onclick={copyText} title="Copy message" aria-label="Copy message">
        {#if copied}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M20 6L9 17l-5-5"/></svg>
        {:else}
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
        {/if}
      </button>
      {#if onExportFrom}
        <button class="copy-btn" onclick={() => onExportFrom?.()} title="Export from here to the end (Markdown)" aria-label="Export from this message onward">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 3v12"/><path d="m8 11 4 4 4-4"/><path d="M4 21h16"/></svg>
        </button>
      {/if}
    </div>
  </article>
{/if}

<style>
  /* ── Search highlight ── */

  :global(mark.search-mark) {
    background: var(--mark-bg);
    color: var(--mark-text);
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
    color: var(--text-secondary);
  }

  .compaction-label {
    font-size: 13px;
    font-weight: 600;
  }

  .compaction-header .timestamp {
    font-size: 12px;
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

  .compaction-tokens {
    font-size: 12px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  /* ── System events: hooks, API errors, recaps, notices ── */

  .event-row {
    margin: 10px 0;
    padding: 5px 10px;
    border-left: 1px solid var(--border-strong);
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-muted);
  }

  .event-line {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .event-kind {
    flex-shrink: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-faint);
  }

  .event-text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .event-line .timestamp {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-faint);
  }

  .event-error { border-left-color: var(--danger); }
  .event-error .event-kind,
  .event-error .event-text { color: var(--danger); }

  .event-recap .event-kind { color: var(--text-secondary); }
  .event-recap .event-text { color: var(--text-secondary); }

  /* Where a fork's own work starts: a boundary across the column, not one more note. */
  .event-fork {
    margin: 24px 0;
    padding: 8px 0 0;
    border-left: none;
    border-top: 1px solid var(--border-strong);
  }

  .event-fork .event-kind { color: var(--text-primary); }

  /* Copied from the session this one forked from: same layout, quieter text. Colour, not
     opacity, so it stays readable. */
  .inherited {
    --text-primary: var(--text-secondary);
    --text-secondary: var(--text-muted);
    --bubble-user: var(--bg-sidebar);
  }

  .inherited-tag {
    font-size: 12px;
    color: var(--text-muted);
  }

  .event-details,
  .hook-block { margin-top: 4px; }

  .event-details summary,
  .hook-block summary {
    font-size: 12px;
    color: var(--text-faint);
    cursor: pointer;
    user-select: none;
  }

  .event-details summary:hover,
  .hook-block summary:hover { color: var(--text-secondary); }

  .event-details pre,
  .hook-block pre { margin-top: 6px; }

  .hook-block { margin: 4px 0; }
  .hook-block.hook-error summary { color: var(--danger); }

  /* ── Shell commands the user ran with `!` ── */

  .shell-row {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    margin: 28px 0 12px;
  }

  .shell-card {
    width: 100%;
    background: var(--code-bg);
    color: var(--code-text);
    border-radius: 10px;
    padding: 10px 14px;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    line-height: 1.5;
  }

  .shell-command {
    font-weight: 600;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .shell-prompt { color: var(--code-muted); }

  .shell-output {
    margin: 8px 0 0;
    max-height: 320px;
    overflow-y: auto;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--code-muted);
    font: inherit;
  }

  .shell-stderr { color: var(--code-danger); }

  /* ── Plan, recap and chapter blocks inside a turn ── */

  .plan-block {
    margin: 4px 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-app);
  }

  .plan-block summary {
    padding: 5px 10px;
    font-size: 12px;
    cursor: pointer;
    user-select: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .plan-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--teal-text);
    font-family: "SF Mono", "Fira Code", monospace;
  }

  .plan-progress { color: var(--text-muted); }

  .plan-items {
    list-style: none;
    margin: 0;
    padding: 4px 10px 9px;
    font-size: 12.5px;
    line-height: 1.5;
  }

  .assistant-content .plan-items li { margin: 2px 0; }

  .plan-item {
    display: flex;
    gap: 7px;
    color: var(--text-secondary);
  }

  .plan-mark {
    flex-shrink: 0;
    width: 14px;
    text-align: center;
    color: var(--text-faint);
  }

  .plan-completed { color: var(--text-muted); }
  .plan-completed .plan-mark { color: var(--ok); }
  .plan-in_progress { color: var(--text-primary); font-weight: 500; }
  .plan-in_progress .plan-mark { color: var(--accent-text); }

  .note-block {
    margin: 10px 0 4px;
    padding: 8px 12px;
    border-left: 1px solid var(--accent);
    background: var(--bg-app);
    border-radius: 0 6px 6px 0;
  }

  .note-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-text);
    margin-bottom: 2px;
  }

  /* ── Agent-notification card (background task finished / subagent reported back) ── */

  .notification-row {
    margin: 18px 0;
  }

  .notification-card {
    border: 1px solid var(--border-strong);
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
    color: var(--text-muted);
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

  button.notification-title {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 24px;
    padding: 0;
    background: none;
    border: none;
    color: var(--text-primary);
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }

  .notification-link-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .notification-link-hint {
    flex-shrink: 0;
    font-size: 12px;
    font-weight: 400;
    color: var(--text-muted);
  }

  .notification-link svg { flex-shrink: 0; color: var(--text-muted); }
  .notification-link:hover .notification-link-text { text-decoration: underline; }

  .notification-status {
    font-size: 12px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 4px;
    color: var(--ok);
    background: var(--ok-soft);
    flex-shrink: 0;
  }

  .notification-status.is-neutral {
    color: var(--text-muted);
    background: var(--bg-hover);
  }

  .notification-status.is-error {
    color: var(--danger);
    background: var(--danger-soft);
  }

  .notification-head .timestamp {
    font-size: 12px;
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
    font-size: 12px;
    color: var(--text-secondary);
    font-weight: 600;
  }

  .notification-stat {
    font-size: 12px;
    color: var(--text-muted);
    padding: 1px 7px;
    border-radius: 4px;
    background: var(--bg-app);
    border: 1px solid var(--border);
    font-variant-numeric: tabular-nums;
  }

  .notification-stat.is-error {
    color: var(--danger);
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
    max-height: 420px;
    overflow-y: auto;
  }

  .notification-result :global(p) { margin: 0 0 8px 0; }
  .notification-result :global(p:last-child) { margin-bottom: 0; }

  /* ── User messages ── */

  /* Your messages sit on the right in a bubble, and the agent's replies run plain across the
     column. Position and fill both carry it, so it reads at a glance in any theme. */
  .user-row {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    margin: 28px 0 12px;
  }

  .user-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }

  .user-meta .timestamp { font-size: 12px; color: var(--text-faint); }

  .role-tag {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text-primary);
  }

  /* Six whole lines. The border stands in for bottom padding, because overflow shows
     through padding and would slice the seventh line. */
  .user-bubble.clamped {
    box-sizing: content-box;
    max-height: calc(6 * 21px);
    overflow: hidden;
    padding-bottom: 0;
    border-bottom: 8px solid var(--bubble-user);
    line-height: 21px;
  }

  .user-bubble.clamped :global(p) { line-height: 21px; }

  .prompt-cut {
    margin-top: 8px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .show-prompt {
    margin-top: 3px;
    min-height: 24px;
    padding: 2px 0;
    background: none;
    border: none;
    font-size: 12.5px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .show-prompt:hover { text-decoration: underline; }

  .user-bubble {
    width: fit-content;
    max-width: 82%;
    padding: 10px 14px;
    background: var(--bubble-user);
    border: 1px solid var(--border-strong);
    border-radius: 14px 14px 4px 14px;
    color: var(--text-primary);
    font-size: 14px;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  /* Mid-turn interjection: a message the user fired while Claude was still
     responding (they pressed Esc and typed). Amber accent distinguishes it
     from the indigo of an ordinary turn. */
  .user-row.mid-turn .user-bubble {
    border-color: var(--accent-border);
    background: var(--interject-bg);
  }

  /* The clamp's bottom border is padding in disguise, so it stays the bubble's own colour. */
  .user-row.mid-turn .user-bubble.clamped { border-bottom-color: var(--interject-bg); }

  .interject-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-text);
    background: var(--accent-soft);
    border: 1px solid var(--accent-border);
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
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    border: 1px dashed var(--border-strong);
    border-radius: 6px;
    padding: 3px 9px;
  }

  .interrupted-tag svg { flex-shrink: 0; opacity: 0.85; }

  /* AskUserQuestion: the question Claude asked and the option you picked. */
  .question-block {
    margin: 4px 0;
    border: 1px solid var(--border);
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

  .question-head svg { color: var(--text-muted); flex-shrink: 0; }

  .question-tool {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .question-header-tag {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    background: var(--bg-hover);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px 8px;
  }

  .question-multi-tag {
    font-size: 12px;
    font-weight: 600;
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
    border-left: 1px solid var(--border-strong);
    background: var(--bg-hover);
    border-radius: 0 6px 6px 0;
  }

  .question-note-label {
    font-size: 12px;
    font-weight: 700;
    color: var(--text-secondary);
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
    border-left: 1px solid var(--accent);
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
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-text);
  }

  .artifact-title {
    font-size: 13.5px;
    font-weight: 600;
    color: var(--text-primary);
    overflow-wrap: anywhere;
  }

  .artifact-url {
    font-size: 12px;
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
    color: var(--ok);
    border-color: var(--ok);
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
  }

  .question-option.chosen {
    border-color: var(--accent-border);
    background: var(--accent-soft);
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

  .question-option.chosen .option-mark { color: var(--accent-text); }

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

  /* Unchosen options recede through colour so the picked answer stands out. */
  .option-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    gap: 7px;
    flex-wrap: wrap;
  }

  .question-option.chosen .option-label { font-weight: 600; color: var(--accent-text); }

  .custom-tag {
    font-size: 12px;
    font-weight: 700;
    color: var(--accent-text);
    background: var(--accent-soft);
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
    white-space: pre-wrap;
    margin: 0;
    overflow-wrap: anywhere;
    word-break: break-word;
  }
  .user-bubble p + p { margin-top: 6px; }

  .user-image-link {
    display: inline-block;
    min-width: 24px;
    min-height: 24px;
    margin: 6px 6px 0 0;
    padding: 0;
    background: transparent;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid var(--border);
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
    background: var(--bg-hover);
    border: 1px dashed var(--border-strong);
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .image-missing svg { color: var(--text-faint); }

  .user-actions {
    display: flex;
    justify-content: flex-start;
    margin-top: 2px;
    opacity: 0;
    transition: opacity 0.15s;
  }

  .user-row:hover .user-actions,
  .user-actions:focus-within { opacity: 1; }
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

  .assistant-row:hover .assistant-actions,
  .assistant-actions:focus-within { opacity: 1; }
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

  .copy-btn:hover { background: var(--bg-hover); color: var(--text-secondary); }
  .copy-btn.copied { color: var(--ok); }
  .bookmark-btn:hover { color: var(--accent-text); }
  .bookmark-btn.bookmarked { color: var(--accent-text); }

  /* ── Assistant messages ── */

  .assistant-row {
    margin: 8px 0 14px;
  }

  .assistant-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }

  .claude-label { font-size: 12.5px; font-weight: 600; color: var(--text-secondary); }
  .assistant-header .timestamp { font-size: 12px; color: var(--text-faint); }

  .assistant-content {
    color: var(--text-primary);
    font-size: 14px;
    line-height: 1.6;
  }

  /* Tool activity folds into one line, so prompts and answers read as the conversation. */
  .activity { margin: 6px 0; }

  .activity summary {
    display: flex;
    align-items: baseline;
    gap: 7px;
    padding: 4px 6px 4px 2px;
    list-style: none;
    border-radius: 4px;
    font-size: 12.5px;
    color: var(--text-muted);
    cursor: pointer;
    user-select: none;
  }

  .activity summary::-webkit-details-marker { display: none; }
  .activity summary:hover { background: var(--bg-hover); color: var(--text-primary); }

  .activity-chevron {
    flex-shrink: 0;
    align-self: center;
    transition: transform 0.12s;
  }

  .activity[open] > summary .activity-chevron { transform: rotate(90deg); }
  .activity-failed,
  .activity-hook-failed { flex-shrink: 0; font-weight: 500; color: var(--danger); }
  .activity-items { padding: 2px 0 4px 16px; }

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
    background: var(--bg-app);
    border: 1px solid var(--border);
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
    background: var(--bg-hover);
    border-color: var(--border-strong);
  }

  .tool-pill-expanded {
    background: var(--accent-soft);
    border-color: var(--accent-border);
  }

  .tool-pill-error {
    background: var(--danger-soft);
    border-color: var(--danger-border);
  }

  .tool-pill-error:hover {
    background: var(--danger-soft);
    border-color: var(--danger);
  }

  .tool-error-tag {
    font-size: 12px;
    color: var(--danger);
    padding: 1px 6px;
    border-radius: 3px;
    font-weight: 600;
  }

  .tool-image-tag {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-muted);
    background: var(--bg-hover);
    padding: 1px 6px;
    border-radius: 3px;
  }

  .tool-section-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-faint);
    margin: 8px 0 4px;
  }

  .tool-section-label:first-child { margin-top: 0; }

  .tool-show-all { margin-top: 8px; }

  .tool-images {
    display: flex;
    flex-wrap: wrap;
    margin-top: 6px;
  }

  .workflow-agents {
    margin: 0 0 4px 16px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .workflow-agents summary {
    cursor: pointer;
    user-select: none;
  }

  .workflow-agent-list {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    margin-top: 4px;
    max-height: 320px;
    overflow-y: auto;
  }

  .tool-chevron {
    color: var(--text-faint);
    flex-shrink: 0;
  }

  .tool-pill-clickable:hover .tool-chevron {
    color: var(--text-primary);
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
    border-left: 1px solid var(--border-strong);
  }

  .tool-result-panel.tool-result-error {
    border-left-color: var(--danger);
    background: var(--danger-soft);
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
    font-size: 12px;
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
    font-size: 12px;
    color: var(--text-muted);
  }

  .error-text {
    font-size: 12px;
    color: var(--danger);
  }

  .persisted-block summary {
    font-size: 12px;
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
    color: var(--text-secondary);
    white-space: nowrap;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
    background: var(--bg-hover);
    padding: 1px 6px;
    border-radius: 3px;
  }

  .tool-summary {
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 12px;
  }

  /* A match past the ellipsis would be a mark nobody can see. */
  .tool-summary:has(:global(mark.search-mark)) {
    white-space: normal;
    overflow-wrap: anywhere;
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

  .markdown-body :global(.code-block-wrapper) {
    margin: 10px 0;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid var(--bg-elevated);
  }

  .markdown-body :global(.code-block-header) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 12px;
    background: var(--code-header-bg);
    border-bottom: 1px solid var(--code-border);
  }

  .markdown-body :global(.code-lang) {
    font-size: 12px;
    color: var(--code-muted);
    font-family: "SF Mono", "Fira Code", monospace;
    text-transform: lowercase;
  }

  .markdown-body :global(.code-copy-btn) {
    font-size: 12px;
    color: var(--code-muted);
    background: transparent;
    border: 1px solid var(--code-border);
    border-radius: 4px;
    min-height: 24px;
    padding: 2px 8px;
    cursor: pointer;
    font-family: inherit;
    transition: all 0.15s;
  }

  .markdown-body :global(.code-copy-btn:hover) {
    color: var(--code-text);
    border-color: var(--code-muted);
    background: var(--code-hover);
  }

  .markdown-body :global(.code-block-wrapper pre) {
    margin: 0;
    border: none;
    border-radius: 0;
  }

  /* Code blocks stay dark in every theme (syntax pastels need a dark ground). */
  .markdown-body :global(pre) {
    background: var(--code-bg);
    color: var(--code-text);
    border-radius: 8px;
    padding: 14px 16px;
    overflow-x: auto;
    margin: 10px 0;
    border: 1px solid var(--border);
  }

  .markdown-body :global(code) {
    font-family: "SF Mono", "Fira Code", "Cascadia Code", monospace;
    font-size: 13px;
  }

  /* Inline code is a tint of its theme. Only code blocks keep the dark surface. */
  .markdown-body :global(:not(pre) > code) {
    background: var(--inline-code-bg);
    padding: 1px 5px;
    border-radius: 4px;
    font-size: 13px;
    color: var(--inline-code-text);
    -webkit-box-decoration-break: clone;
    box-decoration-break: clone;
  }

  /* ── Syntax highlighting ── */

  .markdown-body :global(.syn-keyword) { color: var(--syn-keyword); }
  .markdown-body :global(.syn-string) { color: var(--syn-string); }
  .markdown-body :global(.syn-comment) { color: var(--syn-comment); font-style: italic; }

  /* ── Standard markdown elements ── */

  /* Long URLs and identifiers wrap instead of running off the message. break-word, not
     anywhere: anywhere also shrinks a table's columns down to one letter. */
  .markdown-body { overflow-wrap: break-word; }

  .markdown-body :global(:is(h3, h4, h5, h6)) { color: var(--text-primary); margin: 16px 0 8px 0; font-size: 14px; }

  .markdown-body :global(.md-h1) { font-size: 18px; }
  .markdown-body :global(.md-h2) { font-size: 16px; }
  .markdown-body :global(.md-h3) { font-size: 15px; }
  .markdown-body :global(strong) { color: var(--text-primary); }
  .markdown-body :global(a) { color: var(--accent-hover); }
  .markdown-body :global(.file-ref) { color: var(--accent-text); }
  .markdown-body :global(ul) { margin: 6px 0; padding-left: 22px; }
  .markdown-body :global(ol) { margin: 6px 0; padding-left: 28px; }
  .markdown-body :global(li) { margin: 4px 0; }

  .markdown-body :global(blockquote) {
    margin: 8px 0;
    padding: 2px 0 2px 12px;
    border-left: 2px solid var(--border-strong);
    color: var(--text-secondary);
  }

  .markdown-body :global(.md-image) {
    display: inline-block;
    margin: 6px 6px 6px 0;
    padding: 0;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    cursor: pointer;
  }

  .markdown-body :global(.md-image:hover) { border-color: var(--border-strong); }

  .markdown-body :global(.md-image img) {
    display: block;
    max-width: 100%;
    max-height: 360px;
  }

  .markdown-body :global(.md-image-missing) {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 5px;
    margin: 2px 0;
    padding: 3px 8px;
    background: var(--bg-hover);
    border: 1px dashed var(--border-strong);
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .markdown-body :global(.md-image-file) {
    font-family: "SF Mono", "Fira Code", monospace;
    color: var(--text-muted);
  }

  .markdown-body :global(.table-scroll) {
    margin: 10px 0;
    overflow-x: auto;
  }

  .markdown-body :global(table) {
    border-collapse: collapse;
    width: 100%;
    font-size: 13px;
  }

  .markdown-body :global(th),
  .markdown-body :global(td) {
    border: 1px solid var(--border);
    padding: 8px 12px;
    text-align: left;
  }

  .markdown-body :global(th) { background: var(--bg-elevated); color: var(--text-primary); font-weight: 600; }
  .markdown-body :global(td) { background: var(--bg-app); }
  .markdown-body :global(hr) { border: none; border-top: 1px solid var(--border); margin: 14px 0; }
</style>
