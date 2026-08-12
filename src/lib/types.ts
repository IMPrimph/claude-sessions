export interface ProjectInfo {
  project_path: string;
  project_name: string;
  short_path: string;
  session_count: number;
  last_active: string | null;
  last_active_ms: number;
}

export interface SessionInfo {
  session_id: string;
  summary: string | null;
  custom_title: string | null;
  ai_title: string | null;
  first_prompt: string | null;
  project_path: string;
  project_name: string;
  created: string | null;
  modified: string | null;
  message_count: number | null;
  conversation_count: number;
  total_tokens: number;
  git_branch: string | null;
  // Parent session id when this session was forked/branched from another.
  forked_from_session_id: string | null;
  jsonl_path: string;
}

export interface GlobalSearchResult {
  session_id: string;
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

export interface ConversationMessage {
  role: "user" | "assistant" | "compaction" | "agent-notification";
  text: string;
  timestamp: string;
  images?: MessageImage[];
  // True on an assistant reply that was cut off mid-response by the user
  // pressing Esc.
  interrupted?: boolean;
  // True on a user message sent *during* an interruption — i.e. a mid-turn
  // steering message fired while Claude was still responding.
  mid_turn?: boolean;
  // Present only when role === "agent-notification": a background subagent /
  // workflow agent finishing. Claude Code stores these as user entries, but the
  // user didn't type them — they render as a distinct card, not a bubble.
  notification?: AgentNotification;
}

// Parsed from a <task-notification> transcript entry. Covers single subagents,
// dynamic workflows, and multi-agent fan-outs (they share the same wrapper).
export interface AgentNotification {
  summary: string;
  status: string;
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
}

export interface ToolResultPayload {
  content: string;
  is_error: boolean;
  persisted_path: string | null;
}

export interface FileEditEntry {
  timestamp: string | null;
  action: "edit" | "write" | "multiedit" | "notebookedit";
  old_string: string | null;
  new_string: string | null;
  tool_use_id: string | null;
  replace_all: boolean;
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
}
