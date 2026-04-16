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
  first_prompt: string | null;
  project_path: string;
  project_name: string;
  created: string | null;
  modified: string | null;
  message_count: number | null;
  conversation_count: number;
  total_tokens: number;
  git_branch: string | null;
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
  role: "user" | "assistant" | "compaction";
  text: string;
  timestamp: string;
  images?: MessageImage[];
}
