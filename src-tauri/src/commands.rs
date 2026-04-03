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
    #[serde(rename = "toolUseResult")]
    tool_use_result: Option<Value>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "isCompactSummary")]
    is_compact_summary: Option<bool>,
    message: Option<JsonlMessage>,
    timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonlMessage {
    content: Option<Value>,
    stop_reason: Option<Value>,
}

// ── Commands ────────────────────────────────────────────────────────────────

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
                        let jsonl_path = project_dir
                            .join(format!("{}.jsonl", entry.session_id))
                            .to_string_lossy()
                            .to_string();

                        // Skip sessions whose JSONL files no longer exist
                        if !PathBuf::from(&jsonl_path).exists() {
                            continue;
                        }

                        sessions.push(SessionInfo {
                            session_id: entry.session_id,
                            summary: entry.summary,
                            first_prompt: entry.first_prompt,
                            project_path: original_path.clone(),
                            project_name: project_name.clone(),
                            created: entry.created,
                            modified: entry.modified,
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

                            sessions.push(SessionInfo {
                                session_id,
                                summary: metadata.custom_title,
                                first_prompt: metadata.first_prompt,
                                project_path: original_path.clone(),
                                project_name: project_name.clone(),
                                created: metadata.first_timestamp,
                                modified: metadata.last_timestamp,
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

                    sessions.push(SessionInfo {
                        session_id,
                        summary: metadata.custom_title,
                        first_prompt: metadata.first_prompt,
                        project_path: decoded_path.clone(),
                        project_name: project_name.clone(),
                        created: metadata.first_timestamp,
                        modified: metadata.last_timestamp,
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

    for line in reader.lines() {
        let line = match line {
            Ok(content) => content,
            Err(_) => continue,
        };
        if line.is_empty() {
            continue;
        }

        let entry: JsonlEntry = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        let entry_type = match &entry.entry_type {
            Some(entry_type) => entry_type.as_str(),
            None => continue,
        };

        // Skip sidechains (subagent conversations)
        if entry.is_sidechain.unwrap_or(false) {
            continue;
        }

        match entry_type {
            "user" => {
                // Flush any pending assistant text
                if in_assistant_turn && !current_assistant_text.is_empty() {
                    messages.push(ConversationMessage {
                        role: "assistant".to_string(),
                        text: current_assistant_text.clone(),
                        timestamp: current_assistant_timestamp.clone(),
                    });
                    current_assistant_text.clear();
                    in_assistant_turn = false;
                }

                // Skip tool results — only show actual human messages
                if entry.tool_use_result.is_some() {
                    continue;
                }

                let timestamp = entry.timestamp.unwrap_or_default();
                let text = extract_user_text(&entry.message);

                if !text.is_empty() {
                    let role = if entry.is_compact_summary.unwrap_or(false) {
                        "compaction"
                    } else {
                        "user"
                    };
                    messages.push(ConversationMessage {
                        role: role.to_string(),
                        text,
                        timestamp,
                    });
                }
            }
            "assistant" => {
                let timestamp = entry.timestamp.unwrap_or_default();

                if !in_assistant_turn {
                    in_assistant_turn = true;
                    current_assistant_timestamp = timestamp;
                }

                // Extract text blocks from assistant content
                if let Some(message) = &entry.message {
                    let text_parts = extract_assistant_text(&message.content);
                    if !text_parts.is_empty() {
                        if !current_assistant_text.is_empty() {
                            current_assistant_text.push_str("\n\n");
                        }
                        current_assistant_text.push_str(&text_parts);
                    }

                    // Check if this is the final chunk of the assistant turn
                    let is_end = match &message.stop_reason {
                        Some(Value::String(reason)) => {
                            reason == "end_turn" || reason == "tool_use"
                        }
                        _ => false,
                    };

                    if is_end && !current_assistant_text.is_empty() {
                        messages.push(ConversationMessage {
                            role: "assistant".to_string(),
                            text: current_assistant_text.clone(),
                            timestamp: current_assistant_timestamp.clone(),
                        });
                        current_assistant_text.clear();
                        in_assistant_turn = false;
                    }
                }
            }
            _ => {
                // Skip permission-mode, file-history-snapshot, attachment, system, etc.
            }
        }
    }

    // Flush any remaining assistant text
    if in_assistant_turn && !current_assistant_text.is_empty() {
        messages.push(ConversationMessage {
            role: "assistant".to_string(),
            text: current_assistant_text,
            timestamp: current_assistant_timestamp,
        });
    }

    Ok(messages)
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

fn decode_project_path(encoded: &str) -> String {
    // "-Users-vishnu-Documents-archer" → "/Users/vishnu/Documents/archer"
    if encoded.starts_with('-') {
        encoded.replacen('-', "/", 1).replace('-', "/")
    } else {
        encoded.replace('-', "/")
    }
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
                    if block_type == "text" {
                        if let Some(text) = block.get("text").and_then(|text| text.as_str()) {
                            parts.push(text.to_string());
                        }
                    }
                }
            }
            parts.join("\n\n")
        }
        _ => String::new(),
    }
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
    let empty = SessionQuickMetadata {
        custom_title: None,
        first_prompt: None,
        first_timestamp: None,
        last_timestamp: None,
        conversation_count: 0,
        total_tokens: 0,
    };

    let file = match fs::File::open(jsonl_path) {
        Ok(file) => file,
        Err(_) => return empty,
    };
    let file_size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);

    // ── Pass 1: Read first 100 lines for head metadata ─────────────────
    let head_reader = BufReader::new(match fs::File::open(jsonl_path) {
        Ok(file) => file,
        Err(_) => return empty,
    });

    let mut custom_title = None;
    let mut first_prompt = None;
    let mut first_timestamp = None;

    for line in head_reader.lines().flatten().take(100) {
        if line.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
            if first_timestamp.is_none() {
                if let Some(timestamp) = &entry.timestamp {
                    first_timestamp = Some(timestamp.clone());
                }
            }
            if let Some(ref entry_type) = entry.entry_type {
                if entry_type == "custom-title" {
                    if let Some(title) = &entry.custom_title {
                        custom_title = Some(title.clone());
                    }
                }
            }
            if first_prompt.is_none() {
                if let Some(ref entry_type) = entry.entry_type {
                    if entry_type == "user" && entry.tool_use_result.is_none() {
                        first_prompt = Some(
                            extract_user_text(&entry.message)
                                .chars()
                                .take(200)
                                .collect(),
                        );
                    }
                }
            }
            // Stop early if we have everything from the head
            if first_timestamp.is_some() && first_prompt.is_some() {
                break;
            }
        }
    }

    // ── Pass 2: Read last 128KB for tail metadata ──────────────────────
    let tail_size: u64 = 128 * 1024;
    let mut last_timestamp = None;
    let mut token_by_request: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();

    let tail_file = match fs::File::open(jsonl_path) {
        Ok(file) => file,
        Err(_) => return empty,
    };

    if file_size > tail_size {
        use std::io::Seek;
        let mut seekable = tail_file;
        let _ = seekable.seek(std::io::SeekFrom::End(-(tail_size as i64)));
        let tail_reader = BufReader::new(seekable);
        let mut lines_iter = tail_reader.lines();
        // Skip first partial line after seek
        let _ = lines_iter.next();
        for line in lines_iter.flatten() {
            parse_tail_line(&line, &mut last_timestamp, &mut custom_title, &mut token_by_request);
        }
    } else {
        // Small file — just read all of it for tail data
        let tail_reader = BufReader::new(tail_file);
        for line in tail_reader.lines().flatten() {
            parse_tail_line(&line, &mut last_timestamp, &mut custom_title, &mut token_by_request);
        }
    }

    let total_tokens: u64 = token_by_request.values().sum();

    // ── Pass 3: Fast conversation count via string matching ────────────
    // Avoid full JSON parse — just count lines that look like user messages
    let count_file = match fs::File::open(jsonl_path) {
        Ok(file) => file,
        Err(_) => return empty,
    };
    let count_reader = BufReader::new(count_file);
    let mut conversation_count: u64 = 0;

    for line in count_reader.lines().flatten() {
        // Fast string checks before any JSON parsing
        if !line.contains("\"type\":\"user\"") {
            continue;
        }
        if line.contains("\"toolUseResult\"") {
            continue;
        }
        if line.contains("\"isSidechain\":true") {
            continue;
        }
        if line.contains("\"isCompactSummary\":true") {
            continue;
        }
        conversation_count += 1;
    }

    SessionQuickMetadata {
        custom_title,
        first_prompt,
        first_timestamp,
        last_timestamp,
        conversation_count,
        total_tokens,
    }
}

fn parse_tail_line(
    line: &str,
    last_timestamp: &mut Option<String>,
    custom_title: &mut Option<String>,
    token_by_request: &mut std::collections::HashMap<String, u64>,
) {
    if line.is_empty() {
        return;
    }

    // Extract timestamp with lightweight string search
    if let Some(timestamp_start) = line.find("\"timestamp\":\"") {
        let value_start = timestamp_start + 13;
        if let Some(value_end) = line[value_start..].find('"') {
            *last_timestamp = Some(line[value_start..value_start + value_end].to_string());
        }
    }

    // Pick up custom title (may appear late if user renamed)
    if line.contains("\"type\":\"custom-title\"") {
        if let Ok(entry) = serde_json::from_str::<JsonlEntry>(line) {
            if let Some(title) = entry.custom_title {
                *custom_title = Some(title);
            }
        }
    }

    // Extract token usage from assistant messages
    if line.contains("\"type\":\"assistant\"") && line.contains("\"usage\"") {
        if let Ok(raw) = serde_json::from_str::<Value>(line) {
            if let Some(usage) = raw.get("message").and_then(|msg| msg.get("usage")) {
                let input = usage.get("input_tokens").and_then(|val| val.as_u64()).unwrap_or(0);
                let output = usage.get("output_tokens").and_then(|val| val.as_u64()).unwrap_or(0);
                if let Some(request_id) = raw.get("requestId").and_then(|val| val.as_str()) {
                    token_by_request.insert(request_id.to_string(), input + output);
                }
            }
        }
    }
}
