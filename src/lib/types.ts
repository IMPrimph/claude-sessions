export interface ProjectInfo {
  // Every tool with sessions in this folder.
  tools: string[];
  // False when the folder has been moved or deleted; its history is still here.
  folder_exists: boolean;
  project_path: string;
  project_name: string;
  short_path: string;
  session_count: number;
  last_active: string | null;
  last_active_ms: number;
}

export interface SessionInfo {
  session_id: string;
  // Which agent wrote this session: "claude" or "codex".
  tool: string;
  // Archived inside the tool itself, which is different from saved in Evertusk.
  archived_in_tool: boolean;
  summary: string | null;
  custom_title: string | null;
  ai_title: string | null;
  first_prompt: string | null;
  project_path: string;
  project_name: string;
  created: string | null;
  // Time of the last real message.
  modified: string | null;
  // The file's own write time, used only for "Now" and New activity.
  last_written: string | null;
  conversation_count: number;
  git_branch: string | null;
  // Parent session id when this session was forked/branched from another.
  forked_from_session_id: string | null;
  jsonl_path: string;
}

export interface GlobalSearchResult {
  session_id: string;
  tool: string;
  project_name: string;
  project_path: string;
  session_name: string;
  matched_text: string;
  match_source: "session_name" | "message";
  timestamp: string | null;
  jsonl_path: string;
}

export interface MessageImage {
  number: number;
  data_url: string;
}

export type MessageBlock =
  | { kind: "text"; text: string }
  | { kind: "thinking"; text: string }
  | {
      kind: "tool";
      name: string;
      summary: string;
      // Human-readable input, capped. Empty when the summary already says it all.
      input: string;
      // The cap cut the input; get_tool_input returns all of it.
      input_truncated?: boolean;
      tool_use_id?: string;
      // Agent id (Agent calls) or workflow run id (Workflow calls).
      agent_id?: string;
    }
  | { kind: "plan"; items: PlanItem[] }
  // A labelled aside: the end-of-turn recap or a chapter marker.
  | { kind: "note"; label: string; text: string }
  // Hook output during a turn.
  | { kind: "hook"; label: string; text: string; is_error: boolean };

export interface PlanItem {
  content: string;
  status: string;
}

export interface ShellRun {
  command: string;
  stdout: string;
  stderr: string;
}

export interface SystemEvent {
  // "fork" marks where a fork's own work starts.
  kind: "hook" | "api-error" | "recap" | "command" | "files" | "notice" | "fork";
  is_error?: boolean;
  // Longer content shown collapsed under the message's one-line text.
  detail?: string;
}

export interface ConversationMessage {
  role: "user" | "assistant" | "compaction" | "agent-notification" | "shell" | "system-event";
  // Plain searchable text. For assistant turns this is derived from `blocks`.
  text: string;
  timestamp: string;
  // The ordered content of an assistant turn.
  blocks?: MessageBlock[];
  images?: MessageImage[];
  // True on an assistant reply that was cut off mid-response by the user
  // pressing Esc.
  interrupted?: boolean;
  // True on a user message sent *during* an interruption — i.e. a mid-turn
  // steering message fired while Claude was still responding.
  mid_turn?: boolean;
  // A background task finishing or a subagent reporting back. Claude Code stores
  // these as user entries, but the user didn't type them, so they render as a card.
  notification?: AgentNotification;
  // A command the user ran with `!`.
  shell?: ShellRun;
  // Hooks, API errors, recaps and notices.
  event?: SystemEvent;
  // On a compaction: its trigger and token counts.
  compaction_detail?: string;
  // Copied from the parent session when this one was forked.
  inherited?: boolean;
}

// A background task finishing or a subagent reporting back. Covers subagents,
// background shells, Monitor events, dynamic workflows and multi-agent fan-outs.
export interface AgentNotification {
  summary: string;
  status: string;
  kind: "agent" | "monitor" | "shell" | "hand-back" | "message" | "task";
  result?: string;
  tokens?: number;
  tool_uses?: number;
  duration_ms?: number;
  agent_count?: number;
  agents_done?: number;
  agents_error?: number;
}

export interface ToolCount {
  name: string;
  count: number;
}

export interface SubagentInfo {
  agent_id: string;
  agent_type: string | null;
  description: string | null;
  jsonl_path: string;
  tool_use_id: string | null;
  // The workflow run this agent belongs to, for agents a Workflow call spawned.
  workflow_run?: string | null;
}

export interface ToolResultPayload {
  content: string;
  is_error: boolean;
  persisted_path: string | null;
  // Images in this result, fetched on demand with get_tool_result_images.
  image_count: number;
}

export interface FileEditEntry {
  timestamp: string | null;
  action: "edit" | "write" | "delete" | "multiedit" | "notebookedit" | "bash";
  old_string: string | null;
  new_string: string | null;
  tool_use_id: string | null;
  replace_all: boolean;
  // The edit was rejected, so it stays listed but wasn't applied.
  failed: boolean;
}

export interface FileChange {
  path: string;
  display_path: string;
  edits: FileEditEntry[];
  edit_count: number;
  read_count: number;
}

export interface AnsweredOption {
  label: string;
  description: string;
  chosen: boolean;
  // True when the user picked "Other" and typed a custom answer (not an offered option).
  custom: boolean;
}

export interface AnsweredQuestion {
  question: string;
  header: string;
  multi_select: boolean;
  options: AnsweredOption[];
  // Free-text note the user attached (sometimes the note is the whole answer).
  notes?: string | null;
}

export interface SessionArtifact {
  url: string;
  title: string;
  path: string;
}

export interface Bookmark {
  id: string;
  role: "user" | "assistant";
  text: string;
  preview: string;
  project_path: string;
  project_name: string;
  session_id: string;
  jsonl_path: string;
  timestamp: string;
  created_at: number;
}

export interface SessionStats {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  assistant_count: number;
  user_prompt_count: number;
  thinking_block_count: number;
  models: string[];
  tool_counts: ToolCount[];
  hooks: HookStat[];
  // Time Claude spent working, summed from turn durations (excludes idle time).
  active_ms: number;
  cost_usd?: number;
  lines_added?: number;
  lines_removed?: number;
  // The record covers only part of the session, so these are lower bounds.
  cost_is_partial: boolean;
  active_is_partial: boolean;
  // Subagents' own tokens, kept out of the main-thread numbers above.
  agent_output_tokens: number;
  agent_input_tokens: number;
  // Prompts copied from the parent of a fork, excluded from user_prompt_count.
  inherited_prompts: number;
}

export interface HookStat {
  event: string;
  runs: number;
  failures: number;
}
