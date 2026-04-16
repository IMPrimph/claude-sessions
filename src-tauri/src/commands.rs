use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

// ── Types returned to the frontend ──────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct ProjectInfo {
    pub project_path: String,
    pub project_name: String,
    pub short_path: String,
    pub session_count: u64,
    pub last_active: Option<String>,
    pub last_active_ms: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub summary: Option<String>,
    pub first_prompt: Option<String>,
    pub project_path: String,
    pub project_name: String,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub message_count: Option<u64>,
    pub conversation_count: u64,
    pub total_tokens: u64,
    pub git_branch: Option<String>,
    pub jsonl_path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ConversationMessage {
    pub role: String, // "user" or "assistant"
    pub text: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<MessageImage>,
}

#[derive(Debug, Serialize, Clone)]
pub struct MessageImage {
    pub number: u32,
    pub data_url: String,
}

// ── Internal types for parsing JSONL ────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SessionIndexFile {
    entries: Vec<SessionIndexEntry>,
    #[serde(rename = "originalPath")]
    original_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionIndexEntry {
    #[serde(rename = "sessionId")]
    session_id: String,
    summary: Option<String>,
    #[serde(rename = "firstPrompt")]
    first_prompt: Option<String>,
    created: Option<String>,
    modified: Option<String>,
    #[serde(rename = "messageCount")]
    message_count: Option<u64>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonlEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    #[serde(rename = "isMeta")]
    is_meta: Option<bool>,
    #[serde(rename = "toolUseResult")]
    tool_use_result: Option<Value>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "isCompactSummary")]
    is_compact_summary: Option<bool>,
    message: Option<JsonlMessage>,
    timestamp: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonlMessage {
    content: Option<Value>,
    model: Option<String>,
}

// ── Commands ────────────────────────────────────────────────────────────────

/// Returns the absolute path to a session's pasted image, if it exists.
/// Claude Code caches pasted images at ~/.claude/image-cache/<session_id>/<N>.<ext>
#[tauri::command]
pub fn get_image_path(session_id: String, image_number: u32) -> Option<String> {
    let home = dirs::home_dir()?;
    let base = home
        .join(".claude")
        .join("image-cache")
        .join(&session_id);

    for extension in ["png", "jpg", "jpeg", "gif", "webp"] {
        let path = base.join(format!("{}.{}", image_number, extension));
        if path.exists() {
            return Some(path.to_string_lossy().to_string());
        }
    }
    None
}

#[tauri::command]
pub fn get_projects() -> Result<Vec<ProjectInfo>, String> {
    let claude_dir = get_claude_projects_dir()?;
    let mut projects: Vec<ProjectInfo> = Vec::new();
    let home = dirs::home_dir()
        .map(|home| home.to_string_lossy().to_string())
        .unwrap_or_default();

    let project_dirs = fs::read_dir(&claude_dir)
        .map_err(|read_error| format!("Cannot read {:?}: {}", claude_dir, read_error))?;

    for project_entry in project_dirs.flatten() {
        let project_dir = project_entry.path();
        if !project_dir.is_dir() {
            continue;
        }

        let dir_name = project_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Resolve the original path from index or decode from dir name
        let original_path = resolve_project_path(&project_dir, &dir_name);

        let project_name = original_path
            .split('/')
            .last()
            .unwrap_or(&original_path)
            .to_string();

        // Count actual JSONL files and find most recent mtime
        let mut session_count: u64 = 0;
        let mut latest_mtime_ms: u64 = 0;

        if let Ok(files) = fs::read_dir(&project_dir) {
            for file_entry in files.flatten() {
                let file_path = file_entry.path();
                if file_path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                    session_count += 1;
                    if let Ok(metadata) = file_path.metadata() {
                        if let Ok(modified_time) = metadata.modified() {
                            if let Ok(duration) =
                                modified_time.duration_since(std::time::UNIX_EPOCH)
                            {
                                let mtime_ms = duration.as_millis() as u64;
                                if mtime_ms > latest_mtime_ms {
                                    latest_mtime_ms = mtime_ms;
                                }
                            }
                        }
                    }
                }
            }
        }

        if session_count == 0 {
            continue;
        }

        let short_path = if original_path.starts_with(&home) {
            format!("~{}", &original_path[home.len()..])
        } else {
            original_path.clone()
        };

        // Convert mtime to a relative "X ago" string
        let last_active = if latest_mtime_ms > 0 {
            Some(format_relative_time(latest_mtime_ms))
        } else {
            None
        };

        projects.push(ProjectInfo {
            project_path: original_path,
            project_name,
            short_path,
            session_count,
            last_active,
            last_active_ms: latest_mtime_ms,
        });
    }

    // Sort by last active timestamp, most recent first
    projects.sort_by(|project_a, project_b| {
        project_b.last_active_ms.cmp(&project_a.last_active_ms)
    });

    Ok(projects)
}

fn resolve_project_path(project_dir: &PathBuf, dir_name: &str) -> String {
    let index_path = project_dir.join("sessions-index.json");
    if index_path.exists() {
        if let Ok(content) = fs::read_to_string(&index_path) {
            if let Ok(data) = serde_json::from_str::<SessionIndexFile>(&content) {
                if let Some(path) = data.original_path {
                    return path;
                }
            }
        }
    }
    decode_project_path(dir_name)
}

fn format_relative_time(mtime_ms: u64) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);

    if now_ms <= mtime_ms {
        return "just now".to_string();
    }

    let diff_secs = (now_ms - mtime_ms) / 1000;
    let diff_mins = diff_secs / 60;
    let diff_hours = diff_mins / 60;
    let diff_days = diff_hours / 24;

    if diff_mins < 1 {
        "just now".to_string()
    } else if diff_mins == 1 {
        "1 minute ago".to_string()
    } else if diff_mins < 60 {
        format!("{} minutes ago", diff_mins)
    } else if diff_hours == 1 {
        "1 hour ago".to_string()
    } else if diff_hours < 24 {
        format!("{} hours ago", diff_hours)
    } else if diff_days == 1 {
        "yesterday".to_string()
    } else {
        format!("{} days ago", diff_days)
    }
}

#[tauri::command]
pub fn scan_projects(project_path: Option<String>) -> Result<Vec<SessionInfo>, String> {
    let claude_dir = get_claude_projects_dir()?;
    let mut sessions: Vec<SessionInfo> = Vec::new();

    let project_dirs = fs::read_dir(&claude_dir)
        .map_err(|read_error| format!("Cannot read {:?}: {}", claude_dir, read_error))?;

    for project_entry in project_dirs.flatten() {
        let project_dir = project_entry.path();
        if !project_dir.is_dir() {
            continue;
        }

        let dir_name = project_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // If filtering by project, check if this directory matches
        if let Some(ref filter_path) = project_path {
            let resolved = resolve_project_path(&project_dir, &dir_name);
            if &resolved != filter_path {
                continue;
            }
        }

        // Try reading sessions-index.json first (fast path)
        let index_path = project_dir.join("sessions-index.json");
        if index_path.exists() {
            if let Ok(index_content) = fs::read_to_string(&index_path) {
                if let Ok(index_data) = serde_json::from_str::<SessionIndexFile>(&index_content) {
                    let original_path = index_data
                        .original_path
                        .unwrap_or_else(|| decode_project_path(&dir_name));

                    let project_name = original_path
                        .split('/')
                        .last()
                        .unwrap_or(&original_path)
                        .to_string();

                    for entry in index_data.entries {
                        // Always construct path from project dir + session ID
                        // because fullPath in the index is often stale
                        let jsonl_pathbuf = project_dir
                            .join(format!("{}.jsonl", entry.session_id));
                        let jsonl_path = jsonl_pathbuf.to_string_lossy().to_string();

                        // Skip sessions whose JSONL files no longer exist
                        if !jsonl_pathbuf.exists() {
                            continue;
                        }

                        // Prefer file mtime over index modified (index can be stale)
                        let file_modified = get_file_mtime_iso(&jsonl_pathbuf);

                        sessions.push(SessionInfo {
                            session_id: entry.session_id,
                            summary: entry.summary,
                            first_prompt: entry.first_prompt,
                            project_path: original_path.clone(),
                            project_name: project_name.clone(),
                            created: entry.created,
                            modified: file_modified.or(entry.modified),
                            message_count: entry.message_count,
                            conversation_count: 0,
                            total_tokens: 0,
                            git_branch: entry.git_branch,
                            jsonl_path,
                        });
                    }

                    // Also pick up any JSONL files not in the index
                    if let Ok(files) = fs::read_dir(&project_dir) {
                        let indexed_ids: std::collections::HashSet<String> = sessions
                            .iter()
                            .filter(|session| session.project_path == original_path)
                            .map(|session| session.session_id.clone())
                            .collect();

                        for file_entry in files.flatten() {
                            let file_path = file_entry.path();
                            if file_path.extension().and_then(|ext| ext.to_str()) != Some("jsonl")
                            {
                                continue;
                            }
                            let session_id = file_path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            if indexed_ids.contains(&session_id) {
                                continue;
                            }

                            let metadata = extract_quick_metadata(&file_path);
                            let file_modified = get_file_mtime_iso(&file_path);

                            sessions.push(SessionInfo {
                                session_id,
                                summary: metadata.custom_title,
                                first_prompt: metadata.first_prompt,
                                project_path: original_path.clone(),
                                project_name: project_name.clone(),
                                created: metadata.first_timestamp,
                                modified: file_modified.or(metadata.last_timestamp),
                                message_count: None,
                                conversation_count: metadata.conversation_count,
                                total_tokens: metadata.total_tokens,
                                git_branch: None,
                                jsonl_path: file_path.to_string_lossy().to_string(),
                            });
                        }
                    }
                    continue;
                }
            }
        }

        // Fallback: scan for .jsonl files directly
        let decoded_path = decode_project_path(&dir_name);
        let project_name = decoded_path
            .split('/')
            .last()
            .unwrap_or(&decoded_path)
            .to_string();

        if let Ok(files) = fs::read_dir(&project_dir) {
            for file_entry in files.flatten() {
                let file_path = file_entry.path();
                if file_path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
                    let session_id = file_path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    let metadata = extract_quick_metadata(&file_path);
                    let file_modified = get_file_mtime_iso(&file_path);

                    sessions.push(SessionInfo {
                        session_id,
                        summary: metadata.custom_title,
                        first_prompt: metadata.first_prompt,
                        project_path: decoded_path.clone(),
                        project_name: project_name.clone(),
                        created: metadata.first_timestamp,
                        modified: file_modified.or(metadata.last_timestamp),
                        message_count: None,
                        conversation_count: metadata.conversation_count,
                        total_tokens: metadata.total_tokens,
                        git_branch: None,
                        jsonl_path: file_path.to_string_lossy().to_string(),
                    });
                }
            }
        }
    }

    // Sort by modified date, most recent first
    sessions.sort_by(|session_a, session_b| {
        session_b
            .modified
            .as_deref()
            .unwrap_or("")
            .cmp(session_a.modified.as_deref().unwrap_or(""))
    });

    Ok(sessions)
}

#[derive(Debug, Serialize, Clone)]
pub struct GlobalSearchResult {
    pub session_id: String,
    pub project_name: String,
    pub project_path: String,
    pub session_name: String,
    pub matched_text: String,
    pub match_source: String, // "session_name", "message"
    pub timestamp: Option<String>,
    pub jsonl_path: String,
}

#[tauri::command]
pub fn global_search(query: String) -> Result<Vec<GlobalSearchResult>, String> {
    let claude_dir = get_claude_projects_dir()?;
    let mut results: Vec<GlobalSearchResult> = Vec::new();
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();

    if query_words.is_empty() {
        return Ok(results);
    }

    let project_dirs = fs::read_dir(&claude_dir)
        .map_err(|read_error| format!("Cannot read {:?}: {}", claude_dir, read_error))?;

    for project_entry in project_dirs.flatten() {
        let project_dir = project_entry.path();
        if !project_dir.is_dir() {
            continue;
        }

        let dir_name = project_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let original_path = resolve_project_path(&project_dir, &dir_name);
        let project_name = original_path
            .split('/')
            .last()
            .unwrap_or(&original_path)
            .to_string();

        let files = match fs::read_dir(&project_dir) {
            Ok(files) => files,
            Err(_) => continue,
        };

        for file_entry in files.flatten() {
            let file_path = file_entry.path();
            if file_path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }

            let session_id = file_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let metadata = extract_quick_metadata(&file_path);
            let session_name = metadata
                .custom_title
                .clone()
                .or(metadata.first_prompt.clone())
                .unwrap_or_else(|| session_id.clone());

            // Check if session name matches
            let name_lower = session_name.to_lowercase();
            if query_words.iter().all(|word| name_lower.contains(word)) {
                // For name matches, use first_prompt as the preview context (falls back
                // to empty string so the frontend can hide the preview row cleanly).
                let preview = metadata
                    .first_prompt
                    .clone()
                    .filter(|prompt| prompt != &session_name)
                    .unwrap_or_default();
                results.push(GlobalSearchResult {
                    session_id: session_id.clone(),
                    project_name: project_name.clone(),
                    project_path: original_path.clone(),
                    session_name: session_name.chars().take(120).collect(),
                    matched_text: preview.chars().take(200).collect(),
                    match_source: "session_name".to_string(),
                    timestamp: metadata.first_timestamp.clone(),
                    jsonl_path: file_path.to_string_lossy().to_string(),
                });
                continue; // Don't also search messages if name matched
            }

            // Search message content (only user and assistant text blocks)
            let file = match fs::File::open(&file_path) {
                Ok(file) => file,
                Err(_) => continue,
            };
            let reader = BufReader::new(file);
            let mut found_in_session = false;

            for line in reader.lines().flatten() {
                if found_in_session {
                    break;
                }
                // Fast pre-check before JSON parsing
                let line_lower = line.to_lowercase();
                if !query_words.iter().all(|word| line_lower.contains(word)) {
                    continue;
                }

                if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
                    let entry_type = match &entry.entry_type {
                        Some(entry_type) => entry_type.as_str(),
                        None => continue,
                    };
                    if entry.is_sidechain.unwrap_or(false) {
                        continue;
                    }
                    if entry_type == "user" && entry.tool_use_result.is_none() {
                        let text = extract_user_text(&entry.message);
                        let text_lower = text.to_lowercase();
                        if query_words.iter().all(|word| text_lower.contains(word)) {
                            results.push(GlobalSearchResult {
                                session_id: session_id.clone(),
                                project_name: project_name.clone(),
                                project_path: original_path.clone(),
                                session_name: session_name.chars().take(120).collect(),
                                matched_text: text.chars().take(200).collect(),
                                match_source: "message".to_string(),
                                timestamp: entry.timestamp,
                                jsonl_path: file_path.to_string_lossy().to_string(),
                            });
                            found_in_session = true;
                        }
                    } else if entry_type == "assistant" {
                        let text = extract_assistant_text(&entry.message.as_ref().and_then(|msg| msg.content.clone()));
                        let text_lower = text.to_lowercase();
                        if query_words.iter().all(|word| text_lower.contains(word)) {
                            results.push(GlobalSearchResult {
                                session_id: session_id.clone(),
                                project_name: project_name.clone(),
                                project_path: original_path.clone(),
                                session_name: session_name.chars().take(120).collect(),
                                matched_text: text.chars().take(200).collect(),
                                match_source: "message".to_string(),
                                timestamp: entry.timestamp,
                                jsonl_path: file_path.to_string_lossy().to_string(),
                            });
                            found_in_session = true;
                        }
                    }
                }
            }
        }
    }

    // Sort by timestamp, most recent first
    results.sort_by(|result_a, result_b| {
        result_b
            .timestamp
            .as_deref()
            .unwrap_or("")
            .cmp(result_a.timestamp.as_deref().unwrap_or(""))
    });

    // Limit to 50 results
    results.truncate(50);

    Ok(results)
}

#[tauri::command]
pub fn get_session_tokens(jsonl_path: String) -> Result<u64, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }

    let file = fs::File::open(&path)
        .map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut token_by_request: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();

    for line in reader.lines().flatten() {
        if !line.contains("\"type\":\"assistant\"") || !line.contains("\"usage\"") {
            continue;
        }
        if let Ok(raw) = serde_json::from_str::<Value>(&line) {
            if let Some(usage) = raw.get("message").and_then(|msg| msg.get("usage")) {
                let input = usage.get("input_tokens").and_then(|val| val.as_u64()).unwrap_or(0);
                let output = usage.get("output_tokens").and_then(|val| val.as_u64()).unwrap_or(0);
                if let Some(request_id) = raw.get("requestId").and_then(|val| val.as_str()) {
                    token_by_request.insert(request_id.to_string(), input + output);
                }
            }
        }
    }

    Ok(token_by_request.values().sum())
}

#[tauri::command]
pub fn get_session_messages(jsonl_path: String) -> Result<Vec<ConversationMessage>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }

    let file =
        fs::File::open(&path).map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut messages: Vec<ConversationMessage> = Vec::new();
    let mut current_assistant_text = String::new();
    let mut current_assistant_timestamp = String::new();
    let mut in_assistant_turn = false;

    // Streaming dedup: Claude Code writes multiple JSONL entries per API response
    // with the same requestId, each superseding the previous one. Buffer the most
    // recent assistant entry per requestId and commit it when a new requestId
    // (or any non-assistant entry) appears.
    let mut pending_assistant: Option<JsonlEntry> = None;

    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let entry: JsonlEntry = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        if should_skip_entry(&entry) {
            continue;
        }

        let entry_type = match &entry.entry_type {
            Some(entry_type) => entry_type.as_str(),
            None => continue,
        };

        // Assistant entries: maybe buffer for dedup, maybe commit previous pending
        if entry_type == "assistant" {
            if let (Some(ref pending), Some(ref current_rid)) =
                (pending_assistant.as_ref(), entry.request_id.as_ref())
            {
                if pending.request_id.as_ref() == Some(current_rid) {
                    // Same streaming response — replace the pending entry
                    pending_assistant = Some(entry);
                    continue;
                }
            }
            // Different requestId (or no requestId on either side) — commit
            // the old pending entry into the accumulator before buffering the new one.
            if let Some(previous) = pending_assistant.take() {
                accumulate_assistant(
                    previous,
                    &mut current_assistant_text,
                    &mut current_assistant_timestamp,
                    &mut in_assistant_turn,
                );
            }
            pending_assistant = Some(entry);
            continue;
        }

        // Non-assistant entry: commit any pending assistant first
        if let Some(previous) = pending_assistant.take() {
            accumulate_assistant(
                previous,
                &mut current_assistant_text,
                &mut current_assistant_timestamp,
                &mut in_assistant_turn,
            );
        }

        if entry_type == "user" {
            process_user_entry(
                entry,
                &mut messages,
                &mut current_assistant_text,
                &mut current_assistant_timestamp,
                &mut in_assistant_turn,
            );
        }
        // Other entry types (system/summary/etc.) are already filtered by should_skip_entry
    }

    // End of file — commit any remaining pending assistant, then flush the turn
    if let Some(final_entry) = pending_assistant.take() {
        accumulate_assistant(
            final_entry,
            &mut current_assistant_text,
            &mut current_assistant_timestamp,
            &mut in_assistant_turn,
        );
    }
    flush_assistant(
        &mut messages,
        &mut current_assistant_text,
        &mut current_assistant_timestamp,
        &mut in_assistant_turn,
    );

    Ok(messages)
}

fn should_skip_entry(entry: &JsonlEntry) -> bool {
    if entry.is_sidechain.unwrap_or(false) {
        return true;
    }
    if let Some(ref message) = entry.message {
        if message.model.as_deref() == Some("<synthetic>") {
            return true;
        }
    }
    matches!(
        entry.entry_type.as_deref(),
        Some("system") | Some("summary") | Some("file-history-snapshot") | Some("queue-operation")
    )
}

fn accumulate_assistant(
    entry: JsonlEntry,
    current_text: &mut String,
    current_timestamp: &mut String,
    in_turn: &mut bool,
) {
    if !*in_turn {
        *in_turn = true;
        *current_timestamp = entry.timestamp.clone().unwrap_or_default();
    }
    if let Some(message) = &entry.message {
        let text_parts = extract_assistant_text(&message.content);
        if !text_parts.is_empty() {
            if !current_text.is_empty() {
                current_text.push_str("\n\n");
            }
            current_text.push_str(&text_parts);
        }
    }
}

fn process_user_entry(
    entry: JsonlEntry,
    messages: &mut Vec<ConversationMessage>,
    current_assistant_text: &mut String,
    current_assistant_timestamp: &mut String,
    in_assistant_turn: &mut bool,
) {
    // Compaction summaries are special
    if entry.is_compact_summary.unwrap_or(false) {
        flush_assistant(messages, current_assistant_text, current_assistant_timestamp, in_assistant_turn);
        let text = extract_user_text(&entry.message);
        if !text.is_empty() {
            messages.push(ConversationMessage {
                role: "compaction".to_string(),
                text,
                timestamp: entry.timestamp.unwrap_or_default(),
                images: Vec::new(),
            });
        }
        return;
    }

    // Skip meta and tool-result entries
    if entry.is_meta.unwrap_or(false) || entry.tool_use_result.is_some() {
        return;
    }
    if is_tool_result_content(&entry.message) {
        return;
    }

    let text = extract_user_text(&entry.message);
    let cleaned = strip_system_tags(&text);
    let images = extract_user_images(&entry.message, &cleaned);

    if cleaned.is_empty() && images.is_empty() {
        return;
    }

    flush_assistant(messages, current_assistant_text, current_assistant_timestamp, in_assistant_turn);

    messages.push(ConversationMessage {
        role: "user".to_string(),
        text: cleaned,
        timestamp: entry.timestamp.unwrap_or_default(),
        images,
    });
}

fn flush_assistant(
    messages: &mut Vec<ConversationMessage>,
    current_text: &mut String,
    current_timestamp: &mut String,
    in_turn: &mut bool,
) {
    if *in_turn && !current_text.is_empty() {
        messages.push(ConversationMessage {
            role: "assistant".to_string(),
            text: current_text.clone(),
            timestamp: current_timestamp.clone(),
            images: Vec::new(),
        });
        current_text.clear();
        *in_turn = false;
    }
}

/// Extract image content blocks from a user message, pairing them positionally
/// with `[Image #N]` text references. Returns a list of (number, data_url) pairs.
fn extract_user_images(message: &Option<JsonlMessage>, text: &str) -> Vec<MessageImage> {
    let msg = match message {
        Some(msg) => msg,
        None => return Vec::new(),
    };
    let blocks = match &msg.content {
        Some(Value::Array(blocks)) => blocks,
        _ => return Vec::new(),
    };

    // Collect image blocks in order
    let mut image_blocks: Vec<&Value> = Vec::new();
    for block in blocks {
        if block.get("type").and_then(|block_type| block_type.as_str()) == Some("image") {
            image_blocks.push(block);
        }
    }

    if image_blocks.is_empty() {
        return Vec::new();
    }

    // Find [Image #N] references in the text, in order of appearance
    let mut text_refs: Vec<u32> = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find("[Image #") {
        let start = cursor + found + "[Image #".len();
        if let Some(end_offset) = text[start..].find(']') {
            if let Ok(number) = text[start..start + end_offset].parse::<u32>() {
                text_refs.push(number);
            }
            cursor = start + end_offset + 1;
        } else {
            break;
        }
    }

    // Extras (image blocks without a matching text reference) are numbered
    // starting after the highest referenced number, so they can't collide.
    let extras_base = text_refs.iter().copied().max().unwrap_or(0);

    // Pair image blocks with text references positionally
    let mut images: Vec<MessageImage> = Vec::new();
    for (image_index, block) in image_blocks.iter().enumerate() {
        let source = match block.get("source") {
            Some(source) => source,
            None => continue,
        };
        let media_type = source
            .get("media_type")
            .and_then(|val| val.as_str())
            .unwrap_or("image/png");
        let data = match source.get("data").and_then(|val| val.as_str()) {
            Some(data) => data,
            None => continue,
        };

        let number = match text_refs.get(image_index).copied() {
            Some(referenced) => referenced,
            None => extras_base + (image_index - text_refs.len() + 1) as u32,
        };

        images.push(MessageImage {
            number,
            data_url: format!("data:{};base64,{}", media_type, data),
        });
    }

    images
}

/// Check if user message content is an array containing tool_result blocks
/// (these are tool responses, not real user input)
fn is_tool_result_content(message: &Option<JsonlMessage>) -> bool {
    match message {
        Some(msg) => match &msg.content {
            Some(Value::Array(blocks)) => {
                blocks.iter().any(|block| {
                    block.get("type").and_then(|block_type| block_type.as_str())
                        == Some("tool_result")
                })
            }
            _ => false,
        },
        None => false,
    }
}

/// Strip system/meta tags and ANSI escape sequences that shouldn't be displayed
fn strip_system_tags(text: &str) -> String {
    const DROP_TAGS: &[&str] = &[
        "system-reminder",
        "local-command-caveat",
        "local-command-stdout",
        "local-command-stderr",
        "command-args",
    ];
    const UNWRAP_TAGS: &[&str] = &["command-name", "command-message"];

    let mut result = strip_paired_tags(text, DROP_TAGS, false);
    result = strip_paired_tags(&result, UNWRAP_TAGS, true);
    strip_ansi(&result).trim().to_string()
}

/// Remove `<tag>...</tag>` blocks. If `keep_inner` is true, the inner text is kept.
/// Scans left-to-right so mismatched tags are handled correctly and allocates once.
fn strip_paired_tags(text: &str, tags: &[&str], keep_inner: bool) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;

    while cursor < text.len() {
        let mut earliest: Option<(usize, &str)> = None;
        for tag in tags {
            let open = format!("<{}>", tag);
            if let Some(found) = text[cursor..].find(&open) {
                let abs = cursor + found;
                if earliest.map_or(true, |(earlier, _)| abs < earlier) {
                    earliest = Some((abs, *tag));
                }
            }
        }

        match earliest {
            Some((open_start, tag)) => {
                output.push_str(&text[cursor..open_start]);
                let open_len = tag.len() + 2; // <tag>
                let inner_start = open_start + open_len;
                let close = format!("</{}>", tag);
                match text[inner_start..].find(&close) {
                    Some(close_offset) => {
                        if keep_inner {
                            output.push_str(&text[inner_start..inner_start + close_offset]);
                        }
                        cursor = inner_start + close_offset + close.len();
                    }
                    None => {
                        // Unclosed — drop everything from the open tag onward
                        return output;
                    }
                }
            }
            None => {
                output.push_str(&text[cursor..]);
                break;
            }
        }
    }

    output
}

/// Strip ANSI CSI escape sequences (e.g. `\x1b[2m`, `\x1b[22m`, `\x1b[0m`).
fn strip_ansi(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == 0x1b && cursor + 1 < bytes.len() && bytes[cursor + 1] == b'[' {
            cursor += 2;
            while cursor < bytes.len() && !bytes[cursor].is_ascii_alphabetic() {
                cursor += 1;
            }
            if cursor < bytes.len() {
                cursor += 1;
            }
        } else {
            output.push(bytes[cursor] as char);
            cursor += 1;
        }
    }
    output
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn get_claude_projects_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let claude_projects = home.join(".claude").join("projects");
    if !claude_projects.exists() {
        return Err(format!(
            "Claude projects directory not found: {:?}",
            claude_projects
        ));
    }
    Ok(claude_projects)
}

fn get_file_mtime_iso(path: &PathBuf) -> Option<String> {
    let metadata = path.metadata().ok()?;
    let modified = metadata.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Utc> = modified.into();
    Some(datetime.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// Decode Claude Code's encoded project directory name back to a filesystem path.
/// `-Users-vishnu-Documents-my-project` could be `/Users/vishnu/Documents/my-project`
/// or `/Users/vishnu/Documents/my/project` — project names with dashes are ambiguous.
/// Try progressively fewer dash→slash substitutions from the right, preferring the
/// first candidate that exists on disk.
fn decode_project_path(encoded: &str) -> String {
    if !encoded.starts_with('-') {
        return encoded.replace('-', "/");
    }

    let without_leading = &encoded[1..];
    let dash_positions: Vec<usize> = without_leading
        .char_indices()
        .filter(|(_, character)| *character == '-')
        .map(|(index, _)| index)
        .collect();

    // Try candidates from most slashes (all dashes → slashes) down to just the root slash.
    for split_count in (0..=dash_positions.len()).rev() {
        let mut candidate = String::with_capacity(encoded.len());
        candidate.push('/');
        let mut previous = 0;
        for &position in dash_positions.iter().take(split_count) {
            candidate.push_str(&without_leading[previous..position]);
            candidate.push('/');
            previous = position + 1;
        }
        candidate.push_str(&without_leading[previous..]);

        if PathBuf::from(&candidate).exists() {
            return candidate;
        }
    }

    // Nothing on disk matched — fall back to the original aggressive decode.
    format!("/{}", without_leading.replace('-', "/"))
}

fn extract_user_text(message: &Option<JsonlMessage>) -> String {
    match message {
        Some(msg) => match &msg.content {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Array(blocks)) => {
                let mut parts = Vec::new();
                for block in blocks {
                    if let Some(text) = block.get("text").and_then(|text| text.as_str()) {
                        if block.get("type").and_then(|block_type| block_type.as_str()) == Some("text") {
                            parts.push(text.to_string());
                        }
                    }
                }
                parts.join("\n")
            }
            _ => String::new(),
        },
        None => String::new(),
    }
}

fn extract_assistant_text(content: &Option<Value>) -> String {
    match content {
        Some(Value::Array(blocks)) => {
            let mut parts = Vec::new();
            for block in blocks {
                if let Some(block_type) = block.get("type").and_then(|block_type| block_type.as_str()) {
                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|text| text.as_str()) {
                                parts.push(text.to_string());
                            }
                        }
                        "tool_use" => {
                            if let Some(formatted) = format_tool_use(block) {
                                parts.push(formatted);
                            }
                        }
                        "thinking" => {
                            if let Some(thinking) = block.get("thinking").and_then(|val| val.as_str()) {
                                if !thinking.is_empty() {
                                    // Escape any accidental marker sequences in thinking content
                                    let safe_content = thinking.replace("{{THINKING_END}}", "");
                                    parts.push(format!("{{{{THINKING_START}}}}\n{}\n{{{{THINKING_END}}}}", safe_content));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            parts.join("\n\n")
        }
        _ => String::new(),
    }
}

fn format_tool_use(block: &Value) -> Option<String> {
    let tool_name = block.get("name").and_then(|name| name.as_str())?;
    let input = block.get("input")?;

    let summary = match tool_name {
        "Read" | "read" => {
            let path = input.get("file_path").and_then(|path| path.as_str()).unwrap_or("unknown");
            path.to_string()
        }
        "Write" | "write" => {
            let path = input.get("file_path").and_then(|path| path.as_str()).unwrap_or("unknown");
            path.to_string()
        }
        "Edit" | "edit" => {
            let path = input.get("file_path").and_then(|path| path.as_str()).unwrap_or("unknown");
            path.to_string()
        }
        "Bash" | "bash" => {
            let command = input.get("command").and_then(|cmd| cmd.as_str()).unwrap_or("");
            let truncated: String = command.chars().take(200).collect();
            truncated
        }
        "Grep" | "grep" => {
            let pattern = input.get("pattern").and_then(|pat| pat.as_str()).unwrap_or("");
            let path = input.get("path").and_then(|path| path.as_str()).unwrap_or(".");
            format!("{} in {}", pattern, path)
        }
        "Glob" | "glob" => {
            let pattern = input.get("pattern").and_then(|pat| pat.as_str()).unwrap_or("");
            pattern.to_string()
        }
        "Agent" | "agent" => {
            let description = input.get("description").and_then(|desc| desc.as_str()).unwrap_or("subagent");
            description.to_string()
        }
        "TaskCreate" | "TaskUpdate" | "TaskGet" | "TaskList" => {
            let subject = input.get("subject").and_then(|subj| subj.as_str()).unwrap_or("");
            subject.to_string()
        }
        "Skill" | "skill" => {
            let skill_name = input.get("skill").and_then(|skill| skill.as_str()).unwrap_or("");
            skill_name.to_string()
        }
        _ => String::new(),
    };

    // Escape pipe in summary to avoid breaking the marker format
    let safe_summary = summary.replace('|', "/");
    Some(format!("{{{{TOOL:{}|{}}}}}", tool_name, safe_summary))
}

struct SessionQuickMetadata {
    custom_title: Option<String>,
    first_prompt: Option<String>,
    first_timestamp: Option<String>,
    last_timestamp: Option<String>,
    conversation_count: u64,
    total_tokens: u64,
}

fn extract_quick_metadata(jsonl_path: &PathBuf) -> SessionQuickMetadata {
    let mut custom_title: Option<String> = None;
    let mut first_prompt: Option<String> = None;
    let mut first_timestamp: Option<String> = None;
    let mut last_timestamp: Option<String> = None;
    let mut conversation_count: u64 = 0;
    let mut token_by_request: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();

    let file = match fs::File::open(jsonl_path) {
        Ok(file) => file,
        Err(_) => {
            return SessionQuickMetadata {
                custom_title,
                first_prompt,
                first_timestamp,
                last_timestamp,
                conversation_count,
                total_tokens: 0,
            };
        }
    };
    let reader = BufReader::new(file);

    // Single pass: extract head fields on first sighting, update tail fields
    // (custom_title, last_timestamp, tokens) continuously, and count user messages
    // using fast substring checks to skip irrelevant lines.
    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }

        // Lightweight timestamp scan — every line has one; avoid JSON parse cost
        update_timestamps_from_line(&line, &mut first_timestamp, &mut last_timestamp);

        // Custom title (can appear anywhere after /rename)
        if line.contains("\"type\":\"custom-title\"") {
            if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
                if let Some(title) = entry.custom_title {
                    custom_title = Some(title);
                }
            }
            continue;
        }

        // Token accounting on assistant usage entries
        if line.contains("\"type\":\"assistant\"") && line.contains("\"usage\"") {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if let Some(usage) = value.get("message").and_then(|msg| msg.get("usage")) {
                    let input = usage.get("input_tokens").and_then(|val| val.as_u64()).unwrap_or(0);
                    let output = usage.get("output_tokens").and_then(|val| val.as_u64()).unwrap_or(0);
                    if let Some(request_id) = value.get("requestId").and_then(|val| val.as_str()) {
                        token_by_request.insert(request_id.to_string(), input + output);
                    }
                }
            }
            continue;
        }

        // User message counting + first_prompt extraction
        if !line.contains("\"type\":\"user\"") {
            continue;
        }
        if line.contains("\"toolUseResult\"")
            || line.contains("\"isSidechain\":true")
            || line.contains("\"isCompactSummary\":true")
        {
            continue;
        }

        conversation_count += 1;

        if first_prompt.is_none() {
            if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
                first_prompt = Some(extract_user_text(&entry.message).chars().take(200).collect());
            }
        }
    }

    SessionQuickMetadata {
        custom_title,
        first_prompt,
        first_timestamp,
        last_timestamp,
        conversation_count,
        total_tokens: token_by_request.values().sum(),
    }
}

fn update_timestamps_from_line(
    line: &str,
    first_timestamp: &mut Option<String>,
    last_timestamp: &mut Option<String>,
) {
    if let Some(timestamp_start) = line.find("\"timestamp\":\"") {
        let value_start = timestamp_start + "\"timestamp\":\"".len();
        if let Some(value_end) = line[value_start..].find('"') {
            let timestamp = line[value_start..value_start + value_end].to_string();
            if first_timestamp.is_none() {
                *first_timestamp = Some(timestamp.clone());
            }
            *last_timestamp = Some(timestamp);
        }
    }
}

