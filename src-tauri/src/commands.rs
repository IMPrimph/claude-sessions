use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use tauri::Manager;

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
    pub custom_title: Option<String>,
    pub ai_title: Option<String>,
    pub first_prompt: Option<String>,
    pub project_path: String,
    pub project_name: String,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub message_count: Option<u64>,
    pub conversation_count: u64,
    pub total_tokens: u64,
    pub git_branch: Option<String>,
    /// Set when this session was created by forking/branching another session
    /// (`/branch` in Claude Code). Holds the parent session's id; the frontend
    /// resolves it to the parent's title and shows a "fork of …" badge.
    pub forked_from_session_id: Option<String>,
    pub jsonl_path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ConversationMessage {
    pub role: String, // "user" or "assistant"
    pub text: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<MessageImage>,
    /// True on an assistant reply that was cut off mid-response by the user
    /// pressing Esc (the transcript records a synthetic
    /// `[Request interrupted by user]` marker right after it).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub interrupted: bool,
    /// True on a user message that was sent *during* an interruption — i.e. it
    /// came after an interrupt marker and before Claude's reply resumed. These
    /// are the "mid-turn" steering messages fired while Claude was still working.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mid_turn: bool,
    /// Present only when `role == "agent-notification"`: the parsed contents of a
    /// `<task-notification>` entry (a background subagent / workflow agent
    /// finishing). Claude Code injects these with `type:"user"`, but they are not
    /// something the user typed — they are surfaced as a distinct card, not a bubble.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification: Option<AgentNotification>,
}

/// A background agent / workflow completion event, parsed from a
/// `<task-notification>` transcript entry. Covers single subagents, dynamic
/// workflows, and multi-agent fan-outs (they share the same wrapper).
#[derive(Debug, Serialize, Clone, Default)]
pub struct AgentNotification {
    /// Human summary, e.g. `Agent "Map Archer database domain model" finished`.
    pub summary: String,
    /// `completed`, `error`, etc.
    pub status: String,
    /// The agent's full returned output. May be large; the UI keeps it collapsed.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_uses: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    // Multi-agent fan-out summaries carry these instead of per-agent usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents_done: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents_error: Option<u64>,
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
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
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
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    #[serde(rename = "isCompactSummary")]
    is_compact_summary: Option<bool>,
    message: Option<JsonlMessage>,
    timestamp: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    /// Present on `type:"attachment"` entries. A `queued_command` attachment is a
    /// message the user queued while Claude was still working — Claude Code stores
    /// it here rather than as a normal user turn, so we surface it explicitly.
    attachment: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct JsonlMessage {
    content: Option<Value>,
    model: Option<String>,
}

// ── Commands ────────────────────────────────────────────────────────────────

/// Returns the absolute path to a session's pasted image, if it exists.
/// Claude Code caches pasted images at ~/.claude/image-cache/<session_id>/<N>.<ext>.
/// Falls back to the local archive so images survive Claude Code's 30-day cleanup.
#[tauri::command]
pub fn get_image_path(app: tauri::AppHandle, session_id: String, image_number: u32) -> Option<String> {
    let image_in = |base: PathBuf| -> Option<String> {
        for extension in ["png", "jpg", "jpeg", "gif", "webp"] {
            let path = base.join(format!("{}.{}", image_number, extension));
            if path.exists() {
                return Some(path.to_string_lossy().to_string());
            }
        }
        None
    };

    // Live cache first.
    if let Some(home) = dirs::home_dir() {
        if let Some(found) = image_in(home.join(".claude").join("image-cache").join(&session_id)) {
            return Some(found);
        }
    }
    // Archive fallback (the live cache may have expired).
    if let Ok(root) = archive_root(&app) {
        if let Some(found) = image_in(root.join("image-cache").join(&session_id)) {
            return Some(found);
        }
    }
    None
}

/// Path of the small config file that can hold a user-chosen archive location.
fn config_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|resolve_error| format!("Cannot resolve app data dir: {}", resolve_error))?;
    Ok(data_dir.join("config.json"))
}

/// Root of the local session archive. Defaults to <app_data_dir>/archive, but
/// honors a user-chosen location stored in config.json (Settings → Storage).
fn archive_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|resolve_error| format!("Cannot resolve app data dir: {}", resolve_error))?;

    if let Ok(text) = fs::read_to_string(data_dir.join("config.json")) {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            if let Some(custom) = value.get("archivePath").and_then(|path| path.as_str()) {
                let trimmed = custom.trim();
                if !trimmed.is_empty() {
                    return Ok(PathBuf::from(trimmed));
                }
            }
        }
    }
    Ok(data_dir.join("archive"))
}

/// Recursive total size of a directory in bytes (0 if it doesn't exist).
fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                total += dir_size(&entry_path);
            } else if let Ok(metadata) = entry.metadata() {
                total += metadata.len();
            }
        }
    }
    total
}

#[derive(Debug, Serialize, Clone)]
pub struct ArchiveInfo {
    pub path: String,
    pub session_count: u64,
    pub total_bytes: u64,
    /// True when using a user-chosen location rather than the app default.
    pub is_custom: bool,
}

fn archive_info(app: &tauri::AppHandle) -> Result<ArchiveInfo, String> {
    let root = archive_root(app)?;
    let default_root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("{}", error))?
        .join("archive");

    let mut session_count = 0;
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            // Each archived session is its own dir; the shared image-cache dir isn't one.
            if entry.path().is_dir() && entry.file_name() != "image-cache" {
                session_count += 1;
            }
        }
    }

    Ok(ArchiveInfo {
        path: root.to_string_lossy().to_string(),
        session_count,
        total_bytes: dir_size(&root),
        is_custom: root != default_root,
    })
}

/// Transparency for Settings → Storage: where archives live, how many, total size.
#[tauri::command]
pub fn get_archive_info(app: tauri::AppHandle) -> Result<ArchiveInfo, String> {
    archive_info(&app)
}

/// Move the archive to a user-chosen folder and remember it. Existing archived
/// sessions are moved along, so nothing is orphaned. Returns updated info.
#[tauri::command]
pub fn set_archive_location(
    app: tauri::AppHandle,
    new_parent_dir: String,
) -> Result<ArchiveInfo, String> {
    let old_root = archive_root(&app)?;
    // Nest under a named folder so we never dump files into e.g. Documents root.
    let target = PathBuf::from(&new_parent_dir).join("ClaudeSessionsArchive");

    if target == old_root {
        return archive_info(&app);
    }

    fs::create_dir_all(&target)
        .map_err(|error| format!("Cannot create archive folder: {}", error))?;

    // Move existing archives across (copy then remove the old root).
    if old_root.exists() {
        copy_dir_recursive(&old_root, &target)
            .map_err(|error| format!("Failed to move existing archives: {}", error))?;
        let _ = fs::remove_dir_all(&old_root);
    }

    let data_dir = config_file(&app)?
        .parent()
        .map(|dir| dir.to_path_buf())
        .ok_or_else(|| "Cannot resolve config dir".to_string())?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("{}", error))?;
    let config = serde_json::json!({ "archivePath": target.to_string_lossy() });
    fs::write(
        config_file(&app)?,
        serde_json::to_string_pretty(&config).unwrap_or_default(),
    )
    .map_err(|error| format!("Cannot save config: {}", error))?;

    archive_info(&app)
}

/// Reset the archive back to the app-default location (moves files back too).
#[tauri::command]
pub fn reset_archive_location(app: tauri::AppHandle) -> Result<ArchiveInfo, String> {
    let old_root = archive_root(&app)?;
    let default_root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("{}", error))?
        .join("archive");

    if old_root != default_root && old_root.exists() {
        fs::create_dir_all(&default_root).map_err(|error| format!("{}", error))?;
        copy_dir_recursive(&old_root, &default_root)
            .map_err(|error| format!("Failed to move archives back: {}", error))?;
        let _ = fs::remove_dir_all(&old_root);
    }
    let _ = fs::remove_file(config_file(&app)?);
    archive_info(&app)
}

/// Reveal the archive folder in Finder.
#[tauri::command]
pub fn open_archive_location(app: tauri::AppHandle) -> Result<(), String> {
    let root = archive_root(&app)?;
    fs::create_dir_all(&root).ok();
    std::process::Command::new("open")
        .arg(&root)
        .spawn()
        .map_err(|error| format!("Cannot open folder: {}", error))?;
    Ok(())
}

/// Recursively copy a directory (used for subagent transcripts + image caches).
fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Core archive logic against an explicit root (so it's unit-testable without a
/// running Tauri app). Mirrors the parent-relative layout — <root>/<id>/<id>.jsonl
/// and <root>/<id>/<id>/subagents — so the existing path derivations keep working
/// when reading from the archive. Idempotent: the transcript is re-copied only when
/// the source is newer than the archived copy.
fn archive_session_to(
    root: &Path,
    source_jsonl: &Path,
    session_id: &str,
    live_images: Option<&Path>,
    meta: &Value,
) -> Result<(), String> {
    if !source_jsonl.exists() {
        return Err(format!("Session file not found: {}", source_jsonl.display()));
    }
    let session_dir = root.join(session_id);
    fs::create_dir_all(&session_dir)
        .map_err(|create_error| format!("Cannot create archive dir: {}", create_error))?;

    // Re-archive only when the live transcript is newer than the archived copy —
    // so opening an unchanged saved session is a no-op (no wasteful re-copy of the
    // transcript, subagents, or images), while a session you kept working in gets
    // fully refreshed.
    let dest_jsonl = session_dir.join(format!("{}.jsonl", session_id));
    let source_newer = match (fs::metadata(&dest_jsonl), fs::metadata(source_jsonl)) {
        (Ok(dest_meta), Ok(source_meta)) => match (dest_meta.modified(), source_meta.modified()) {
            (Ok(dest_time), Ok(source_time)) => source_time > dest_time,
            _ => true,
        },
        _ => true,
    };
    if !source_newer {
        return Ok(());
    }

    // 1) Transcript.
    fs::copy(source_jsonl, &dest_jsonl)
        .map_err(|copy_error| format!("Copy transcript failed: {}", copy_error))?;

    // 2) Subagent logs: <project_dir>/<id>/subagents → <root>/<id>/<id>/subagents
    if let Some(parent_dir) = source_jsonl.parent() {
        let source_subagents = parent_dir.join(session_id).join("subagents");
        if source_subagents.is_dir() {
            let dest_subagents = session_dir.join(session_id).join("subagents");
            let _ = copy_dir_recursive(&source_subagents, &dest_subagents);
        }
    }

    // 3) Pasted images: <images>/… → <root>/image-cache/<id>
    if let Some(images) = live_images {
        if images.is_dir() {
            let _ = copy_dir_recursive(images, &root.join("image-cache").join(session_id));
        }
    }

    // 4) Self-describing metadata so the archive stands on its own.
    let _ = fs::write(
        session_dir.join("meta.json"),
        serde_json::to_string_pretty(meta).unwrap_or_default(),
    );

    Ok(())
}

/// Copy a session's transcript + subagent logs + pasted images into the local
/// archive so it survives Claude Code's 30-day cleanup, keeping bookmarks openable.
#[tauri::command]
pub fn archive_session(
    app: tauri::AppHandle,
    jsonl_path: String,
    session_id: String,
    project_path: String,
    project_name: String,
    title: Option<String>,
) -> Result<(), String> {
    let root = archive_root(&app)?;
    let source = PathBuf::from(&jsonl_path);
    let live_images = dirs::home_dir()
        .map(|home| home.join(".claude").join("image-cache").join(&session_id));
    let meta = serde_json::json!({
        "session_id": session_id,
        "project_path": project_path,
        "project_name": project_name,
        "title": title,
        "source_path": jsonl_path,
        "archived_at": chrono::Utc::now().to_rfc3339(),
    });
    archive_session_to(&root, &source, &session_id, live_images.as_deref(), &meta)
}

/// If a session has been archived, returns its archived transcript path — used as a
/// fallback when the live file has expired. None if the session isn't archived.
#[tauri::command]
pub fn get_archived_session_path(app: tauri::AppHandle, session_id: String) -> Option<String> {
    let root = archive_root(&app).ok()?;
    let dest = root.join(&session_id).join(format!("{}.jsonl", session_id));
    if dest.exists() {
        Some(dest.to_string_lossy().to_string())
    } else {
        None
    }
}

/// The session ids that currently have an archived copy — so the UI can mark saved
/// sessions in the list and header with one call instead of one probe per session.
#[tauri::command]
pub fn get_archived_session_ids(app: tauri::AppHandle) -> Vec<String> {
    let root = match archive_root(&app) {
        Ok(root) => root,
        Err(_) => return Vec::new(),
    };
    let mut ids = Vec::new();
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "image-cache" || !entry.path().is_dir() {
                continue;
            }
            // Only count it as saved if the transcript is actually present.
            if entry.path().join(format!("{}.jsonl", name)).exists() {
                ids.push(name);
            }
        }
    }
    ids
}

/// Remove a session's archived copy (transcript + subagents + images + meta). Deletes
/// only our archive — the live session, if any, is untouched.
#[tauri::command]
pub fn unarchive_session(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    let root = archive_root(&app)?;
    let session_dir = root.join(&session_id);
    if session_dir.exists() {
        fs::remove_dir_all(&session_dir)
            .map_err(|error| format!("Failed to remove archived session: {}", error))?;
    }
    let images = root.join("image-cache").join(&session_id);
    if images.exists() {
        let _ = fs::remove_dir_all(&images);
    }
    Ok(())
}

#[derive(Debug, Serialize, Clone)]
pub struct FileEditEntry {
    pub timestamp: Option<String>,
    /// One of "edit", "write", "multiedit", "notebookedit".
    pub action: String,
    /// Previous content (None for Write — there's nothing before).
    pub old_string: Option<String>,
    /// New content. None for unusual cases (shouldn't happen in practice).
    pub new_string: Option<String>,
    pub tool_use_id: Option<String>,
    /// True for `replace_all` Edit calls — informational tag in the UI.
    pub replace_all: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct FileChange {
    pub path: String,
    pub display_path: String,
    pub edits: Vec<FileEditEntry>,
    pub edit_count: u32,
    pub read_count: u32,
}

/// Per-session breakdown of file changes — captures the actual old/new content
/// from each Edit/Write/MultiEdit/NotebookEdit call so the UI can render a diff.
/// Bash file ops are skipped (too heuristic). Subagent ops aren't included in v1.
#[tauri::command]
pub fn get_session_file_changes(jsonl_path: String) -> Result<Vec<FileChange>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }

    // Resolve project path so we can build project-relative display paths
    let project_path = path
        .parent()
        .and_then(|parent| {
            parent
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| (parent.to_path_buf(), name.to_string()))
        })
        .map(|(parent, name)| resolve_project_path(&parent, &name))
        .unwrap_or_default();
    let project_name = project_path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string();

    struct Bucket {
        edits: Vec<FileEditEntry>,
        read_count: u32,
    }
    let mut buckets: std::collections::HashMap<String, Bucket> =
        std::collections::HashMap::new();

    let file = fs::File::open(&path)
        .map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    for line in reader.lines().map_while(Result::ok) {
        if !line.contains("\"tool_use\"") {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let timestamp = value
            .get("timestamp")
            .and_then(|val| val.as_str())
            .map(String::from);

        let blocks = match value
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_array())
        {
            Some(blocks) => blocks,
            None => continue,
        };

        for block in blocks {
            if block.get("type").and_then(|val| val.as_str()) != Some("tool_use") {
                continue;
            }
            let tool_name = match block.get("name").and_then(|val| val.as_str()) {
                Some(name) => name,
                None => continue,
            };
            let input = match block.get("input") {
                Some(input) => input,
                None => continue,
            };
            let tool_use_id = block.get("id").and_then(|val| val.as_str()).map(String::from);

            // Extract file path + entries (one tool call can produce multiple entries
            // for MultiEdit). Read calls just bump read_count.
            let extracted = extract_file_change_entries(tool_name, input, &timestamp, &tool_use_id);
            let (file_path, entries, is_read) = match extracted {
                Some(extracted) => extracted,
                None => continue,
            };

            let bucket = buckets.entry(file_path).or_insert_with(|| Bucket {
                edits: Vec::new(),
                read_count: 0,
            });
            if is_read {
                bucket.read_count += 1;
            } else {
                for entry in entries {
                    bucket.edits.push(entry);
                }
            }
        }
    }

    let mut changes: Vec<FileChange> = buckets
        .into_iter()
        .map(|(file_path, bucket)| FileChange {
            display_path: build_display_path(&project_path, &project_name, &file_path),
            edit_count: bucket.edits.len() as u32,
            read_count: bucket.read_count,
            edits: bucket.edits,
            path: file_path,
        })
        .collect();

    // Sort: most edits first, then by reads, then alphabetically
    changes.sort_by(|first, second| {
        second
            .edit_count
            .cmp(&first.edit_count)
            .then_with(|| second.read_count.cmp(&first.read_count))
            .then_with(|| first.display_path.cmp(&second.display_path))
    });

    Ok(changes)
}

/// Returns (file_path, entries, is_read). `entries` is empty for Read calls.
fn extract_file_change_entries(
    tool_name: &str,
    input: &Value,
    timestamp: &Option<String>,
    tool_use_id: &Option<String>,
) -> Option<(String, Vec<FileEditEntry>, bool)> {
    let pick = |key: &str| -> Option<String> {
        input.get(key).and_then(|val| val.as_str()).map(String::from)
    };

    match tool_name {
        "Read" | "read" => {
            let path = pick("file_path")?;
            Some((path, Vec::new(), true))
        }
        "Write" | "write" => {
            let path = pick("file_path")?;
            Some((
                path,
                vec![FileEditEntry {
                    timestamp: timestamp.clone(),
                    action: "write".to_string(),
                    old_string: None,
                    new_string: pick("content"),
                    tool_use_id: tool_use_id.clone(),
                    replace_all: false,
                }],
                false,
            ))
        }
        "Edit" | "edit" => {
            let path = pick("file_path")?;
            let replace_all = input
                .get("replace_all")
                .and_then(|val| val.as_bool())
                .unwrap_or(false);
            Some((
                path,
                vec![FileEditEntry {
                    timestamp: timestamp.clone(),
                    action: "edit".to_string(),
                    old_string: pick("old_string"),
                    new_string: pick("new_string"),
                    tool_use_id: tool_use_id.clone(),
                    replace_all,
                }],
                false,
            ))
        }
        "MultiEdit" | "multiedit" => {
            let path = pick("file_path")?;
            let edits_array = input.get("edits").and_then(|val| val.as_array())?;
            let mut entries: Vec<FileEditEntry> = Vec::with_capacity(edits_array.len());
            for edit in edits_array {
                entries.push(FileEditEntry {
                    timestamp: timestamp.clone(),
                    action: "multiedit".to_string(),
                    old_string: edit.get("old_string").and_then(|val| val.as_str()).map(String::from),
                    new_string: edit.get("new_string").and_then(|val| val.as_str()).map(String::from),
                    tool_use_id: tool_use_id.clone(),
                    replace_all: edit
                        .get("replace_all")
                        .and_then(|val| val.as_bool())
                        .unwrap_or(false),
                });
            }
            Some((path, entries, false))
        }
        "NotebookEdit" | "notebookedit" => {
            let path = pick("notebook_path").or_else(|| pick("file_path"))?;
            Some((
                path,
                vec![FileEditEntry {
                    timestamp: timestamp.clone(),
                    action: "notebookedit".to_string(),
                    old_string: pick("old_source"),
                    new_string: pick("new_source").or_else(|| pick("new_string")),
                    tool_use_id: tool_use_id.clone(),
                    replace_all: false,
                }],
                false,
            ))
        }
        _ => None,
    }
}

fn build_display_path(project_path: &str, project_name: &str, file_path: &str) -> String {
    if project_path.is_empty() || project_name.is_empty() {
        return file_path.to_string();
    }
    let prefix = format!("{}/", project_path);
    if let Some(relative) = file_path.strip_prefix(&prefix) {
        return format!("{} / {}", project_name, relative);
    }
    if file_path == project_path {
        return project_name.to_string();
    }
    file_path.to_string()
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
            .next_back()
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

fn resolve_project_path(project_dir: &Path, dir_name: &str) -> String {
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

/// Peek a session's first lines for a `forkedFrom.sessionId`. Claude Code stamps
/// this on every entry inherited from the parent when a session is forked
/// (`/branch`), so it's present from line 1 — reading a few lines is enough and
/// stays cheap even for multi-MB sessions.
fn detect_fork_parent(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines().map_while(Result::ok).take(5) {
        if !line.contains("\"forkedFrom\"") {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            if let Some(parent) = value
                .get("forkedFrom")
                .and_then(|fork| fork.get("sessionId"))
                .and_then(|id| id.as_str())
            {
                return Some(parent.to_string());
            }
        }
    }
    None
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
                        .next_back()
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
                            custom_title: entry.custom_title,
                            ai_title: entry.ai_title,
                            first_prompt: entry.first_prompt,
                            project_path: original_path.clone(),
                            project_name: project_name.clone(),
                            created: entry.created,
                            modified: file_modified.or(entry.modified),
                            message_count: entry.message_count,
                            conversation_count: 0,
                            total_tokens: 0,
                            git_branch: entry.git_branch,
                            forked_from_session_id: detect_fork_parent(&jsonl_pathbuf),
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
                                summary: None,
                                custom_title: metadata.custom_title,
                                ai_title: metadata.ai_title,
                                first_prompt: metadata.first_prompt,
                                project_path: original_path.clone(),
                                project_name: project_name.clone(),
                                created: metadata.first_timestamp,
                                modified: file_modified.or(metadata.last_timestamp),
                                message_count: None,
                                conversation_count: metadata.conversation_count,
                                total_tokens: metadata.total_tokens,
                                git_branch: None,
                                forked_from_session_id: detect_fork_parent(&file_path),
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
            .next_back()
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
                        summary: None,
                        custom_title: metadata.custom_title,
                        ai_title: metadata.ai_title,
                        first_prompt: metadata.first_prompt,
                        project_path: decoded_path.clone(),
                        project_name: project_name.clone(),
                        created: metadata.first_timestamp,
                        modified: file_modified.or(metadata.last_timestamp),
                        message_count: None,
                        conversation_count: metadata.conversation_count,
                        total_tokens: metadata.total_tokens,
                        git_branch: None,
                        forked_from_session_id: detect_fork_parent(&file_path),
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
            .next_back()
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
                .or(metadata.ai_title.clone())
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

            for line in reader.lines().map_while(Result::ok) {
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

#[derive(Debug, Serialize, Clone, Default)]
pub struct SessionStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub assistant_count: u64,
    pub user_prompt_count: u64,
    pub thinking_block_count: u64,
    pub models: Vec<String>,
    pub tool_counts: Vec<ToolCount>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ToolCount {
    pub name: String,
    pub count: u64,
}

#[tauri::command]
pub fn get_session_stats(jsonl_path: String) -> Result<SessionStats, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }

    let file = fs::File::open(&path)
        .map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut stats = SessionStats::default();
    // Dedupe assistant token usage by requestId (streaming responses emit multiple lines)
    let mut token_by_request: std::collections::HashMap<String, (u64, u64, u64, u64)> =
        std::collections::HashMap::new();
    let mut models_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut tool_counts: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    // Also dedupe assistant turn count by requestId — one logical turn per request
    let mut assistant_requests_seen: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }

        // Fast pre-checks before JSON parse
        if line.contains("\"type\":\"assistant\"") {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                let request_id = value
                    .get("requestId")
                    .and_then(|val| val.as_str())
                    .map(|s| s.to_string());

                if let Some(message) = value.get("message") {
                    if let Some(model) = message.get("model").and_then(|val| val.as_str()) {
                        if model != "<synthetic>" {
                            models_seen.insert(model.to_string());
                        }
                    }
                    if let Some(usage) = message.get("usage") {
                        let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let cache_creation = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        if let Some(ref rid) = request_id {
                            // Replace any prior partial entry — last write per rid wins
                            token_by_request.insert(
                                rid.clone(),
                                (input_tokens, output_tokens, cache_read, cache_creation),
                            );
                        }
                    }

                    // Tool use + thinking block counting
                    if let Some(content) = message.get("content").and_then(|val| val.as_array()) {
                        for block in content {
                            let block_type = block.get("type").and_then(|val| val.as_str()).unwrap_or("");
                            match block_type {
                                "tool_use" => {
                                    if let Some(tool_name) = block.get("name").and_then(|val| val.as_str()) {
                                        *tool_counts.entry(tool_name.to_string()).or_insert(0) += 1;
                                    }
                                }
                                "thinking" => {
                                    stats.thinking_block_count += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                if let Some(rid) = request_id {
                    if assistant_requests_seen.insert(rid) {
                        stats.assistant_count += 1;
                    }
                }
            }
        } else if line.contains("\"type\":\"user\"")
            && !line.contains("\"toolUseResult\"")
            && !line.contains("\"isSidechain\":true")
            && !line.contains("\"isMeta\":true")
            && !line.contains("\"isCompactSummary\":true")
        {
            // Real user prompt — parse to filter out tool_result-only content
            if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
                if !is_tool_result_content(&entry.message) {
                    stats.user_prompt_count += 1;
                }
            }
        }
    }

    for (input_tokens, output_tokens, cache_read, cache_creation) in token_by_request.values() {
        stats.input_tokens += input_tokens;
        stats.output_tokens += output_tokens;
        stats.cache_read_tokens += cache_read;
        stats.cache_creation_tokens += cache_creation;
    }

    stats.models = models_seen.into_iter().collect();
    stats.models.sort();

    let mut tool_pairs: Vec<ToolCount> = tool_counts
        .into_iter()
        .map(|(name, count)| ToolCount { name, count })
        .collect();
    tool_pairs.sort_by(|a, b| b.count.cmp(&a.count));
    stats.tool_counts = tool_pairs;

    Ok(stats)
}

/// Sum input+output tokens for a single session, de-duped by requestId (streaming
/// emits multiple usage lines per request). Returns 0 on any read error.
fn count_session_tokens(path: &Path) -> u64 {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return 0,
    };
    let reader = BufReader::new(file);

    let mut token_by_request: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();

    for line in reader.lines().map_while(Result::ok) {
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

    token_by_request.values().sum()
}

/// Token totals for a whole project in one call (session_id → tokens), avoiding a
/// per-session IPC round-trip. Sessions with zero tokens or read errors are omitted.
#[tauri::command]
pub fn get_project_tokens(
    jsonl_paths: Vec<String>,
) -> Result<std::collections::HashMap<String, u64>, String> {
    let mut totals: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for path_string in jsonl_paths {
        let path = PathBuf::from(&path_string);
        let session_id = match path.file_stem().and_then(|stem| stem.to_str()) {
            Some(stem) => stem.to_string(),
            None => continue,
        };
        let tokens = count_session_tokens(&path);
        if tokens > 0 {
            totals.insert(session_id, tokens);
        }
    }
    Ok(totals)
}

#[derive(Debug, Serialize, Clone)]
pub struct ToolResultPayload {
    pub content: String,
    pub is_error: bool,
    pub persisted_path: Option<String>,
}

/// Extract every tool_result from a session's JSONL into a map keyed by tool_use_id.
/// The frontend stores this map and renders results when a tool pill is expanded.
#[tauri::command]
pub fn get_tool_results(
    jsonl_path: String,
) -> Result<std::collections::HashMap<String, ToolResultPayload>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }

    let file = fs::File::open(&path)
        .map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut results: std::collections::HashMap<String, ToolResultPayload> =
        std::collections::HashMap::new();

    for line in reader.lines().map_while(Result::ok) {
        // Fast pre-check — only user entries with tool_result blocks
        if !line.contains("\"tool_result\"") {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let blocks = match value
            .get("message")
            .and_then(|msg| msg.get("content"))
            .and_then(|val| val.as_array())
        {
            Some(blocks) => blocks,
            None => continue,
        };

        for block in blocks {
            if block.get("type").and_then(|val| val.as_str()) != Some("tool_result") {
                continue;
            }
            let tool_use_id = match block.get("tool_use_id").and_then(|val| val.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };
            let is_error = block
                .get("is_error")
                .and_then(|val| val.as_bool())
                .unwrap_or(false);

            // Content can be a string OR an array of {type: "text", text: ...} blocks.
            let content = match block.get("content") {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Array(content_blocks)) => {
                    let mut parts = Vec::new();
                    for content_block in content_blocks {
                        if let Some(text) = content_block.get("text").and_then(|val| val.as_str()) {
                            parts.push(text.to_string());
                        }
                    }
                    parts.join("\n")
                }
                _ => String::new(),
            };

            let persisted_path = extract_persisted_path(&content);

            results.insert(
                tool_use_id,
                ToolResultPayload {
                    content,
                    is_error,
                    persisted_path,
                },
            );
        }
    }

    Ok(results)
}

#[derive(Debug, Serialize, Clone)]
pub struct AnsweredOption {
    pub label: String,
    pub description: String,
    pub chosen: bool,
    /// True when this wasn't one of the offered choices — i.e. the user picked
    /// "Other" and typed a custom answer.
    pub custom: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnsweredQuestion {
    pub question: String,
    pub header: String,
    pub multi_select: bool,
    pub options: Vec<AnsweredOption>,
    /// Free-text note the user attached to their answer (from `annotations`).
    /// Present even when no option was chosen — sometimes the note IS the answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Extract every AskUserQuestion Q&A from a session, keyed by the asking tool's
/// tool_use_id. Each answer entry's `toolUseResult` is self-contained: it carries
/// the questions, the offered options, AND the user's chosen answer(s) in an
/// `answers` map, so a single pass over the tool_result entries is enough. The
/// frontend renders this inline under the AskUserQuestion pill, chosen option lit.
#[tauri::command]
pub fn get_session_questions(
    jsonl_path: String,
) -> Result<std::collections::HashMap<String, Vec<AnsweredQuestion>>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }

    let file = fs::File::open(&path)
        .map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut results: std::collections::HashMap<String, Vec<AnsweredQuestion>> =
        std::collections::HashMap::new();

    for line in reader.lines().map_while(Result::ok) {
        // Fast pre-check — the answer entry always carries a toolUseResult with questions.
        if !line.contains("\"toolUseResult\"") || !line.contains("\"questions\"") {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        let tool_use_result = match value.get("toolUseResult") {
            Some(result) => result,
            None => continue,
        };
        let questions = match tool_use_result.get("questions").and_then(|val| val.as_array()) {
            Some(questions) => questions,
            None => continue,
        };

        // The tool_use_id ties this answer back to the assistant's AskUserQuestion pill.
        let tool_use_id = match find_tool_result_id(&value) {
            Some(id) => id,
            None => continue,
        };

        let parsed = parse_answered_questions(
            questions,
            tool_use_result.get("answers"),
            tool_use_result.get("annotations"),
        );
        if !parsed.is_empty() {
            results.insert(tool_use_id, parsed);
        }
    }

    Ok(results)
}

/// The tool_use_id of the first tool_result block in a user entry's message content.
fn find_tool_result_id(entry: &Value) -> Option<String> {
    let blocks = entry
        .get("message")
        .and_then(|msg| msg.get("content"))
        .and_then(|val| val.as_array())?;
    for block in blocks {
        if block.get("type").and_then(|val| val.as_str()) == Some("tool_result") {
            if let Some(id) = block.get("tool_use_id").and_then(|val| val.as_str()) {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// Collect a question's chosen answer(s). Single-select stores a bare string;
/// multi-select stores the picked labels joined as "A, B" (NOT a JSON array), so
/// for multi-select we split on ", " and match each part back to an option. A
/// JSON array is also accepted in case the stored format ever changes. Keyed by
/// exact question text — the map is sparse (only answered questions appear), so a
/// positional fallback would mis-attribute one question's answer to another.
fn answer_values(answers: Option<&Value>, question: &str, multi_select: bool) -> Vec<String> {
    let map = match answers.and_then(|val| val.as_object()) {
        Some(map) => map,
        None => return Vec::new(),
    };
    match map.get(question) {
        Some(Value::String(text)) if multi_select => text
            .split(", ")
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect(),
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(|text| text.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// The free-text note the user attached to a question, from the `annotations`
/// map. Keyed by exact question text — the map is sparse (only annotated
/// questions appear), so positional matching would cross-attribute notes.
fn question_note(annotations: Option<&Value>, question: &str) -> Option<String> {
    let map = annotations?.as_object()?;
    let entry = map.get(question)?;
    entry
        .get("notes")
        .and_then(|note| note.as_str())
        .map(|note| note.trim().to_string())
        .filter(|note| !note.is_empty())
}

fn parse_answered_questions(
    questions: &[Value],
    answers: Option<&Value>,
    annotations: Option<&Value>,
) -> Vec<AnsweredQuestion> {
    let mut parsed = Vec::new();
    for question_value in questions.iter() {
        let question = question_value
            .get("question")
            .and_then(|val| val.as_str())
            .unwrap_or("")
            .to_string();
        let header = question_value
            .get("header")
            .and_then(|val| val.as_str())
            .unwrap_or("")
            .to_string();
        let multi_select = question_value
            .get("multiSelect")
            .and_then(|val| val.as_bool())
            .unwrap_or(false);

        let chosen = answer_values(answers, &question, multi_select);

        let mut options: Vec<AnsweredOption> = Vec::new();
        if let Some(option_values) = question_value.get("options").and_then(|val| val.as_array()) {
            for option_value in option_values {
                let label = option_value
                    .get("label")
                    .and_then(|val| val.as_str())
                    .unwrap_or("")
                    .to_string();
                let description = option_value
                    .get("description")
                    .and_then(|val| val.as_str())
                    .unwrap_or("")
                    .to_string();
                let is_chosen = chosen.iter().any(|answer| answer == &label);
                options.push(AnsweredOption {
                    label,
                    description,
                    chosen: is_chosen,
                    custom: false,
                });
            }
        }

        // A chosen answer matching no offered label = the user picked "Other" and
        // typed their own text. Surface it so the real answer isn't lost.
        for answer in &chosen {
            if !options.iter().any(|option| &option.label == answer) {
                options.push(AnsweredOption {
                    label: answer.clone(),
                    description: String::new(),
                    chosen: true,
                    custom: true,
                });
            }
        }

        let notes = question_note(annotations, &question);

        parsed.push(AnsweredQuestion {
            question,
            header,
            multi_select,
            options,
            notes,
        });
    }
    parsed
}

#[derive(Debug, Serialize, Clone)]
pub struct SessionArtifact {
    pub url: String,
    pub title: String,
    /// Local source file that was published (informational).
    pub path: String,
}

/// Extract every published Artifact from a session, keyed by the Artifact tool's
/// tool_use_id. The answer entry's `toolUseResult` carries {url, path, title}, so
/// a single pass over the tool_result entries is enough. The frontend renders a
/// card with the title and a copy-link button in place of the generic pill.
#[tauri::command]
pub fn get_session_artifacts(
    jsonl_path: String,
) -> Result<std::collections::HashMap<String, SessionArtifact>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }

    let file = fs::File::open(&path)
        .map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut results: std::collections::HashMap<String, SessionArtifact> =
        std::collections::HashMap::new();

    for line in reader.lines().map_while(Result::ok) {
        // Fast pre-check — the published URL always contains this path segment.
        if !line.contains("/code/artifact/") {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let tool_use_result = match value.get("toolUseResult") {
            Some(result) => result,
            None => continue,
        };
        let url = match tool_use_result.get("url").and_then(|value| value.as_str()) {
            Some(url) if url.contains("/code/artifact/") => url.to_string(),
            _ => continue,
        };
        let tool_use_id = match find_tool_result_id(&value) {
            Some(id) => id,
            None => continue,
        };
        let title = tool_use_result
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let artifact_path = tool_use_result
            .get("path")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();

        results.insert(tool_use_id, SessionArtifact { url, title, path: artifact_path });
    }

    Ok(results)
}

/// Detect the `Full output saved to: <path>` line inside a `<persisted-output>` block.
/// Returns the path so the frontend can load the full output on demand.
fn extract_persisted_path(content: &str) -> Option<String> {
    if !content.contains("<persisted-output>") {
        return None;
    }
    const NEEDLE: &str = "Full output saved to: ";
    let start = content.find(NEEDLE)? + NEEDLE.len();
    let rest = &content[start..];
    let end = rest.find('\n').unwrap_or(rest.len());
    let path = rest[..end].trim();
    if path.is_empty() { None } else { Some(path.to_string()) }
}

/// Read the full text of a persisted tool-result sidecar file. Capped at a generous
/// size to avoid yanking gigabytes into the renderer; the frontend warns when truncated.
#[tauri::command]
pub fn read_tool_output_file(path: String) -> Result<String, String> {
    let pathbuf = PathBuf::from(&path);
    if !pathbuf.exists() {
        return Err(format!("File not found: {}", path));
    }
    // Safety: only allow reading files inside ~/.claude/projects/.../tool-results/.
    // Refuse any path that doesn't have "tool-results" as a directory component.
    if !pathbuf
        .components()
        .any(|component| component.as_os_str() == "tool-results")
    {
        return Err("Path is not inside a tool-results directory".to_string());
    }

    const MAX_BYTES: u64 = 5 * 1024 * 1024; // 5 MB
    let metadata = pathbuf
        .metadata()
        .map_err(|metadata_error| format!("Cannot stat: {}", metadata_error))?;
    if metadata.len() > MAX_BYTES {
        // Read a 5 MB head and append a notice
        use std::io::Read;
        let mut file = fs::File::open(&pathbuf)
            .map_err(|open_error| format!("Cannot open: {}", open_error))?;
        let mut buffer = vec![0u8; MAX_BYTES as usize];
        file.read_exact(&mut buffer)
            .map_err(|read_error| format!("Cannot read: {}", read_error))?;
        let mut text = String::from_utf8_lossy(&buffer).to_string();
        text.push_str(&format!(
            "\n\n[…truncated; file is {} bytes total]",
            metadata.len()
        ));
        return Ok(text);
    }

    fs::read_to_string(&pathbuf)
        .map_err(|read_error| format!("Cannot read: {}", read_error))
}

#[tauri::command]
pub fn export_session_markdown(
    jsonl_path: String,
    save_path: String,
    title: Option<String>,
) -> Result<(), String> {
    let messages = get_session_messages(jsonl_path)?;

    let mut markdown = String::new();
    if let Some(title) = title.as_ref().filter(|t| !t.is_empty()) {
        markdown.push_str(&format!("# {}\n\n", title));
    }

    for message in &messages {
        match message.role.as_str() {
            "user" => {
                markdown.push_str(&format!("## You — {}\n\n", message.timestamp));
                markdown.push_str(&message.text);
                markdown.push_str("\n\n");
            }
            "assistant" => {
                markdown.push_str(&format!("## Claude — {}\n\n", message.timestamp));
                // Replace internal markers with readable Markdown equivalents
                let cleaned = render_assistant_for_export(&message.text);
                markdown.push_str(&cleaned);
                markdown.push_str("\n\n");
            }
            "compaction" => {
                markdown.push_str(&format!(
                    "<details>\n<summary>Context Compacted — {}</summary>\n\n",
                    message.timestamp
                ));
                markdown.push_str(&message.text);
                markdown.push_str("\n\n</details>\n\n");
            }
            _ => {}
        }
    }

    fs::write(&save_path, markdown)
        .map_err(|write_error| format!("Cannot write {}: {}", save_path, write_error))?;
    Ok(())
}

fn render_assistant_for_export(text: &str) -> String {
    // {{TOOL:name|summary}} → "**[name]** summary"; {{THINKING_*}} blocks → collapsed details.
    let mut result = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if let Some(rest) = text.get(cursor..) {
            if let Some(tool_start) = rest.find("{{TOOL:") {
                result.push_str(&rest[..tool_start]);
                let after_open = cursor + tool_start + "{{TOOL:".len();
                if let Some(close) = text.get(after_open..).and_then(|s| s.find("}}")) {
                    let inner = &text[after_open..after_open + close];
                    // Marker format: name|summary[|toolUseId[|agentId]] — only name + summary
                    // are meaningful for the exported document.
                    let mut parts = inner.splitn(3, '|');
                    let tool_name = parts.next().unwrap_or(inner);
                    let summary = parts.next().unwrap_or("");
                    if summary.is_empty() {
                        result.push_str(&format!("> **[{}]**\n\n", tool_name));
                    } else {
                        result.push_str(&format!("> **[{}]** `{}`\n\n", tool_name, summary));
                    }
                    cursor = after_open + close + "}}".len();
                    continue;
                }
            }
            if let Some(think_start) = rest.find("{{THINKING_START}}") {
                result.push_str(&rest[..think_start]);
                let after_open = cursor + think_start + "{{THINKING_START}}".len();
                if let Some(close) = text.get(after_open..).and_then(|s| s.find("{{THINKING_END}}")) {
                    let inner = text[after_open..after_open + close].trim();
                    result.push_str("<details>\n<summary>Thinking…</summary>\n\n");
                    result.push_str(inner);
                    result.push_str("\n\n</details>\n\n");
                    cursor = after_open + close + "{{THINKING_END}}".len();
                    continue;
                }
            }
            // No more markers — copy remainder
            result.push_str(rest);
            break;
        }
        break;
    }
    result
}

#[derive(Debug, Serialize, Clone)]
pub struct SubagentInfo {
    pub agent_id: String,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    pub jsonl_path: String,
    pub tool_use_id: Option<String>,
}

/// Builds a (tool_use_id → agent_id) map by scanning user/toolUseResult entries.
/// The parent JSONL has `toolUseResult.agentId` and `message.content[].tool_use_id`
/// in the same entry, so we can correlate Agent tool calls to subagent files.
fn build_tool_to_agent_map(jsonl_path: &PathBuf) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let file = match fs::File::open(jsonl_path) {
        Ok(file) => file,
        Err(_) => return map,
    };
    let reader = BufReader::new(file);
    for line in reader.lines().map_while(Result::ok) {
        // Fast pre-check before JSON parse — most lines aren't tool results
        if !line.contains("\"toolUseResult\"") || !line.contains("\"agentId\"") {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let agent_id = match value
            .get("toolUseResult")
            .and_then(|val| val.get("agentId"))
            .and_then(|val| val.as_str())
        {
            Some(id) => id.to_string(),
            None => continue,
        };
        // Find the tool_use_id inside message.content[] blocks
        if let Some(content) = value
            .get("message")
            .and_then(|msg| msg.get("content"))
            .and_then(|val| val.as_array())
        {
            for block in content {
                if let Some(tool_use_id) = block.get("tool_use_id").and_then(|val| val.as_str()) {
                    map.insert(tool_use_id.to_string(), agent_id.clone());
                    break;
                }
            }
        }
    }
    map
}

#[tauri::command]
pub fn list_subagents(jsonl_path: String) -> Result<Vec<SubagentInfo>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }

    // Subagent dir is `<parent_dir>/<session_id>/subagents/`
    let parent_dir = match path.parent() {
        Some(parent) => parent,
        None => return Ok(Vec::new()),
    };
    let session_id = match path.file_stem().and_then(|stem| stem.to_str()) {
        Some(stem) => stem.to_string(),
        None => return Ok(Vec::new()),
    };
    let subagents_dir = parent_dir.join(&session_id).join("subagents");
    if !subagents_dir.exists() {
        return Ok(Vec::new());
    }

    // Reverse the (tool_use_id → agent_id) map to (agent_id → tool_use_id) so we can
    // attach the originating tool_use_id to each subagent if present.
    let tool_to_agent = build_tool_to_agent_map(&path);
    let mut agent_to_tool: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for (tool_use_id, agent_id) in tool_to_agent.iter() {
        agent_to_tool.insert(agent_id.clone(), tool_use_id.clone());
    }

    let mut subagents: Vec<SubagentInfo> = Vec::new();
    let entries = match fs::read_dir(&subagents_dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(Vec::new()),
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
            continue;
        }
        let file_name = match entry_path.file_stem().and_then(|stem| stem.to_str()) {
            Some(stem) => stem.to_string(),
            None => continue,
        };
        let agent_id = file_name
            .strip_prefix("agent-")
            .unwrap_or(&file_name)
            .to_string();

        // Sidecar meta file has agentType + description
        let meta_path = entry_path.with_extension("meta.json");
        let (agent_type, description) = match fs::read_to_string(&meta_path) {
            Ok(meta_content) => match serde_json::from_str::<Value>(&meta_content) {
                Ok(meta_value) => (
                    meta_value
                        .get("agentType")
                        .and_then(|val| val.as_str())
                        .map(String::from),
                    meta_value
                        .get("description")
                        .and_then(|val| val.as_str())
                        .map(String::from),
                ),
                Err(_) => (None, None),
            },
            Err(_) => (None, None),
        };

        subagents.push(SubagentInfo {
            tool_use_id: agent_to_tool.get(&agent_id).cloned(),
            agent_id,
            agent_type,
            description,
            jsonl_path: entry_path.to_string_lossy().to_string(),
        });
    }

    // Sort by mtime so the order vaguely matches conversation order
    subagents.sort_by(|first, second| {
        let first_meta = std::fs::metadata(&first.jsonl_path).ok();
        let second_meta = std::fs::metadata(&second.jsonl_path).ok();
        match (first_meta, second_meta) {
            (Some(first_meta), Some(second_meta)) => {
                first_meta
                    .modified()
                    .ok()
                    .cmp(&second_meta.modified().ok())
            }
            _ => std::cmp::Ordering::Equal,
        }
    });

    Ok(subagents)
}

#[tauri::command]
pub fn get_subagent_messages(jsonl_path: String) -> Result<Vec<ConversationMessage>, String> {
    // Subagent transcripts share the same JSONL shape as the parent — reuse the parser.
    // Sidechain sidechains aren't a thing yet (subagents don't spawn subagents); the existing
    // is_sidechain skip in should_skip_entry would drop everything since the entire subagent
    // log has isSidechain=true. So we use a sibling parser that doesn't skip on isSidechain.
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Subagent file not found: {}", jsonl_path));
    }

    let file =
        fs::File::open(&path).map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut messages: Vec<ConversationMessage> = Vec::new();
    let mut current_assistant_text = String::new();
    let mut current_assistant_timestamp = String::new();
    let mut in_assistant_turn = false;
    let mut interrupt_active = false;
    let mut pending_assistant: Option<JsonlEntry> = None;
    let empty_map = std::collections::HashMap::new();
    // Subagent logs don't contain user-queued messages, but the attachment
    // dispatch needs a set — an empty one means no dedup is applied.
    let queued_dedup: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_task_notifications: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let entry: JsonlEntry = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        // Same skip rules EXCEPT we allow isSidechain (whole subagent log is sidechain)
        if let Some(ref message) = entry.message {
            if message.model.as_deref() == Some("<synthetic>") {
                continue;
            }
        }
        if matches!(
            entry.entry_type.as_deref(),
            Some("system") | Some("summary") | Some("file-history-snapshot") | Some("queue-operation")
        ) {
            continue;
        }

        let entry_type = match &entry.entry_type {
            Some(entry_type) => entry_type.as_str(),
            None => continue,
        };

        if entry_type == "assistant" {
            if let (Some(pending), Some(current_rid)) =
                (pending_assistant.as_ref(), entry.request_id.as_ref())
            {
                if pending.request_id.as_ref() == Some(current_rid) {
                    pending_assistant = Some(entry);
                    continue;
                }
            }
            if let Some(previous) = pending_assistant.take() {
                accumulate_assistant_with_map(
                    previous,
                    &mut current_assistant_text,
                    &mut current_assistant_timestamp,
                    &mut in_assistant_turn,
                    &mut interrupt_active,
                    &empty_map,
                );
            }
            pending_assistant = Some(entry);
            continue;
        }

        if let Some(previous) = pending_assistant.take() {
            accumulate_assistant_with_map(
                previous,
                &mut current_assistant_text,
                &mut current_assistant_timestamp,
                &mut in_assistant_turn,
                &mut interrupt_active,
                &empty_map,
            );
        }

        if entry_type == "user" {
            process_user_entry(
                entry,
                &mut messages,
                &mut current_assistant_text,
                &mut current_assistant_timestamp,
                &mut in_assistant_turn,
                &mut interrupt_active,
                &mut seen_task_notifications,
            );
        } else if entry_type == "attachment" {
            process_attachment_entry(
                entry,
                &mut messages,
                &mut current_assistant_text,
                &mut current_assistant_timestamp,
                &mut in_assistant_turn,
                &queued_dedup,
                &mut seen_task_notifications,
            );
        }
    }

    if let Some(final_entry) = pending_assistant.take() {
        accumulate_assistant_with_map(
            final_entry,
            &mut current_assistant_text,
            &mut current_assistant_timestamp,
            &mut in_assistant_turn,
            &mut interrupt_active,
            &empty_map,
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

#[tauri::command]
pub fn get_session_messages(jsonl_path: String) -> Result<Vec<ConversationMessage>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }

    let tool_to_agent = build_tool_to_agent_map(&path);
    // Normal user-message texts, so a queued_command attachment that also appears
    // as a normal user turn isn't surfaced twice.
    let queued_dedup = collect_normal_user_texts(&path);

    let file =
        fs::File::open(&path).map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut messages: Vec<ConversationMessage> = Vec::new();
    let mut current_assistant_text = String::new();
    let mut current_assistant_timestamp = String::new();
    let mut in_assistant_turn = false;
    // Armed by an interrupt marker, cleared when the next assistant turn begins;
    // while armed, user messages are tagged as mid-turn interjections.
    let mut interrupt_active = false;
    // A background-agent task-notification can be stored as both a delivered user
    // entry and a queued attachment; this shared set ensures it renders once.
    let mut seen_task_notifications: std::collections::HashSet<String> = std::collections::HashSet::new();

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
            if let (Some(pending), Some(current_rid)) =
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
                accumulate_assistant_with_map(
                    previous,
                    &mut current_assistant_text,
                    &mut current_assistant_timestamp,
                    &mut in_assistant_turn,
                    &mut interrupt_active,
                    &tool_to_agent,
                );
            }
            pending_assistant = Some(entry);
            continue;
        }

        // Non-assistant entry: commit any pending assistant first
        if let Some(previous) = pending_assistant.take() {
            accumulate_assistant_with_map(
                previous,
                &mut current_assistant_text,
                &mut current_assistant_timestamp,
                &mut in_assistant_turn,
                &mut interrupt_active,
                &tool_to_agent,
            );
        }

        if entry_type == "user" {
            process_user_entry(
                entry,
                &mut messages,
                &mut current_assistant_text,
                &mut current_assistant_timestamp,
                &mut in_assistant_turn,
                &mut interrupt_active,
                &mut seen_task_notifications,
            );
        } else if entry_type == "attachment" {
            process_attachment_entry(
                entry,
                &mut messages,
                &mut current_assistant_text,
                &mut current_assistant_timestamp,
                &mut in_assistant_turn,
                &queued_dedup,
                &mut seen_task_notifications,
            );
        }
        // Other entry types (system/summary/etc.) are already filtered by should_skip_entry
    }

    // End of file — commit any remaining pending assistant, then flush the turn
    if let Some(final_entry) = pending_assistant.take() {
        accumulate_assistant_with_map(
            final_entry,
            &mut current_assistant_text,
            &mut current_assistant_timestamp,
            &mut in_assistant_turn,
            &mut interrupt_active,
            &tool_to_agent,
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

/// Exact text of the synthetic markers Claude Code writes when the user presses
/// Esc. The marker's content may be a bare string or a `[{type:text,...}]` list,
/// but `extract_user_text` normalises both to this plain string before we match.
fn is_interrupt_marker(text: &str) -> bool {
    matches!(
        text.trim(),
        "[Request interrupted by user]" | "[Request interrupted by user for tool use]"
    )
}

fn accumulate_assistant_with_map(
    entry: JsonlEntry,
    current_text: &mut String,
    current_timestamp: &mut String,
    in_turn: &mut bool,
    interrupt_active: &mut bool,
    tool_to_agent: &std::collections::HashMap<String, String>,
) {
    if !*in_turn {
        *in_turn = true;
        *current_timestamp = entry.timestamp.clone().unwrap_or_default();
        // A fresh assistant turn is starting, so any pending interruption is over —
        // subsequent user messages are ordinary turns again, not mid-turn interjections.
        *interrupt_active = false;
    }
    if let Some(message) = &entry.message {
        let text_parts = extract_assistant_text_with_map(&message.content, tool_to_agent);
        if !text_parts.is_empty() {
            if !current_text.is_empty() {
                current_text.push_str("\n\n");
            }
            current_text.push_str(&text_parts);
        }
    }
}

/// Build MessageImages from the image blocks of a queued_command prompt, pairing
/// them with `[Image #N]` references the same way ordinary user images are paired.
fn attachment_images(prompt: &[Value], text: &str) -> Vec<MessageImage> {
    let image_blocks: Vec<&Value> = prompt
        .iter()
        .filter(|part| part.get("type").and_then(|kind| kind.as_str()) == Some("image"))
        .collect();
    if image_blocks.is_empty() {
        return Vec::new();
    }
    let mut refs: Vec<u32> = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find("[Image #") {
        let start = cursor + found + "[Image #".len();
        if let Some(end_offset) = text[start..].find(']') {
            if let Ok(number) = text[start..start + end_offset].parse::<u32>() {
                refs.push(number);
            }
            cursor = start + end_offset + 1;
        } else {
            break;
        }
    }
    let extras_base = refs.iter().copied().max().unwrap_or(0);
    let mut images = Vec::new();
    for (index, block) in image_blocks.iter().enumerate() {
        let source = match block.get("source") {
            Some(source) => source,
            None => continue,
        };
        let media_type = source
            .get("media_type")
            .and_then(|value| value.as_str())
            .unwrap_or("image/png");
        let data = match source.get("data").and_then(|value| value.as_str()) {
            Some(data) => data,
            None => continue,
        };
        let number = refs.get(index).copied().unwrap_or(extras_base + index as u32 + 1);
        images.push(MessageImage {
            number,
            data_url: format!("data:{};base64,{}", media_type, data),
        });
    }
    images
}

/// Normalize message text for dedup (collapse whitespace + lowercase).
fn normalize_for_dedup(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Pre-pass: normalized text of every ordinary user message. A queued message is
/// sometimes stored BOTH as a `queued_command` attachment AND as a normal user
/// turn — this lets us skip the attachment copy so it isn't shown twice.
fn collect_normal_user_texts(path: &Path) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return set,
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if !line.contains("\"type\":\"user\"") {
            continue;
        }
        let entry: JsonlEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.is_meta.unwrap_or(false) || entry.tool_use_result.is_some() {
            continue;
        }
        if is_tool_result_content(&entry.message) {
            continue;
        }
        let cleaned = strip_system_tags(&extract_user_text(&entry.message));
        let normalized = normalize_for_dedup(&cleaned);
        if !normalized.is_empty() {
            set.insert(normalized);
        }
    }
    set
}

/// Emit an `agent-notification` card for a parsed `<task-notification>`, unless an
/// identical one was already emitted. The same notification can arrive through two
/// channels — a delivered `user` entry and a queued `attachment` (when the agent
/// finished while Claude was mid-turn) — so both handlers dedupe against a shared
/// set keyed on the notification's content. Two genuinely different notifications
/// (an agent that stopped, resumed, and stopped again with a new result) differ in
/// content and both survive. Returns true when a card was pushed.
fn push_task_notification(
    notification: AgentNotification,
    timestamp: String,
    messages: &mut Vec<ConversationMessage>,
    current_assistant_text: &mut String,
    current_assistant_timestamp: &mut str,
    in_assistant_turn: &mut bool,
    seen_task_notifications: &mut std::collections::HashSet<String>,
) -> bool {
    let dedup_key = format!(
        "{}\u{1}{}\u{1}{}",
        notification.summary, notification.status, notification.result
    );
    if !seen_task_notifications.insert(dedup_key) {
        return false;
    }
    flush_assistant(messages, current_assistant_text, current_assistant_timestamp, in_assistant_turn);
    messages.push(ConversationMessage {
        role: "agent-notification".to_string(),
        text: notification.summary.clone(),
        timestamp,
        images: Vec::new(),
        interrupted: false,
        mid_turn: false,
        notification: Some(notification),
    });
    true
}

/// Surface a `queued_command` attachment — a message the user queued while Claude
/// was still working. Claude Code stores it as an attachment rather than a normal
/// user turn, so without this it never appears (and can't be searched). Tagged as
/// mid-turn, since it was sent while Claude was mid-response. Skipped when the same
/// message also exists as a normal user turn (avoids a duplicate). A queued entry
/// can also be a background-agent task-notification, which is surfaced as an agent
/// card (deduped) rather than a user bubble.
fn process_attachment_entry(
    entry: JsonlEntry,
    messages: &mut Vec<ConversationMessage>,
    current_assistant_text: &mut String,
    current_assistant_timestamp: &mut str,
    in_assistant_turn: &mut bool,
    normal_user_texts: &std::collections::HashSet<String>,
    seen_task_notifications: &mut std::collections::HashSet<String>,
) {
    let attachment = match &entry.attachment {
        Some(attachment) => attachment,
        None => return,
    };
    if attachment.get("type").and_then(|kind| kind.as_str()) != Some("queued_command") {
        return;
    }
    // `prompt` is usually a bare string, but can be a [{type:text/image}] array
    // when the queued message included pasted images.
    let (text, images) = match attachment.get("prompt") {
        Some(Value::String(prompt_text)) => (prompt_text.clone(), Vec::new()),
        Some(Value::Array(parts)) => {
            let mut text = String::new();
            for part in parts {
                if part.get("type").and_then(|kind| kind.as_str()) == Some("text") {
                    if let Some(part_text) = part.get("text").and_then(|value| value.as_str()) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(part_text);
                    }
                }
            }
            let images = attachment_images(parts, &text);
            (text, images)
        }
        _ => return,
    };
    // A queued task-notification: a background agent finished mid-turn. Surface it
    // as an agent card (deduped), not a queued user bubble.
    if let Some(notification) = parse_task_notification(&text) {
        push_task_notification(
            notification,
            entry.timestamp.unwrap_or_default(),
            messages,
            current_assistant_text,
            current_assistant_timestamp,
            in_assistant_turn,
            seen_task_notifications,
        );
        return;
    }
    let cleaned = strip_system_tags(&text);
    if cleaned.is_empty() && images.is_empty() {
        return;
    }
    // Already shown as a normal user turn — don't duplicate it.
    if normal_user_texts.contains(&normalize_for_dedup(&cleaned)) {
        return;
    }
    flush_assistant(messages, current_assistant_text, current_assistant_timestamp, in_assistant_turn);
    messages.push(ConversationMessage {
        role: "user".to_string(),
        text: cleaned,
        timestamp: entry.timestamp.unwrap_or_default(),
        images,
        interrupted: false,
        mid_turn: true,
        notification: None,
    });
}

fn process_user_entry(
    entry: JsonlEntry,
    messages: &mut Vec<ConversationMessage>,
    current_assistant_text: &mut String,
    current_assistant_timestamp: &mut str,
    in_assistant_turn: &mut bool,
    interrupt_active: &mut bool,
    seen_task_notifications: &mut std::collections::HashSet<String>,
) {
    // Compaction summaries are special
    if entry.is_compact_summary.unwrap_or(false) {
        flush_assistant(messages, current_assistant_text, current_assistant_timestamp, in_assistant_turn);
        *interrupt_active = false;
        let text = extract_user_text(&entry.message);
        if !text.is_empty() {
            messages.push(ConversationMessage {
                role: "compaction".to_string(),
                text,
                timestamp: entry.timestamp.unwrap_or_default(),
                images: Vec::new(),
                interrupted: false,
                mid_turn: false,
                notification: None,
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

    // Interrupt marker: this isn't a real user message — it's the synthetic record
    // of the user pressing Esc. Flush the assistant reply that was in flight and
    // tag it as interrupted, then arm `interrupt_active` so the user's actual
    // follow-up message(s) get flagged as mid-turn. The marker itself is dropped.
    if is_interrupt_marker(&text) {
        let before = messages.len();
        flush_assistant(messages, current_assistant_text, current_assistant_timestamp, in_assistant_turn);
        if messages.len() > before {
            if let Some(last) = messages.last_mut() {
                if last.role == "assistant" {
                    last.interrupted = true;
                }
            }
        }
        *interrupt_active = true;
        return;
    }

    // Task notification: a background subagent / workflow agent finished. Claude
    // Code injects this as a `user`-role entry, but the user didn't type it — it's
    // a system event. Surface it as its own `agent-notification` card instead of a
    // user bubble, so the agent's output doesn't masquerade as a message the user sent.
    if let Some(notification) = parse_task_notification(&text) {
        push_task_notification(
            notification,
            entry.timestamp.unwrap_or_default(),
            messages,
            current_assistant_text,
            current_assistant_timestamp,
            in_assistant_turn,
            seen_task_notifications,
        );
        return;
    }

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
        interrupted: false,
        mid_turn: *interrupt_active,
        notification: None,
    });
}

fn flush_assistant(
    messages: &mut Vec<ConversationMessage>,
    current_text: &mut String,
    current_timestamp: &mut str,
    in_turn: &mut bool,
) {
    if *in_turn && !current_text.is_empty() {
        messages.push(ConversationMessage {
            role: "assistant".to_string(),
            text: current_text.clone(),
            timestamp: current_timestamp.to_string(),
            images: Vec::new(),
            interrupted: false,
            mid_turn: false,
            notification: None,
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
    // Slash-command invocations are stored as
    //   <command-name>/foo</command-name> <command-message>foo</command-message> <command-args>ARGS</command-args>
    // The args hold what the user actually typed after the command (often a long
    // prompt, e.g. `/compact <detailed instructions>`), so they MUST be preserved.
    // Reconstruct the command as "/foo ARGS" — this both keeps the content and reads
    // cleanly, instead of leaving the inter-tag whitespace a generic strip would.
    if let Some(reconstructed) = reconstruct_slash_command(text) {
        return strip_ansi(&reconstructed).trim().to_string();
    }

    // Tags whose entire block is noise and should be removed wholesale.
    const DROP_TAGS: &[&str] = &[
        "system-reminder",
        "local-command-caveat",
        "local-command-stdout",
        "local-command-stderr",
    ];
    // Tags whose wrapper is removed but inner text kept. command-args is included as
    // a safety net so any malformed command entry still keeps the user's content.
    const UNWRAP_TAGS: &[&str] = &["command-name", "command-message", "command-args"];

    let mut result = strip_paired_tags(text, DROP_TAGS, false);
    result = strip_paired_tags(&result, UNWRAP_TAGS, true);
    strip_ansi(&result).trim().to_string()
}

/// If `text` is a slash-command invocation, rebuild it as "<name> <args>".
/// Returns None for ordinary messages (no `<command-name>` tag).
fn reconstruct_slash_command(text: &str) -> Option<String> {
    let name = extract_tag_inner(text, "command-name")?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let args = extract_tag_inner(text, "command-args").unwrap_or_default();
    let args = args.trim();
    if args.is_empty() {
        Some(name.to_string())
    } else {
        Some(format!("{} {}", name, args))
    }
}

/// Parse a `<task-notification>` transcript entry into an `AgentNotification`.
/// Returns None when `text` isn't one. A single wrapper covers all variants:
/// single subagents (usage = subagent_tokens/tool_uses/duration_ms), dynamic
/// workflows, and multi-agent fan-outs (agent_count/agents_done/agents_error).
fn parse_task_notification(text: &str) -> Option<AgentNotification> {
    if !text.trim_start().starts_with("<task-notification>") {
        return None;
    }
    let parse_u64 =
        |tag: &str| extract_tag_inner(text, tag).and_then(|value| value.trim().parse::<u64>().ok());
    Some(AgentNotification {
        summary: extract_tag_inner(text, "summary").unwrap_or_default().trim().to_string(),
        status: extract_tag_inner(text, "status").unwrap_or_default().trim().to_string(),
        result: extract_tag_inner(text, "result")
            .map(|body| decode_xml_entities(body.trim()))
            .unwrap_or_default(),
        tokens: parse_u64("subagent_tokens"),
        tool_uses: parse_u64("tool_uses"),
        duration_ms: parse_u64("duration_ms"),
        agent_count: parse_u64("agent_count"),
        agents_done: parse_u64("agents_done"),
        agents_error: parse_u64("agents_error"),
    })
}

/// Decode the small set of XML entities Claude Code escapes inside stored
/// `<result>` bodies so they read naturally. `&amp;` is decoded last so a
/// double-escaped `&amp;lt;` resolves to `&lt;`, not `<`. Rendered as plain text
/// (the frontend re-escapes on display), so this is display sugar, not a parser.
fn decode_xml_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Return the inner text of the first `<tag>...</tag>` pair, or None if not present
/// (or unclosed). Used to safely pull slash-command fields without dropping content.
fn extract_tag_inner(text: &str, tag: &str) -> Option<String> {
    let open_tag = format!("<{}>", tag);
    let close_tag = format!("</{}>", tag);
    let inner_start = text.find(&open_tag)? + open_tag.len();
    let inner_end = text[inner_start..].find(&close_tag)? + inner_start;
    Some(text[inner_start..inner_end].to_string())
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
                if earliest.is_none_or(|(earlier, _)| abs < earlier) {
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
                        // Unclosed tag: don't silently swallow the rest of the message
                        // (that loses real user content). Keep everything from the open
                        // tag onward as literal text instead.
                        output.push_str(&text[open_start..]);
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

fn get_file_mtime_iso(path: &Path) -> Option<String> {
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
    let empty_map = std::collections::HashMap::new();
    extract_assistant_text_with_map(content, &empty_map)
}

fn extract_assistant_text_with_map(
    content: &Option<Value>,
    tool_to_agent: &std::collections::HashMap<String, String>,
) -> String {
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
                            if let Some(formatted) = format_tool_use(block, tool_to_agent) {
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

fn format_tool_use(
    block: &Value,
    tool_to_agent: &std::collections::HashMap<String, String>,
) -> Option<String> {
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
        "TaskCreate" | "TaskGet" | "TaskList" => {
            let subject = input.get("subject").and_then(|subj| subj.as_str()).unwrap_or("");
            subject.to_string()
        }
        "TaskUpdate" => {
            // Usually just taskId + status; fall back to whichever is present.
            if let Some(subject) = input.get("subject").and_then(|subj| subj.as_str()) {
                subject.to_string()
            } else {
                let task_id = input.get("taskId").and_then(|val| val.as_str()).unwrap_or("");
                let status = input.get("status").and_then(|val| val.as_str()).unwrap_or("");
                format!("#{} {}", task_id, status).trim().to_string()
            }
        }
        "TaskStop" => {
            input.get("task_id").and_then(|val| val.as_str()).unwrap_or("").to_string()
        }
        "Skill" | "skill" => {
            let skill_name = input.get("skill").and_then(|skill| skill.as_str()).unwrap_or("");
            skill_name.to_string()
        }
        "Workflow" => extract_workflow_label(input),
        "WebSearch" => {
            input.get("query").and_then(|val| val.as_str()).unwrap_or("").to_string()
        }
        "WebFetch" => {
            input.get("url").and_then(|val| val.as_str()).unwrap_or("").to_string()
        }
        "ToolSearch" => {
            input.get("query").and_then(|val| val.as_str()).unwrap_or("").to_string()
        }
        "AskUserQuestion" => {
            // New shape: { questions: [{ question, ... }] }; older shape: { question }.
            input
                .get("questions")
                .and_then(|val| val.as_array())
                .and_then(|questions| questions.first())
                .and_then(|first| first.get("question"))
                .and_then(|val| val.as_str())
                .or_else(|| input.get("question").and_then(|val| val.as_str()))
                .unwrap_or("")
                .to_string()
        }
        "Monitor" => input
            .get("command")
            .or_else(|| input.get("description"))
            .or_else(|| input.get("bash_id"))
            .and_then(|val| val.as_str())
            .unwrap_or("")
            .to_string(),
        "ScheduleWakeup" => {
            input.get("reason").and_then(|val| val.as_str()).unwrap_or("").to_string()
        }
        _ => String::new(),
    };

    // Keep pill summaries tidy — long inputs (questions, URLs, reasons) get capped.
    // The frontend also ellipsizes, but capping here keeps message.text from bloating.
    let summary: String = summary.chars().take(200).collect();

    // Escape pipe in summary to avoid breaking the marker format
    let safe_summary = summary.replace('|', "/");

    // Marker format: {{TOOL:name|summary[|toolUseId[|agentId]]}}
    // Third field = tool_use_id (always present when available), enables inline result expansion.
    // Fourth field = agentId (Agent calls only), enables opening the subagent transcript.
    let tool_use_id = block.get("id").and_then(|val| val.as_str()).unwrap_or("");
    let mut suffix = String::new();
    if !tool_use_id.is_empty() {
        suffix.push('|');
        suffix.push_str(tool_use_id);
        if matches!(tool_name, "Agent" | "agent") {
            if let Some(agent_id) = tool_to_agent.get(tool_use_id) {
                suffix.push('|');
                suffix.push_str(agent_id);
            }
        }
    }

    Some(format!(
        "{{{{TOOL:{}|{}{}}}}}",
        tool_name, safe_summary, suffix
    ))
}

/// Pick the best human label for a Workflow tool call. Inline scripts start with
/// `export const meta = { name: '...' }`, so prefer that name; otherwise fall back
/// to the description, the script file's basename, or a generic label.
fn extract_workflow_label(input: &Value) -> String {
    if let Some(script) = input.get("script").and_then(|val| val.as_str()) {
        if let Some(name) = extract_meta_name(script) {
            return name;
        }
    }
    if let Some(description) = input.get("description").and_then(|val| val.as_str()) {
        if !description.is_empty() {
            return description.to_string();
        }
    }
    if let Some(script_path) = input.get("scriptPath").and_then(|val| val.as_str()) {
        return script_path.rsplit('/').next().unwrap_or(script_path).to_string();
    }
    "workflow".to_string()
}

/// Extract the first `name: '<value>'` from a workflow script's meta block.
fn extract_meta_name(script: &str) -> Option<String> {
    let after_name = &script[script.find("name:")? + "name:".len()..];
    let trimmed = after_name.trim_start();
    let quote = trimmed.chars().next()?;
    if quote != '\'' && quote != '"' && quote != '`' {
        return None;
    }
    let rest = &trimmed[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

struct SessionQuickMetadata {
    custom_title: Option<String>,
    ai_title: Option<String>,
    first_prompt: Option<String>,
    first_timestamp: Option<String>,
    last_timestamp: Option<String>,
    conversation_count: u64,
    total_tokens: u64,
}

fn extract_quick_metadata(jsonl_path: &PathBuf) -> SessionQuickMetadata {
    let mut custom_title: Option<String> = None;
    let mut ai_title: Option<String> = None;
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
                ai_title,
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

        // AI-generated title (Claude Code emits this once it has enough context)
        if line.contains("\"type\":\"ai-title\"") {
            if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
                if let Some(title) = entry.ai_title {
                    ai_title = Some(title);
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
        ai_title,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_command_args_are_preserved() {
        // Regression for the copy/display truncation bug: a /compact prompt's long
        // argument must survive intact instead of collapsing to the bare command.
        let input = "<command-name>/compact</command-name>   <command-message>compact</command-message>   <command-args>Summarize the multi-phase redesign in detail and keep every decision</command-args>";
        assert_eq!(
            strip_system_tags(input),
            "/compact Summarize the multi-phase redesign in detail and keep every decision"
        );
    }

    #[test]
    fn slash_command_without_args() {
        let input = "<command-name>/clear</command-name>   <command-message>clear</command-message>";
        assert_eq!(strip_system_tags(input), "/clear");
    }

    #[test]
    fn local_command_stdout_is_dropped() {
        let input = "<local-command-stdout>Login successful</local-command-stdout>";
        assert_eq!(strip_system_tags(input), "");
    }

    #[test]
    fn normal_long_prompt_is_unchanged() {
        let input = "Please refactor the parser and make sure we handle every edge case carefully across all the files.";
        assert_eq!(strip_system_tags(input), input);
    }

    #[test]
    fn closed_system_reminder_is_removed_but_text_kept() {
        let input = "Real question here<system-reminder>injected noise</system-reminder> and more text";
        assert_eq!(strip_system_tags(input), "Real question here and more text");
    }

    #[test]
    fn task_notification_is_parsed_into_agent_card() {
        let input = "<task-notification>\n<task-id>ac7c715edf413a68a</task-id>\n<status>completed</status>\n<summary>Agent \"Map domain model\" finished</summary>\n<result>Report body with &lt;Entity&gt; refs &amp; more.</result>\n<usage><subagent_tokens>122641</subagent_tokens><tool_uses>72</tool_uses><duration_ms>267945</duration_ms></usage>\n</task-notification>";
        let notification = parse_task_notification(input).expect("should parse");
        assert_eq!(notification.summary, "Agent \"Map domain model\" finished");
        assert_eq!(notification.status, "completed");
        assert_eq!(notification.result, "Report body with <Entity> refs & more.");
        assert_eq!(notification.tokens, Some(122641));
        assert_eq!(notification.tool_uses, Some(72));
        assert_eq!(notification.duration_ms, Some(267945));
    }

    #[test]
    fn non_task_notification_text_is_ignored() {
        assert!(parse_task_notification("just a normal user message").is_none());
        assert!(parse_task_notification("mentions <task-notification> mid-sentence only").is_none());
    }

    #[test]
    fn double_escaped_entity_decodes_to_single() {
        // `&amp;lt;` must resolve to the literal `&lt;`, not to `<`.
        assert_eq!(decode_xml_entities("&amp;lt;"), "&lt;");
    }

    fn note_card(summary: &str, status: &str, result: &str) -> AgentNotification {
        AgentNotification {
            summary: summary.to_string(),
            status: status.to_string(),
            result: result.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn identical_task_notifications_dedupe_to_one_card() {
        // The same notification arrives as both a delivered `user` entry and a
        // queued `attachment`; only one card should result.
        let mut messages = Vec::new();
        let (mut text, mut timestamp, mut in_turn) = (String::new(), String::new(), false);
        let mut seen = std::collections::HashSet::new();
        let pushed_first = push_task_notification(
            note_card("Agent \"X\" finished", "completed", "body"),
            "t1".to_string(), &mut messages, &mut text, &mut timestamp, &mut in_turn, &mut seen,
        );
        let pushed_second = push_task_notification(
            note_card("Agent \"X\" finished", "completed", "body"),
            "t2".to_string(), &mut messages, &mut text, &mut timestamp, &mut in_turn, &mut seen,
        );
        assert!(pushed_first);
        assert!(!pushed_second);
        assert_eq!(messages.iter().filter(|message| message.role == "agent-notification").count(), 1);
    }

    #[test]
    fn task_notifications_with_different_content_both_survive() {
        // A task-id that notifies twice with a different result is two real events.
        let mut messages = Vec::new();
        let (mut text, mut timestamp, mut in_turn) = (String::new(), String::new(), false);
        let mut seen = std::collections::HashSet::new();
        push_task_notification(
            note_card("Agent \"X\" finished", "completed", "first run"),
            "t1".to_string(), &mut messages, &mut text, &mut timestamp, &mut in_turn, &mut seen,
        );
        push_task_notification(
            note_card("Agent \"X\" finished", "completed", "second run"),
            "t2".to_string(), &mut messages, &mut text, &mut timestamp, &mut in_turn, &mut seen,
        );
        assert_eq!(messages.iter().filter(|message| message.role == "agent-notification").count(), 2);
    }

    #[test]
    fn workflow_label_prefers_meta_name() {
        let script = "export const meta = {\n  name: 'canvas-restructure-audit',\n  description: 'Audit the Canvas module',\n}";
        let input = serde_json::json!({ "script": script });
        assert_eq!(extract_workflow_label(&input), "canvas-restructure-audit");
    }

    #[test]
    fn workflow_label_falls_back_to_script_path() {
        let input = serde_json::json!({ "scriptPath": "/tmp/session/wf-review.js" });
        assert_eq!(extract_workflow_label(&input), "wf-review.js");
    }

    #[test]
    fn unclosed_drop_tag_does_not_swallow_following_content() {
        // Hardening: an unclosed <system-reminder> must not delete everything after it.
        let input = "Important user content that is quite long <system-reminder> oops no close tag here";
        let out = strip_system_tags(input);
        assert!(out.contains("Important user content that is quite long"), "got: {out:?}");
        assert!(out.contains("oops no close tag here"), "got: {out:?}");
    }

    #[test]
    fn interrupt_marker_is_recognised_in_both_shapes() {
        assert!(is_interrupt_marker("[Request interrupted by user]"));
        assert!(is_interrupt_marker("  [Request interrupted by user for tool use]  "));
        assert!(!is_interrupt_marker("please don't interrupt the build"));
        assert!(!is_interrupt_marker(
            "here is why [Request interrupted by user] happened"
        ));
    }

    #[test]
    fn mid_turn_interrupt_sequence_is_parsed() {
        // Reproduces a real Esc-mid-response sequence: a partial assistant reply,
        // the synthetic interrupt marker (as a text-block list), then two follow-up
        // user messages fired before Claude resumed, then a resumed assistant turn
        // followed by an ordinary user message.
        let lines = [
            r#"{"type":"assistant","requestId":"req_a","timestamp":"2026-08-11T11:40:08.663Z","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"Here is the design language I need to match"}]}}"#,
            r#"{"type":"user","timestamp":"2026-08-11T11:40:08.664Z","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
            r#"{"type":"user","timestamp":"2026-08-11T11:40:08.683Z","message":{"role":"user","content":"think from the user's perspective and add the styles"}}"#,
            r#"{"type":"user","timestamp":"2026-08-11T11:40:08.684Z","message":{"role":"user","content":"without heavy things also"}}"#,
            r#"{"type":"assistant","requestId":"req_b","timestamp":"2026-08-11T11:41:00.000Z","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"Got it — reworking the styles now."}]}}"#,
            r#"{"type":"user","timestamp":"2026-08-11T11:42:00.000Z","message":{"role":"user","content":"looks great, ship it"}}"#,
        ];

        let dir = std::env::temp_dir();
        let path = dir.join(format!("claude_sessions_interrupt_test_{}.jsonl", std::process::id()));
        fs::write(&path, lines.join("\n")).unwrap();

        let messages = get_session_messages(path.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_file(&path);

        // The raw "[Request interrupted by user]" marker must never surface as a bubble.
        assert!(
            !messages.iter().any(|m| m.text.contains("[Request interrupted by user]")),
            "marker leaked into output: {messages:#?}"
        );

        let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            roles,
            ["assistant", "user", "user", "assistant", "user"],
            "unexpected message sequence: {messages:#?}"
        );

        // The cut-off assistant reply is flagged interrupted; the resumed one is not.
        assert!(messages[0].interrupted, "first assistant reply should be interrupted");
        assert!(!messages[3].interrupted, "resumed assistant reply should not be interrupted");

        // Both follow-up user messages are mid-turn; the later normal one is not.
        assert!(messages[1].mid_turn, "first follow-up should be mid_turn");
        assert!(messages[2].mid_turn, "second follow-up should be mid_turn");
        assert!(!messages[4].mid_turn, "post-resume user message should not be mid_turn");
    }

    #[test]
    fn askuserquestion_answer_is_parsed_with_chosen_option() {
        // Mirrors a real toolUseResult: questions + options + an `answers` map.
        let tool_use_result = serde_json::json!({
            "questions": [{
                "question": "Which direction should we take?",
                "header": "Direction",
                "multiSelect": false,
                "options": [
                    { "label": "Option A", "description": "the first path" },
                    { "label": "Option B", "description": "the second path" }
                ]
            }],
            "answers": { "Which direction should we take?": "Option B" }
        });

        let questions = tool_use_result["questions"].as_array().unwrap();
        let parsed = parse_answered_questions(
            questions,
            tool_use_result.get("answers"),
            tool_use_result.get("annotations"),
        );

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].header, "Direction");
        assert!(parsed[0].notes.is_none());
        assert!(!parsed[0].multi_select);
        let chosen: Vec<&str> = parsed[0]
            .options
            .iter()
            .filter(|option| option.chosen)
            .map(|option| option.label.as_str())
            .collect();
        assert_eq!(chosen, ["Option B"], "only the picked option is chosen");
        assert!(parsed[0].options.iter().all(|option| !option.custom));
    }

    #[test]
    fn askuserquestion_custom_other_answer_is_surfaced() {
        // The user picked "Other" and typed text that matches no offered label.
        let tool_use_result = serde_json::json!({
            "questions": [{
                "question": "Pick a colour",
                "header": "Colour",
                "multiSelect": false,
                "options": [{ "label": "Red", "description": "" }]
            }],
            "answers": { "Pick a colour": "Chartreuse, actually" }
        });
        let questions = tool_use_result["questions"].as_array().unwrap();
        let parsed = parse_answered_questions(
            questions,
            tool_use_result.get("answers"),
            tool_use_result.get("annotations"),
        );

        let custom: Vec<&AnsweredOption> =
            parsed[0].options.iter().filter(|option| option.custom).collect();
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].label, "Chartreuse, actually");
        assert!(custom[0].chosen);
        // The offered "Red" option remains, unchosen.
        assert!(parsed[0].options.iter().any(|option| option.label == "Red" && !option.chosen));
    }

    #[test]
    fn multiselect_answers_and_per_question_notes_are_parsed() {
        // Two questions: one multi-select with two chosen, one note-only (no option
        // picked — the note is the whole answer, like the real theme question).
        let tool_use_result = serde_json::json!({
            "questions": [
                {
                    "question": "Which features to enable?",
                    "header": "Features",
                    "multiSelect": true,
                    "options": [
                        { "label": "Search", "description": "" },
                        { "label": "Bookmarks", "description": "" },
                        { "label": "Export", "description": "" }
                    ]
                },
                {
                    "question": "Anything else?",
                    "header": "Notes",
                    "multiSelect": false,
                    "options": [{ "label": "Nope", "description": "" }]
                }
            ],
            // Real multi-select format: chosen labels joined as one "A, B" string.
            "answers": { "Which features to enable?": "Search, Export" },
            "annotations": { "Anything else?": { "notes": "please also add tags" } }
        });
        let questions = tool_use_result["questions"].as_array().unwrap();
        let parsed = parse_answered_questions(
            questions,
            tool_use_result.get("answers"),
            tool_use_result.get("annotations"),
        );

        assert_eq!(parsed.len(), 2);
        // Q1: multi-select, both Search + Export chosen, Bookmarks not.
        assert!(parsed[0].multi_select);
        let chosen: Vec<&str> = parsed[0]
            .options
            .iter()
            .filter(|option| option.chosen)
            .map(|option| option.label.as_str())
            .collect();
        assert_eq!(chosen, ["Search", "Export"]);
        assert!(parsed[0].notes.is_none());
        // Q2: no option chosen, but the note carries the real answer.
        assert!(parsed[1].options.iter().all(|option| !option.chosen));
        assert_eq!(parsed[1].notes.as_deref(), Some("please also add tags"));
    }

    #[test]
    fn published_artifact_is_extracted_by_tool_use_id() {
        // Mirrors a real Artifact publish: tool_use (Artifact) + a tool_result whose
        // toolUseResult carries {url, path, title}.
        let lines = [
            r#"{"type":"assistant","timestamp":"2026-08-12T10:00:00Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_art1","name":"Artifact","input":{"file_path":"/tmp/showcase.html"}}]}}"#,
            r#"{"type":"user","timestamp":"2026-08-12T10:00:01Z","toolUseResult":{"url":"https://claude.ai/code/artifact/ea746622-54d5-4f18-974c-935af34e4e85","path":"/tmp/showcase.html","title":"Signal Showcase"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_art1","content":"Published /tmp/showcase.html at https://claude.ai/code/artifact/ea746622-54d5-4f18-974c-935af34e4e85"}]}}"#,
        ];
        let dir = std::env::temp_dir();
        let path = dir.join(format!("claude_sessions_artifact_test_{}.jsonl", std::process::id()));
        fs::write(&path, lines.join("\n")).unwrap();

        let artifacts = get_session_artifacts(path.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_file(&path);

        let artifact = artifacts.get("toolu_art1").expect("artifact keyed by tool_use_id");
        assert_eq!(artifact.title, "Signal Showcase");
        assert!(artifact.url.contains("/code/artifact/ea746622"));
        assert_eq!(artifact.path, "/tmp/showcase.html");
    }

    #[test]
    fn queued_command_attachment_is_surfaced_as_mid_turn() {
        // A message the user queued while Claude was working: stored as an
        // `attachment` of type queued_command, not a normal user turn. Mirrors the
        // real shape (the "branch feature" message). It must appear + be mid_turn.
        let lines = [
            r#"{"type":"user","timestamp":"2026-08-12T05:00:00Z","message":{"role":"user","content":"kick off the task"}}"#,
            r#"{"type":"assistant","requestId":"req_a","timestamp":"2026-08-12T05:10:00Z","message":{"role":"assistant","content":[{"type":"text","text":"Working on it."}]}}"#,
            r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-08-12T05:26:36Z","content":"And one more feature — branch support"}"#,
            // array-prompt form (queued message with a pasted image)
            r#"{"type":"attachment","isSidechain":false,"timestamp":"2026-08-12T05:26:49Z","attachment":{"type":"queued_command","prompt":[{"type":"text","text":"And one more important feature — branch support"}]}}"#,
            // string-prompt form (the common case — 94% of queued messages)
            r#"{"type":"attachment","isSidechain":false,"timestamp":"2026-08-12T05:27:10Z","attachment":{"type":"queued_command","prompt":"also make the quality full, not dimmed"}}"#,
            // a non-message attachment that must be ignored
            r#"{"type":"attachment","isSidechain":false,"timestamp":"2026-08-12T05:27:20Z","attachment":{"type":"task_reminder","content":[],"itemCount":0}}"#,
            // DEDUP case: queued as BOTH a normal user turn AND an attachment — show once
            r#"{"type":"user","timestamp":"2026-08-12T05:28:00Z","message":{"role":"user","content":"and please add a dark theme toggle"}}"#,
            r#"{"type":"attachment","isSidechain":false,"timestamp":"2026-08-12T05:28:01Z","attachment":{"type":"queued_command","prompt":"and please add a dark theme toggle"}}"#,
        ];
        let dir = std::env::temp_dir();
        let path = dir.join(format!("cs_queued_test_{}.jsonl", std::process::id()));
        fs::write(&path, lines.join("\n")).unwrap();

        let messages = get_session_messages(path.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_file(&path);

        // Both the array-prompt and string-prompt queued messages are surfaced + mid_turn.
        let array_msg = messages.iter().find(|m| m.text.contains("branch support")).expect("array-prompt queued message");
        assert_eq!(array_msg.role, "user");
        assert!(array_msg.mid_turn);
        let string_msg = messages.iter().find(|m| m.text.contains("full, not dimmed")).expect("string-prompt queued message");
        assert!(string_msg.mid_turn, "string-prompt queued message should be mid_turn");
        // No duplication from the queue-operation bookkeeping entry.
        assert_eq!(messages.iter().filter(|m| m.text.contains("branch support")).count(), 1);
        // The task_reminder attachment must NOT become a message.
        assert!(!messages.iter().any(|m| m.text.contains("itemCount")));
        // Dedup: the message stored as BOTH a user turn and an attachment appears once.
        assert_eq!(
            messages.iter().filter(|m| m.text.contains("dark theme toggle")).count(),
            1,
            "a queued message stored as both a user turn and an attachment must not duplicate"
        );
    }

    #[test]
    fn archive_copies_transcript_subagents_and_meta_in_mirrored_layout() {
        let session_id = format!("sess-{}", std::process::id());
        let base = std::env::temp_dir().join(format!("cs_archive_test_{}", std::process::id()));
        let live_project = base.join("live");
        let archive_root = base.join("archive");
        // Live layout: <project>/<id>.jsonl + <project>/<id>/subagents/agent-*.jsonl
        fs::create_dir_all(live_project.join(&session_id).join("subagents")).unwrap();
        let source_jsonl = live_project.join(format!("{}.jsonl", session_id));
        fs::write(&source_jsonl, "{\"type\":\"user\"}\n").unwrap();
        fs::write(
            live_project.join(&session_id).join("subagents").join("agent-x.jsonl"),
            "{\"isSidechain\":true}\n",
        )
        .unwrap();

        let meta = serde_json::json!({ "session_id": session_id, "title": "Test" });
        archive_session_to(&archive_root, &source_jsonl, &session_id, None, &meta).unwrap();

        // Mirrored layout: <archive>/<id>/<id>.jsonl and .../<id>/subagents/agent-x.jsonl
        let archived_jsonl = archive_root.join(&session_id).join(format!("{}.jsonl", session_id));
        let archived_agent = archive_root
            .join(&session_id)
            .join(&session_id)
            .join("subagents")
            .join("agent-x.jsonl");
        let archived_meta = archive_root.join(&session_id).join("meta.json");

        assert!(archived_jsonl.exists(), "transcript should be archived");
        assert!(archived_agent.exists(), "subagent log should be archived");
        assert!(archived_meta.exists(), "meta.json should be written");
        // Subagent path derivation (parentDir + <id> + subagents) resolves inside the archive.
        assert_eq!(
            archived_jsonl.parent().unwrap().join(&session_id).join("subagents").join("agent-x.jsonl"),
            archived_agent
        );

        let _ = fs::remove_dir_all(&base);
    }
}
