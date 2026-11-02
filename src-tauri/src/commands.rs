use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use tauri::Manager;

#[path = "codex.rs"]
mod codex;

// ── Types returned to the frontend ──────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct ProjectInfo {
    /// False when the folder has been moved or deleted; its history is still here.
    pub folder_exists: bool,
    /// Every tool with sessions in this folder ("claude", "codex").
    pub tools: Vec<String>,
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
    /// Which agent wrote this session: "claude" or "codex".
    pub tool: String,
    /// Archived inside the tool itself, which is different from saved in Evertusk.
    pub archived_in_tool: bool,
    pub summary: Option<String>,
    pub custom_title: Option<String>,
    pub ai_title: Option<String>,
    pub first_prompt: Option<String>,
    pub project_path: String,
    pub project_name: String,
    pub created: Option<String>,
    /// Time of the last real message, falling back to the file's time.
    pub modified: Option<String>,
    /// The file's modified time. Only for the "Now" lamp and New activity: tools append
    /// metadata long after the last message, so this isn't when the session last ran.
    pub last_written: Option<String>,
    pub conversation_count: u64,
    pub git_branch: Option<String>,
    /// Set when this session was created by forking/branching another session
    /// (`/branch` in Claude Code). Holds the parent session's id; the frontend
    /// resolves it to the parent's title and shows a "fork of …" badge.
    pub forked_from_session_id: Option<String>,
    pub jsonl_path: String,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct ConversationMessage {
    /// "user", "assistant", "compaction", "agent-notification", "shell" or "system-event".
    pub role: String,
    /// Plain searchable text. For assistant turns this is derived from `blocks`.
    pub text: String,
    pub timestamp: String,
    /// The ordered content of an assistant turn.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<MessageBlock>,
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
    /// True on a message a fork copied from its parent, not this session's own work.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inherited: bool,
    /// Present only when `role == "agent-notification"`: a background task finishing
    /// or a subagent reporting back. Claude Code injects these with `type:"user"`, but
    /// they are not something the user typed, so they render as a card, not a bubble.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification: Option<AgentNotification>,
    /// Present only when `role == "shell"`: a command the user ran with `!`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<ShellRun>,
    /// Present only when `role == "system-event"`: hooks, API errors, recaps, notices.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<SystemEvent>,
    /// Present only when `role == "compaction"`: its trigger and token counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction_detail: Option<String>,
}

/// One piece of an assistant turn. Structured rather than encoded into a string, so
/// tool input (pipes, braces, literal markers in prose) can never corrupt rendering.
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum MessageBlock {
    Text { text: String },
    Thinking { text: String },
    Tool {
        name: String,
        summary: String,
        /// Human-readable tool input, capped. Empty when the summary already says it all.
        input: String,
        /// True when `input` was cut; `get_tool_input` returns all of it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        input_truncated: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_use_id: Option<String>,
        /// Agent id (Agent calls) or workflow run id (Workflow calls).
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
    },
    Plan { items: Vec<PlanItem> },
    /// A labelled aside: the end-of-turn recap or a chapter marker.
    Note { label: String, text: String },
    /// Hook output during a turn. Kept in the turn so it doesn't split Claude's message.
    Hook { label: String, text: String, is_error: bool },
}

#[derive(Debug, Serialize, Clone)]
pub struct PlanItem {
    pub content: String,
    pub status: String,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct ShellRun {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct SystemEvent {
    /// "hook", "api-error", "recap", "command", "files", "notice" or "fork".
    pub kind: String,
    /// True for failures, so the UI can tint them.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
    /// Longer content shown collapsed under the one-line `text`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// A background task finishing or a subagent reporting back, parsed from a
/// `<task-notification>` or a peer hand-back. Covers subagents, background shells,
/// Monitor events, dynamic workflows and multi-agent fan-outs.
#[derive(Debug, Serialize, Clone, Default)]
pub struct AgentNotification {
    /// Human summary, e.g. `Agent "Map the database domain model" finished`.
    pub summary: String,
    /// `completed`, `failed`, `stopped`, `event`, etc.
    pub status: String,
    /// "agent", "monitor", "shell", "hand-back", "message" or "task".
    pub kind: String,
    /// The task's full returned output. May be large; the UI keeps it collapsed.
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
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonlEntry {
    #[serde(rename = "type", default, deserialize_with = "loose")]
    entry_type: Option<String>,
    #[serde(rename = "isSidechain", default, deserialize_with = "loose")]
    is_sidechain: Option<bool>,
    #[serde(rename = "isMeta", default, deserialize_with = "loose")]
    is_meta: Option<bool>,
    #[serde(rename = "customTitle", default, deserialize_with = "loose")]
    custom_title: Option<String>,
    #[serde(rename = "aiTitle", default, deserialize_with = "loose")]
    ai_title: Option<String>,
    #[serde(rename = "isCompactSummary", default, deserialize_with = "loose")]
    is_compact_summary: Option<bool>,
    #[serde(default, deserialize_with = "loose")]
    message: Option<JsonlMessage>,
    #[serde(default, deserialize_with = "loose")]
    timestamp: Option<String>,
    #[serde(rename = "requestId", default, deserialize_with = "loose")]
    request_id: Option<String>,
    #[serde(rename = "sessionId", default, deserialize_with = "loose")]
    session_id: Option<String>,
    /// Stamped on every entry a fork copied from its parent.
    #[serde(rename = "forkedFrom")]
    forked_from: Option<Value>,
    #[serde(rename = "parentUuid", default, deserialize_with = "loose")]
    parent_uuid: Option<String>,
    /// Present on `type:"attachment"` entries. A `queued_command` attachment is a
    /// message the user queued while Claude was still working — Claude Code stores
    /// it here rather than as a normal user turn, so we surface it explicitly.
    attachment: Option<Value>,
    /// Who produced a user entry: `human`, `task-notification`, or `peer` (a subagent
    /// or another session reporting back).
    origin: Option<Value>,
    // The remaining fields only appear on `type:"system"` entries.
    #[serde(default, deserialize_with = "loose")]
    subtype: Option<String>,
    content: Option<Value>,
    error: Option<Value>,
    #[serde(rename = "retryAttempt", default, deserialize_with = "loose")]
    retry_attempt: Option<u64>,
    #[serde(rename = "maxRetries", default, deserialize_with = "loose")]
    max_retries: Option<u64>,
    #[serde(rename = "compactMetadata")]
    compact_metadata: Option<Value>,
    #[serde(rename = "hookErrors")]
    hook_errors: Option<Value>,
    #[serde(rename = "preventedContinuation", default, deserialize_with = "loose")]
    prevented_continuation: Option<bool>,
    #[serde(rename = "stopReason", default, deserialize_with = "loose")]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonlMessage {
    content: Option<Value>,
    #[serde(default, deserialize_with = "loose")]
    model: Option<String>,
}

/// A field of the wrong type (a numeric timestamp, say) reads as absent, so one odd field
/// can't lose the whole entry.
fn loose<'de, Input, Field>(deserializer: Input) -> Result<Option<Field>, Input::Error>
where
    Input: serde::Deserializer<'de>,
    Field: serde::de::DeserializeOwned,
{
    Ok(serde_json::from_value(Value::deserialize(deserializer)?).ok())
}

/// A transcript's lines. One invalid byte costs that character, not the rest of the file.
/// `read_failed` says whether reading stopped early.
struct TranscriptLines<Source> {
    source: Source,
    read_failed: bool,
}

fn transcript_lines<Source: BufRead>(source: Source) -> TranscriptLines<Source> {
    TranscriptLines { source, read_failed: false }
}

impl<Source: BufRead> Iterator for TranscriptLines<Source> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        let mut bytes = Vec::new();
        match self.source.read_until(b'\n', &mut bytes) {
            Ok(0) => None,
            Ok(_) => {
                if bytes.last() == Some(&b'\n') {
                    bytes.pop();
                    if bytes.last() == Some(&b'\r') {
                        bytes.pop();
                    }
                }
                Some(String::from_utf8(bytes).unwrap_or_else(|invalid| String::from_utf8_lossy(invalid.as_bytes()).into_owned()))
            }
            Err(_) => {
                self.read_failed = true;
                None
            }
        }
    }
}

/// Claude Code's record of each API call, which repeats the conversation's content blocks.
/// Always written with `type` first, so the line never needs parsing to be skipped.
fn is_api_record(line: &str) -> bool {
    line.starts_with("{\"type\":\"api-request")
}

/// The session a transcript belongs to is its file name.
fn own_session_id(path: &Path) -> String {
    path.file_stem().unwrap_or_default().to_string_lossy().to_string()
}

/// A fork stamps the lines it copied with `forkedFrom`. A desktop continuation copy has no
/// stamp, but its copied lines still carry the session they came from.
fn is_inherited(forked_from: Option<&Value>, session_id: Option<&str>, own_id: &str) -> bool {
    forked_from.is_some_and(|fork| !fork.is_null()) || session_id.is_some_and(|session| session != own_id)
}

fn value_is_inherited(value: &Value, own_id: &str) -> bool {
    is_inherited(value.get("forkedFrom"), value.get("sessionId").and_then(|session| session.as_str()), own_id)
}

fn entry_is_inherited(entry: &JsonlEntry, own_id: &str) -> bool {
    is_inherited(entry.forked_from.as_ref(), entry.session_id.as_deref(), own_id)
}

fn timestamp_millis(timestamp: Option<&str>) -> i64 {
    timestamp
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map_or(0, |parsed| parsed.timestamp_millis())
}

// ── Commands ────────────────────────────────────────────────────────────────

/// Returns the absolute path to a session's pasted image, if it exists.
/// Claude Code caches pasted images at ~/.claude/image-cache/<session_id>/<N>.<ext>.
/// Falls back to the local archive so images survive Claude Code's 30-day cleanup.
#[tauri::command(async)]
pub fn get_image_path(app: tauri::AppHandle, session_id: String, image_number: u32) -> Option<String> {
    // Live cache first.
    if let Some(home) = dirs::home_dir() {
        if let Some(found) = find_image_file(&home.join(".claude").join("image-cache").join(&session_id), image_number) {
            return Some(found.to_string_lossy().to_string());
        }
    }
    // Archive fallback (the live cache may have expired).
    if let Ok(root) = archive_root(&app) {
        if let Some(found) = find_image_file(&root.join("image-cache").join(&session_id), image_number) {
            return Some(found.to_string_lossy().to_string());
        }
    }
    None
}

fn find_image_file(folder: &Path, image_number: u32) -> Option<PathBuf> {
    ["png", "jpg", "jpeg", "gif", "webp"]
        .iter()
        .map(|extension| folder.join(format!("{}.{}", image_number, extension)))
        .find(|path| path.exists())
}

/// An image a Markdown message points at, as a data URL. Absolute paths only.
#[tauri::command(async)]
pub fn read_local_image(path: String) -> Option<String> {
    let local_path = Path::new(path.strip_prefix("file://").unwrap_or(&path));
    local_path.is_absolute().then(|| codex::local_image_url(local_path)).flatten()
}

/// Commands run concurrently on Tauri's async runtime, so two archive writes (e.g. a
/// bookmark's auto-save racing a refresh, or a location move) could interleave file
/// copies. Every command that writes to the archive holds this lock.
// ponytail: one global lock for all archive writes; per-session locks if saves ever feel slow
static ARCHIVE_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_archive_writes() -> std::sync::MutexGuard<'static, ()> {
    ARCHIVE_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
#[tauri::command(async)]
pub fn get_archive_info(app: tauri::AppHandle) -> Result<ArchiveInfo, String> {
    archive_info(&app)
}

/// Move the archive to a user-chosen folder and remember it. Existing archived
/// sessions are moved along, so nothing is orphaned. Returns updated info.
#[tauri::command(async)]
pub fn set_archive_location(
    app: tauri::AppHandle,
    new_parent_dir: String,
) -> Result<ArchiveInfo, String> {
    let _archive_guard = lock_archive_writes();
    let old_root = archive_root(&app)?;
    // Nest under a named folder so we never dump files into e.g. Documents root.
    let target = PathBuf::from(&new_parent_dir).join("ClaudeSessionsArchive");

    if target == old_root {
        return archive_info(&app);
    }
    if target.starts_with(&old_root) {
        return Err(
            "Choose a folder outside the current archive — moving the archive into itself would copy it forever."
                .to_string(),
        );
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
    allow_archive_images(&app);

    archive_info(&app)
}

/// Let the webview load pasted images from the archive. The default archive is covered
/// statically in tauri.conf.json; a user-chosen location can be anywhere, so it is
/// allowed at runtime (on startup and after a move).
pub(crate) fn allow_archive_images(app: &tauri::AppHandle) {
    if let Ok(root) = archive_root(app) {
        let _ = app.asset_protocol_scope().allow_directory(root.join("image-cache"), true);
    }
}

/// Reset the archive back to the app-default location (moves files back too).
#[tauri::command(async)]
pub fn reset_archive_location(app: tauri::AppHandle) -> Result<ArchiveInfo, String> {
    let _archive_guard = lock_archive_writes();
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
#[tauri::command(async)]
pub fn open_archive_location(app: tauri::AppHandle) -> Result<(), String> {
    let root = archive_root(&app)?;
    fs::create_dir_all(&root).ok();
    std::process::Command::new("open")
        .arg(&root)
        .spawn()
        .map_err(|error| format!("Cannot open folder: {}", error))?;
    Ok(())
}

/// Open a link from a transcript in the default browser. Web links only: the URL comes
/// from transcript text, and `open` would happily launch a file or another app.
#[tauri::command(async)]
pub fn open_external_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Only web links can be opened".to_string());
    }
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|open_error| format!("Cannot open link: {}", open_error))?;
    Ok(())
}

/// Show an exported file in Finder, selected. Existing files only, and the path goes to `open`
/// as its own argument, so it's never read as a flag or by a shell.
#[tauri::command(async)]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    let file = Path::new(&path);
    if !file.is_absolute() || !file.is_file() {
        return Err(format!("File not found: {}", path));
    }
    std::process::Command::new("open")
        .arg("-R")
        .arg(file)
        .spawn()
        .map_err(|open_error| format!("Cannot open Finder: {}", open_error))?;
    Ok(())
}

/// Recursively copy a directory (used for per-session folders + image caches).
fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            copy_file_whole(&entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Copy to a temporary name, then rename: an interrupted copy can't damage what's there.
fn copy_file_whole(source: &Path, destination: &Path) -> std::io::Result<()> {
    let mut temporary_name = destination.as_os_str().to_os_string();
    temporary_name.push(".evertusk-tmp");
    let temporary = PathBuf::from(temporary_name);
    let copied = fs::copy(source, &temporary).and_then(|_| fs::rename(&temporary, destination));
    if copied.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    copied
}

/// A live transcript only ever grows, so one that shrank or no longer starts with valid
/// JSON has been damaged, and the saved copy is the better one.
// ponytail: only the first line is checked for corruption; validate every line if damage ever shows up further in
fn may_replace_saved_copy(source: &Path, saved_copy: &Path) -> bool {
    let (Ok(source_info), Ok(saved_info)) = (fs::metadata(source), fs::metadata(saved_copy)) else {
        return true;
    };
    if source_info.len() < saved_info.len() {
        return false;
    }
    let first_line = fs::File::open(source).ok().and_then(|file| transcript_lines(BufReader::new(file)).next());
    first_line.is_some_and(|line| serde_json::from_str::<Value>(&line).is_ok())
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

    let dest_jsonl = session_dir.join(format!("{}.jsonl", session_id));
    // An expired session is opened from its archive, so the source can be the copy itself.
    if source_jsonl == dest_jsonl {
        return Ok(());
    }

    // Re-archive only when the live transcript is newer than the archived copy —
    // so opening an unchanged saved session is a no-op (no wasteful re-copy of the
    // transcript, subagents, or images), while a session you kept working in gets
    // fully refreshed.
    let source_newer = match (fs::metadata(&dest_jsonl), fs::metadata(source_jsonl)) {
        (Ok(dest_meta), Ok(source_meta)) => match (dest_meta.modified(), source_meta.modified()) {
            (Ok(dest_time), Ok(source_time)) => source_time > dest_time,
            _ => true,
        },
        _ => true,
    } && may_replace_saved_copy(source_jsonl, &dest_jsonl);
    // Copies made by older versions may lack folders that are archived today, like
    // tool-results. Those still need filling in while the live files exist.
    let source_session_dir = source_jsonl.parent().map(|parent_dir| parent_dir.join(session_id));
    let dest_session_dir = session_dir.join(session_id);
    let folder_incomplete = source_session_dir
        .as_deref()
        .is_some_and(|source_dir| !session_folder_is_complete(source_dir, &dest_session_dir));
    if !source_newer && !folder_incomplete {
        return Ok(());
    }

    // 1) Transcript. Without it there's nothing saved, so this one stops the save.
    if source_newer {
        copy_file_whole(source_jsonl, &dest_jsonl)
            .map_err(|copy_error| format!("Copy transcript failed: {}", copy_error))?;
    }

    // The rest is best-effort per step: keep copying what we can, then report what failed.
    let mut failures: Vec<String> = Vec::new();

    // 2) Everything Claude Code keeps per session: subagent logs, workflow runs and
    // persisted tool output. <project_dir>/<id>/… → <root>/<id>/<id>/…
    if let Some(source_dir) = source_session_dir.filter(|source_dir| source_dir.is_dir()) {
        if let Err(copy_error) = copy_dir_recursive(&source_dir, &dest_session_dir) {
            failures.push(format!("session files: {}", copy_error));
        }
    }

    if source_newer {
        // 3) Pasted images: <images>/… → <root>/image-cache/<id>
        if let Some(images) = live_images.filter(|images| images.is_dir()) {
            if let Err(copy_error) = copy_dir_recursive(images, &root.join("image-cache").join(session_id)) {
                failures.push(format!("images: {}", copy_error));
            }
        }

        // 4) Self-describing metadata so the archive stands on its own.
        let meta_text = serde_json::to_string_pretty(meta).unwrap_or_default();
        if let Err(write_error) = fs::write(session_dir.join("meta.json"), meta_text) {
            failures.push(format!("metadata: {}", write_error));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("Saved the transcript, but couldn't copy {}", failures.join("; ")))
    }
}

/// True when every top-level item in a live session folder is in its archived copy.
/// A missing live folder has nothing left to copy, so it counts as complete.
fn session_folder_is_complete(source_dir: &Path, dest_dir: &Path) -> bool {
    match fs::read_dir(source_dir) {
        Ok(entries) => entries.flatten().all(|entry| dest_dir.join(entry.file_name()).exists()),
        Err(_) => true,
    }
}

/// Copy a session's transcript + subagent logs + pasted images into the local
/// archive so it survives Claude Code's 30-day cleanup, keeping bookmarks openable.
#[tauri::command(async)]
pub fn archive_session(
    app: tauri::AppHandle,
    jsonl_path: String,
    session_id: String,
    project_path: String,
    project_name: String,
    title: Option<String>,
) -> Result<(), String> {
    let _archive_guard = lock_archive_writes();
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
#[tauri::command(async)]
pub fn get_archived_session_path(app: tauri::AppHandle, session_id: String) -> Option<String> {
    let root = archive_root(&app).ok()?;
    let dest = root.join(&session_id).join(format!("{}.jsonl", session_id));
    if dest.exists() {
        Some(dest.to_string_lossy().to_string())
    } else {
        None
    }
}

/// Every archived session, pointing at its archived transcript. The UI marks saved
/// sessions with it and keeps listing them after Claude Code's cleanup deletes the live
/// file — without this, a saved session was only reachable through a bookmark.
#[tauri::command(async)]
pub fn get_archived_sessions(app: tauri::AppHandle) -> Vec<SessionInfo> {
    let sessions = archive_root(&app).map(|root| archived_sessions_in(&root)).unwrap_or_default();
    CLAUDE_INDEX.save();
    sessions
}

/// Core of `get_archived_sessions` against an explicit root (unit-testable, like
/// `archive_session_to`).
fn archived_sessions_in(root: &Path) -> Vec<SessionInfo> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut sessions = Vec::new();
    for entry in entries.flatten() {
        let session_id = entry.file_name().to_string_lossy().to_string();
        if session_id == "image-cache" || !entry.path().is_dir() {
            continue;
        }
        // Only count it as saved if the transcript is actually present.
        let jsonl_path = entry.path().join(format!("{}.jsonl", session_id));
        if !jsonl_path.exists() {
            continue;
        }
        let meta: Value = fs::read_to_string(entry.path().join("meta.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        let meta_text = |key: &str| meta.get(key).and_then(|value| value.as_str()).map(String::from);
        // A copy with no usable meta.json still knows its folder from the transcript.
        let project_path = meta_text("project_path")
            .filter(|path| !path.is_empty())
            .or_else(|| first_recorded_cwd(&jsonl_path))
            .map_or_else(|| "Unknown folder".to_string(), |path| normalize_project_path(&path));
        let project_name = meta_text("project_name")
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| project_name_of(&project_path));
        let metadata = extract_quick_metadata(&jsonl_path);

        sessions.push(SessionInfo {
            tool: "claude".to_string(),
            archived_in_tool: false,
            summary: None,
            custom_title: metadata.custom_title,
            ai_title: metadata.ai_title.or_else(|| meta_text("title")),
            first_prompt: metadata.first_prompt,
            project_path,
            project_name,
            created: metadata.first_timestamp,
            // The archive copy's mtime is the copy time, so prefer the transcript's own last timestamp.
            modified: metadata.last_timestamp.or_else(|| get_file_mtime_iso(&jsonl_path)),
            // A copy being written says nothing about the session running.
            last_written: None,
            conversation_count: metadata.conversation_count,
            git_branch: None,
            forked_from_session_id: detect_fork_parent(&jsonl_path),
            jsonl_path: jsonl_path.to_string_lossy().to_string(),
            session_id,
        });
    }
    sessions
}

/// Remove a session's archived copy (transcript + subagents + images + meta). Deletes
/// only our archive — the live session, if any, is untouched.
#[tauri::command(async)]
pub fn unarchive_session(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    let _archive_guard = lock_archive_writes();
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
    /// One of "edit", "write", "delete", "multiedit", "notebookedit", "bash".
    pub action: String,
    /// Previous content. For a Write, the file it replaced; None when it created the file.
    pub old_string: Option<String>,
    /// New content. None for unusual cases (shouldn't happen in practice).
    pub new_string: Option<String>,
    pub tool_use_id: Option<String>,
    /// True for `replace_all` Edit calls — informational tag in the UI.
    pub replace_all: bool,
    /// The tool rejected this edit, so the file never changed. Listed as "Not applied".
    pub failed: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct FileChange {
    pub path: String,
    pub display_path: String,
    pub edits: Vec<FileEditEntry>,
    pub edit_count: u32,
    pub read_count: u32,
}

#[derive(Default)]
struct FileBucket {
    edits: Vec<FileEditEntry>,
    read_count: u32,
}

/// File operations gathered across a session's transcripts.
#[derive(Default)]
struct FileChangeLog {
    buckets: std::collections::HashMap<String, FileBucket>,
    /// Tool calls that came back as errors.
    rejected_calls: std::collections::HashSet<String>,
    /// What a Write replaced, recorded with its result.
    replaced_files: std::collections::HashMap<String, String>,
}

impl FileChangeLog {
    /// Add one transcript's file operations, one line at a time: logs can be hundreds of
    /// MB, and a session can have a hundred subagents.
    fn collect(&mut self, transcript: &Path) {
        let file = match fs::File::open(transcript) {
            Ok(file) => file,
            // A subagent's log may be gone while the session's isn't; the session must still load.
            Err(_) => return,
        };
        for line in transcript_lines(BufReader::new(file)) {
            if is_api_record(&line) {
                continue;
            }
            let has_call = line.contains("\"tool_use\"") && line.contains("\"type\":\"assistant\"");
            let has_outcome = line.contains("\"bashEditDiff\"") || line.contains("\"originalFile\"") || line.contains("\"is_error\":true");
            if !has_call && !has_outcome {
                continue;
            }
            let value: Value = match serde_json::from_str(&line) {
                Ok(parsed) => parsed,
                Err(_) => continue,
            };
            match value.get("type").and_then(|kind| kind.as_str()) {
                Some("assistant") => self.collect_calls(&value),
                Some("user") => self.collect_outcomes(&value),
                _ => {}
            }
        }
    }

    fn collect_calls(&mut self, entry: &Value) {
        let timestamp = entry.get("timestamp").and_then(|val| val.as_str()).map(String::from);
        let blocks = entry.get("message").and_then(|message| message.get("content")).and_then(|content| content.as_array());
        for block in blocks.into_iter().flatten() {
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

            let bucket = self.buckets.entry(file_path).or_default();
            if is_read {
                bucket.read_count += 1;
            } else {
                bucket.edits.extend(entries);
            }
        }
    }

    fn collect_outcomes(&mut self, entry: &Value) {
        let timestamp = entry.get("timestamp").and_then(|val| val.as_str()).map(String::from);
        // Files a Bash command changed, with the real diff Claude Code recorded.
        for (file_path, edit) in extract_bash_edit_entries(entry, &timestamp) {
            self.buckets.entry(file_path).or_default().edits.push(edit);
        }
        let original = entry.get("toolUseResult").and_then(|result| result.get("originalFile")).and_then(|text| text.as_str());
        if let Some((original, tool_use_id)) = original.zip(find_tool_result_id(entry)) {
            self.replaced_files.insert(tool_use_id, original.to_string());
        }
        let blocks = entry.get("message").and_then(|message| message.get("content")).and_then(|content| content.as_array());
        for block in blocks.into_iter().flatten() {
            let is_rejection = block.get("type").and_then(|kind| kind.as_str()) == Some("tool_result")
                && block.get("is_error").and_then(|flag| flag.as_bool()) == Some(true);
            if let Some(tool_use_id) = block.get("tool_use_id").and_then(|id| id.as_str()).filter(|_| is_rejection) {
                self.rejected_calls.insert(tool_use_id.to_string());
            }
        }
    }

    /// Join each edit with its outcome and put it in time order: subagents' logs are read
    /// after the session's, though they ran during it.
    fn finish(mut self) -> std::collections::HashMap<String, FileBucket> {
        for bucket in self.buckets.values_mut() {
            for edit in &mut bucket.edits {
                let Some(tool_use_id) = edit.tool_use_id.as_ref() else { continue };
                // A failed Bash command can still have changed the file it recorded.
                edit.failed = edit.action != "bash" && self.rejected_calls.contains(tool_use_id);
                if edit.action == "write" {
                    edit.old_string = self.replaced_files.get(tool_use_id).cloned();
                }
            }
            bucket.edits.sort_by(|first, second| first.timestamp.cmp(&second.timestamp));
        }
        self.buckets
    }
}

/// Per-session breakdown of file changes — captures the actual old/new content
/// from each Edit/Write/MultiEdit/NotebookEdit call so the UI can render a diff.
/// Bash edits come from the diffs Claude Code records. Subagent edits are included:
/// a file changed by a subagent changed just the same.
#[tauri::command(async)]
pub fn get_session_file_changes(jsonl_path: String) -> Result<Vec<FileChange>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }
    if codex::is_codex_path(&path) {
        return codex::file_changes(&path);
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
        .map(|(parent, name)| {
            // An archived session's folder name is its id; its project is in meta.json.
            fs::read_to_string(parent.join("meta.json"))
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|meta| meta.get("project_path")?.as_str().map(String::from))
                .unwrap_or_else(|| resolve_project_path(&parent, &name))
        })
        .unwrap_or_default();
    let project_name = project_path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string();

    let mut change_log = FileChangeLog::default();
    change_log.collect(&path);
    for subagent in list_subagents(jsonl_path.clone())? {
        change_log.collect(Path::new(&subagent.jsonl_path));
    }

    let mut changes: Vec<FileChange> = change_log
        .finish()
        .into_iter()
        .map(|(file_path, bucket)| FileChange {
            display_path: build_display_path(&project_path, &project_name, &file_path),
            edit_count: bucket.edits.iter().filter(|edit| !edit.failed).count() as u32,
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

/// File edits made through Bash: `toolUseResult.bashEditDiff.files[]`, whose hunks
/// hold unified-diff lines. Removed lines become the old side, added lines the new.
fn extract_bash_edit_entries(
    entry: &Value,
    timestamp: &Option<String>,
) -> Vec<(String, FileEditEntry)> {
    let files = entry
        .get("toolUseResult")
        .and_then(|result| result.get("bashEditDiff"))
        .and_then(|diff| diff.get("files"))
        .and_then(|files| files.as_array());
    let files = match files {
        Some(files) => files,
        None => return Vec::new(),
    };
    let tool_use_id = find_tool_result_id(entry);
    let mut entries = Vec::new();
    for file in files {
        let file_path = match file.get("filePath").and_then(|path| path.as_str()) {
            Some(path) => path.to_string(),
            None => continue,
        };
        let mut removed = Vec::new();
        let mut added = Vec::new();
        for hunk in file.get("hunks").and_then(|hunks| hunks.as_array()).into_iter().flatten() {
            for diff_line in hunk.get("lines").and_then(|lines| lines.as_array()).into_iter().flatten() {
                let diff_line = diff_line.as_str().unwrap_or("");
                if let Some(content) = diff_line.strip_prefix('-') {
                    removed.push(content);
                } else if let Some(content) = diff_line.strip_prefix('+') {
                    added.push(content);
                }
            }
        }
        entries.push((
            file_path,
            FileEditEntry {
                timestamp: timestamp.clone(),
                action: "bash".to_string(),
                old_string: (!removed.is_empty()).then(|| removed.join("\n")),
                new_string: (!added.is_empty()).then(|| added.join("\n")),
                tool_use_id: tool_use_id.clone(),
                replace_all: false,
                failed: false,
            },
        ));
    }
    entries
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
                    failed: false,
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
                    failed: false,
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
                    failed: false,
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
                    failed: false,
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

/// Session folders and files that exist but can't be read, which would otherwise look like
/// having no history at all. A missing folder is fine: that tool just isn't installed.
#[tauri::command(async)]
pub fn get_read_problems() -> Vec<String> {
    let Some(home) = dirs::home_dir() else { return vec!["Can't find your home folder.".to_string()] };
    let mut problems = claude_read_problems(&home.join(".claude").join("projects"));
    problems.extend(codex::read_problems());
    problems
}

fn plain_read_error(read_error: &std::io::Error) -> &'static str {
    match read_error.kind() {
        std::io::ErrorKind::PermissionDenied => "this app doesn't have permission",
        _ => "it couldn't be opened",
    }
}

/// Everything under Claude's projects folder that should open and doesn't. Project folders
/// also hold thousands of tool output files, so only their top level is looked at.
fn claude_read_problems(projects: &Path) -> Vec<String> {
    let folder_problem = |folder: &Path, read_error: &std::io::Error| {
        format!("Can't read Claude Code sessions in {}: {}.", folder.display(), plain_read_error(read_error))
    };
    match projects.try_exists() {
        Ok(true) => {}
        Ok(false) => return Vec::new(),
        Err(access_error) => return vec![folder_problem(projects, &access_error)],
    }
    let project_entries = match fs::read_dir(projects) {
        Ok(entries) => entries,
        Err(read_error) => return vec![folder_problem(projects, &read_error)],
    };
    let mut problems = Vec::new();
    let mut locked_files: Vec<PathBuf> = Vec::new();
    for project_dir in project_entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()) {
        match fs::read_dir(&project_dir) {
            Ok(files) => locked_files.extend(
                files
                    .flatten()
                    .map(|file| file.path())
                    .filter(|path| is_transcript_file(path) && fs::File::open(path).is_err()),
            ),
            Err(read_error) => problems.push(folder_problem(&project_dir, &read_error)),
        }
    }
    if let Some(example) = locked_files.first() {
        let count = if locked_files.len() == 1 { "1 Claude Code session".to_string() } else { format!("{} Claude Code sessions", locked_files.len()) };
        problems.push(format!("{} couldn't be opened, for example {}.", count, example.display()));
    }
    problems
}

/// Which of these folders still exist, for projects known only from saved sessions.
#[tauri::command(async)]
pub fn existing_folders(paths: Vec<String>) -> Vec<String> {
    paths.into_iter().filter(|path| Path::new(path).exists()).collect()
}

/// One row per folder, whichever tools worked in it.
#[tauri::command(async)]
pub fn get_projects() -> Result<Vec<ProjectInfo>, String> {
    let home = dirs::home_dir().map(|home| home.to_string_lossy().to_string()).unwrap_or_default();
    let mut projects: Vec<ProjectInfo> = Vec::new();
    for session in scan_projects()? {
        let active_ms = timestamp_millis(session.modified.as_deref()).max(0) as u64;
        match projects.iter_mut().find(|project| project.project_path == session.project_path) {
            Some(project) => {
                project.session_count += 1;
                if !project.tools.contains(&session.tool) {
                    project.tools.push(session.tool);
                }
                if active_ms > project.last_active_ms {
                    project.last_active_ms = active_ms;
                    project.last_active = Some(format_relative_time(active_ms));
                }
            }
            None => projects.push(ProjectInfo {
                folder_exists: Path::new(&session.project_path).exists(),
                tools: vec![session.tool],
                short_path: home_relative_path(&session.project_path, &home),
                project_name: session.project_name,
                session_count: 1,
                last_active: (active_ms > 0).then(|| format_relative_time(active_ms)),
                last_active_ms: active_ms,
                project_path: session.project_path,
            }),
        }
    }
    projects.sort_by_key(|project| std::cmp::Reverse(project.last_active_ms));
    Ok(projects)
}

/// `/Users/me/app` → `~/app`, only when home really is a parent: `/Users/me-work` isn't under it.
fn home_relative_path(path: &str, home: &str) -> String {
    let inside_home = path.strip_prefix(home).filter(|rest| !home.is_empty() && (rest.is_empty() || rest.starts_with('/')));
    inside_home.map_or_else(|| path.to_string(), |rest| format!("~{}", rest))
}

/// The same folder written with a trailing slash is still the same project.
fn normalize_project_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() && !path.is_empty() { "/".to_string() } else { trimmed.to_string() }
}

fn is_transcript_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") && path.is_file()
}

fn resolve_project_path(project_dir: &Path, dir_name: &str) -> String {
    let index_path = project_dir.join("sessions-index.json");
    if index_path.exists() {
        if let Ok(content) = fs::read_to_string(&index_path) {
            if let Ok(data) = serde_json::from_str::<SessionIndexFile>(&content) {
                if let Some(path) = data.original_path {
                    return normalize_project_path(&path);
                }
            }
        }
    }
    normalize_project_path(&recorded_project_path(project_dir, dir_name).unwrap_or_else(|| decode_project_path(dir_name)))
}

/// How Claude Code names a project's folder: every non-alphanumeric UTF-16 unit becomes a
/// dash, so an emoji takes two.
fn encode_project_path(path: &str) -> String {
    path.encode_utf16()
        .map(|unit| match char::from_u32(unit.into()) {
            Some(character) if character.is_ascii_alphanumeric() => character,
            _ => '-',
        })
        .collect()
}

/// The folder name can't be decoded reliably (a dash may be a slash, a dash, a dot or
/// an underscore), but transcripts record `cwd`. A session can `cd` elsewhere, so only
/// accept a cwd that encodes back to this exact folder name.
fn recorded_project_path(project_dir: &Path, dir_name: &str) -> Option<String> {
    const FILES_TO_TRY: usize = 5;
    const LINES_TO_TRY: usize = 50;
    let files = fs::read_dir(project_dir).ok()?;
    let transcripts = files
        .flatten()
        .map(|file_entry| file_entry.path())
        .filter(|path| is_transcript_file(path))
        .take(FILES_TO_TRY);
    for transcript in transcripts {
        let file = match fs::File::open(&transcript) {
            Ok(file) => file,
            Err(_) => continue,
        };
        for line in transcript_lines(BufReader::new(file)).take(LINES_TO_TRY) {
            if !line.contains("\"cwd\"") {
                continue;
            }
            let value: Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if let Some(cwd) = value.get("cwd").and_then(|cwd| cwd.as_str()) {
                if encode_project_path(cwd) == dir_name {
                    return Some(cwd.to_string());
                }
            }
        }
    }
    None
}

/// The first working folder a transcript records.
fn first_recorded_cwd(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    transcript_lines(BufReader::new(file)).take(50).filter(|line| line.contains("\"cwd\"")).find_map(|line| {
        let value: Value = serde_json::from_str(&line).ok()?;
        value.get("cwd")?.as_str().map(String::from)
    })
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

/// Peek a session's first lines for the session it came from. Claude Code stamps
/// `forkedFrom.sessionId` on every entry inherited from the parent when a session is forked
/// (`/branch`); a desktop continuation copy keeps the original `sessionId` instead. Either is
/// near the top, so reading a few lines stays cheap. Bookkeeping entries can come first.
fn detect_fork_parent(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let own_id = own_session_id(path);
    for line in transcript_lines(BufReader::new(file)).take(50) {
        if !line.contains("\"forkedFrom\"") && !line.contains("\"sessionId\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(parent) = value.get("forkedFrom").and_then(|fork| fork.get("sessionId")).and_then(|id| id.as_str()) {
            return Some(parent.to_string());
        }
        if let Some(original) = value.get("sessionId").and_then(|id| id.as_str()).filter(|id| *id != own_id) {
            return Some(original.to_string());
        }
    }
    None
}

/// The folder's own name; the root has none, so it goes by its path.
fn project_name_of(project_path: &str) -> String {
    match project_path.trim_end_matches('/').rsplit('/').next() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => project_path.to_string(),
    }
}

/// Build a session from its transcript alone (no sessions-index.json entry for it).
fn session_from_transcript(path: &Path, project_path: &str) -> SessionInfo {
    let metadata = extract_quick_metadata(path);
    SessionInfo {
        tool: "claude".to_string(),
        archived_in_tool: false,
        session_id: path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
        summary: None,
        custom_title: metadata.custom_title,
        ai_title: metadata.ai_title,
        first_prompt: metadata.first_prompt,
        project_path: project_path.to_string(),
        project_name: project_name_of(project_path),
        created: metadata.first_timestamp,
        modified: metadata.last_timestamp.or_else(|| get_file_mtime_iso(path)),
        last_written: get_file_mtime_iso(path),
        conversation_count: metadata.conversation_count,
        git_branch: None,
        forked_from_session_id: detect_fork_parent(path),
        jsonl_path: path.to_string_lossy().to_string(),
    }
}

/// Entries from sessions-index.json, when the project has one. Older Claude Code
/// versions wrote it; it's a fast path for titles, never the source of truth.
fn indexed_sessions(project_dir: &Path, project_path: &str) -> Vec<SessionInfo> {
    let index: Option<SessionIndexFile> = fs::read_to_string(project_dir.join("sessions-index.json"))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok());
    let mut sessions = Vec::new();
    for entry in index.map(|index| index.entries).unwrap_or_default() {
        // Always build the path from project dir + session ID: fullPath in the index
        // is often stale. Skip sessions whose transcripts no longer exist.
        let jsonl_path = project_dir.join(format!("{}.jsonl", entry.session_id));
        if !jsonl_path.exists() {
            continue;
        }
        // The index's titles are useful, but its counts are unreliable and often missing.
        let metadata = extract_quick_metadata(&jsonl_path);
        sessions.push(SessionInfo {
            tool: "claude".to_string(),
            archived_in_tool: false,
            session_id: entry.session_id,
            summary: entry.summary,
            custom_title: entry.custom_title,
            ai_title: entry.ai_title,
            first_prompt: entry.first_prompt,
            project_path: project_path.to_string(),
            project_name: project_name_of(project_path),
            created: entry.created,
            // The transcript over the index, which can be stale.
            modified: metadata.last_timestamp.or(entry.modified).or_else(|| get_file_mtime_iso(&jsonl_path)),
            last_written: get_file_mtime_iso(&jsonl_path),
            conversation_count: metadata.conversation_count,
            git_branch: entry.git_branch,
            forked_from_session_id: detect_fork_parent(&jsonl_path),
            jsonl_path: jsonl_path.to_string_lossy().to_string(),
        });
    }
    sessions
}

#[tauri::command(async)]
pub fn scan_projects() -> Result<Vec<SessionInfo>, String> {
    // Projects and sessions load together. The second caller waits and gets the cache,
    // instead of reading every transcript a second time on a cold start.
    static SCAN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _scan_guard = SCAN_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut sessions: Vec<SessionInfo> = codex::sessions();
    for session in &mut sessions {
        session.project_path = normalize_project_path(&session.project_path);
        session.project_name = project_name_of(&session.project_path);
    }

    let project_dirs: Vec<PathBuf> = get_claude_projects_dir()
        .ok()
        .and_then(|dir| fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    // Only the first load after an update reads much. This spreads that across every core;
    // the loop below then finds each answer waiting.
    let transcripts: Vec<PathBuf> = project_dirs
        .iter()
        .flat_map(|dir| fs::read_dir(dir).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .filter(|path| is_transcript_file(path))
        .collect();
    codex::in_parallel(&transcripts, |path| {
        extract_quick_metadata(path);
        None::<()>
    });

    for project_dir in project_dirs {
        if !project_dir.is_dir() {
            continue;
        }
        let dir_name = project_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let resolved_path = resolve_project_path(&project_dir, &dir_name);

        let indexed = indexed_sessions(&project_dir, &resolved_path);
        let indexed_ids: std::collections::HashSet<String> =
            indexed.iter().map(|session| session.session_id.clone()).collect();
        sessions.extend(indexed);

        // Every transcript the index doesn't know about (usually all of them).
        for file_entry in fs::read_dir(&project_dir).into_iter().flatten().flatten() {
            let file_path = file_entry.path();
            if !is_transcript_file(&file_path) {
                continue;
            }
            let session_id = file_path.file_stem().unwrap_or_default().to_string_lossy();
            if !indexed_ids.contains(session_id.as_ref()) {
                sessions.push(session_from_transcript(&file_path, &resolved_path));
            }
        }
    }

    CLAUDE_INDEX.save();
    codex::save_index();

    // Sort by modified date, most recent first
    sessions.sort_by_key(|session| std::cmp::Reverse(timestamp_millis(session.modified.as_deref())));
    Ok(without_duplicates(sessions))
}

/// A copied project folder or a symlink can list one session twice. The UI keys rows on these
/// values, so each stays unique; the sessions arrive newest first and the newest wins.
fn without_duplicates(sessions: Vec<SessionInfo>) -> Vec<SessionInfo> {
    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_paths = std::collections::HashSet::new();
    sessions
        .into_iter()
        .filter(|session| {
            seen_ids.insert((session.tool.clone(), session.session_id.clone())) && seen_paths.insert(session.jsonl_path.clone())
        })
        .collect()
}

#[derive(Debug, Serialize, Clone)]
pub struct GlobalSearchResult {
    pub session_id: String,
    pub tool: String,
    pub project_name: String,
    pub project_path: String,
    pub session_name: String,
    pub matched_text: String,
    pub match_source: String, // "session_name", "message"
    pub timestamp: Option<String>,
    pub jsonl_path: String,
}

/// A window of `text` around the first query word, so the preview shows the match
/// rather than a message's unrelated opening. Works on chars: `text` may be non-ASCII.
fn match_snippet(text: &str, query_words: &[&str]) -> String {
    const LEAD_IN: usize = 60;
    const WINDOW: usize = 200;
    let lowered = text.to_lowercase();
    let match_byte = query_words.first().and_then(|word| lowered.find(word)).unwrap_or(0);
    // Lowercasing can change a character's length, so walk the original to count up to it.
    let mut lowered_bytes = 0;
    let match_char = text
        .chars()
        .take_while(|character| {
            let before_match = lowered_bytes < match_byte;
            lowered_bytes += character.to_lowercase().map(char::len_utf8).sum::<usize>();
            before_match
        })
        .count();
    let start = match_char.saturating_sub(LEAD_IN);
    let snippet: String = text.chars().skip(start).take(WINDOW).collect();
    if start > 0 { format!("…{}", snippet.trim_start()) } else { snippet }
}

/// Search one transcript by session name, then by message text. Returns its first hit.
fn search_transcript(
    path: &Path,
    project_path: &str,
    query_words: &[&str],
) -> Option<GlobalSearchResult> {
    let metadata = extract_quick_metadata(path);
    let session_id = path.file_stem()?.to_string_lossy().to_string();
    let session_name = metadata
        .custom_title
        .clone()
        .or(metadata.ai_title.clone())
        .or(metadata.first_prompt.clone())
        .unwrap_or_else(|| session_id.clone());
    let contains_all = |text: &str| {
        let lowered = text.to_lowercase();
        query_words.iter().all(|word| lowered.contains(word))
    };
    let make_result = |matched_text: String, match_source: &str, timestamp: Option<String>| GlobalSearchResult {
        tool: "claude".to_string(),
        session_id: session_id.clone(),
        project_name: project_name_of(project_path),
        project_path: project_path.to_string(),
        session_name: session_name.chars().take(120).collect(),
        matched_text,
        match_source: match_source.to_string(),
        timestamp,
        jsonl_path: path.to_string_lossy().to_string(),
    };

    if contains_all(&format!("{} {}", session_name, project_name_of(project_path))) {
        // For name matches, use first_prompt as the preview context.
        let preview = metadata
            .first_prompt
            .clone()
            .filter(|prompt| prompt != &session_name)
            .unwrap_or_default();
        return Some(make_result(
            preview.chars().take(200).collect(),
            "session_name",
            metadata.first_timestamp.clone(),
        ));
    }

    let reader = BufReader::new(fs::File::open(path).ok()?);
    // The raw line escapes `"` and `\`, so words are compared the way the line writes them.
    let escaped_words: Vec<String> = query_words
        .iter()
        .map(|word| serde_json::to_string(word).unwrap_or_default().trim_matches('"').to_string())
        .collect();
    for line in transcript_lines(reader) {
        // Fast pre-checks before JSON parsing
        if !line.contains("\"type\":\"user\"") && !line.contains("\"type\":\"assistant\"") {
            continue;
        }
        let lowered_line = line.to_lowercase();
        if !escaped_words.iter().all(|word| lowered_line.contains(word.as_str())) {
            continue;
        }
        let entry: JsonlEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.is_sidechain.unwrap_or(false) {
            continue;
        }
        let text = match entry.entry_type.as_deref() {
            Some("user") if is_user_prompt(&entry) => strip_system_tags(&extract_user_text(&entry.message)),
            Some("assistant") => prose_text(&entry),
            _ => continue,
        };
        if contains_all(&text) {
            return Some(make_result(match_snippet(&text, query_words), "message", entry.timestamp));
        }
    }
    None
}

/// Only the words Claude wrote in an assistant entry — no tool input or thinking.
fn prose_text(entry: &JsonlEntry) -> String {
    let content = entry.message.as_ref().and_then(|message| message.content.as_ref());
    let blocks = content.and_then(|content| content.as_array()).map(Vec::as_slice).unwrap_or_default();
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(|kind| kind.as_str()) == Some("text"))
        .filter_map(|block| block.get("text").and_then(|text| text.as_str()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Searches live transcripts plus saved sessions whose live file has expired, so a
/// saved session stays findable after Claude Code's cleanup.
#[tauri::command(async)]
pub fn global_search(app: tauri::AppHandle, query: String, tool: Option<String>) -> Result<Vec<GlobalSearchResult>, String> {
    let archived = archive_root(&app).map(|root| archived_sessions_in(&root)).unwrap_or_default();
    Ok(search_every_tool(&query, tool.as_deref(), &archived))
}

/// Search across tools: newest first, however the tools' results interleave. `tool` narrows
/// it before the result limit applies, so a filter can't hide matches. A new search stops any
/// older one still running: each uses every core, and its result is already unwanted.
pub fn search_every_tool(query: &str, tool: Option<&str>, archived: &[SessionInfo]) -> Vec<GlobalSearchResult> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LATEST_SEARCH: AtomicU64 = AtomicU64::new(0);
    let this_search = LATEST_SEARCH.fetch_add(1, Ordering::SeqCst) + 1;
    let is_stale = move || LATEST_SEARCH.load(Ordering::SeqCst) != this_search;
    let mut results = if tool == Some("codex") {
        Vec::new()
    } else {
        search_transcripts_unless(query, get_claude_projects_dir().ok().as_deref(), archived, &is_stale)
    };
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    if !query_words.is_empty() && tool != Some("claude") {
        results.extend(codex::search(&query_words, &is_stale));
        results.extend(codex_project_matches(&query_words, &results));
    }
    results.sort_by_key(|result| std::cmp::Reverse(timestamp_millis(result.timestamp.as_deref())));
    results.truncate(50);
    results
}

/// Codex sessions matched by their folder's name, as names match in the session list.
fn codex_project_matches(query_words: &[&str], found: &[GlobalSearchResult]) -> Vec<GlobalSearchResult> {
    codex::sessions()
        .into_iter()
        .filter(|session| {
            let names = format!("{} {}", project_name_of(&session.project_path), session.custom_title.as_deref().unwrap_or("")).to_lowercase();
            query_words.iter().all(|word| names.contains(word))
                && !found.iter().any(|result| result.tool == "codex" && result.session_id == session.session_id)
        })
        .map(|session| GlobalSearchResult {
            session_name: session
                .custom_title
                .or(session.ai_title)
                .or(session.first_prompt)
                .unwrap_or_else(|| session.session_id.clone())
                .chars()
                .take(120)
                .collect(),
            tool: session.tool,
            session_id: session.session_id,
            project_name: project_name_of(&session.project_path),
            project_path: session.project_path,
            matched_text: String::new(),
            match_source: "session_name".to_string(),
            timestamp: session.modified,
            jsonl_path: session.jsonl_path,
        })
        .collect()
}

#[cfg(test)]
fn search_all_transcripts(query: &str, projects_dir: Option<&Path>, archived: &[SessionInfo]) -> Vec<GlobalSearchResult> {
    search_transcripts_unless(query, projects_dir, archived, &|| false)
}

fn search_transcripts_unless(
    query: &str,
    projects_dir: Option<&Path>,
    archived: &[SessionInfo],
    is_stale: &(dyn Fn() -> bool + Sync),
) -> Vec<GlobalSearchResult> {
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    if query_words.is_empty() {
        return Vec::new();
    }

    let mut results: Vec<GlobalSearchResult> = Vec::new();
    let mut live_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // The live directory is optional: saved sessions remain searchable without it.
    let project_dirs = projects_dir.and_then(|dir| fs::read_dir(dir).ok());
    for project_entry in project_dirs.into_iter().flatten().flatten() {
        let project_dir = project_entry.path();
        if !project_dir.is_dir() {
            continue;
        }
        let dir_name = project_dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        let project_path = resolve_project_path(&project_dir, &dir_name);
        for file_entry in fs::read_dir(&project_dir).into_iter().flatten().flatten() {
            let file_path = file_entry.path();
            if !is_transcript_file(&file_path) {
                continue;
            }
            if is_stale() {
                return Vec::new();
            }
            live_ids.insert(file_path.file_stem().unwrap_or_default().to_string_lossy().to_string());
            results.extend(search_transcript(&file_path, &project_path, &query_words));
        }
    }

    for session in archived.iter().filter(|session| !live_ids.contains(&session.session_id)) {
        results.extend(search_transcript(
            Path::new(&session.jsonl_path),
            &session.project_path,
            &query_words,
        ));
    }

    // Sort by timestamp, most recent first
    results.sort_by_key(|result| std::cmp::Reverse(timestamp_millis(result.timestamp.as_deref())));
    results.truncate(50);
    results
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
    /// Hook runs grouped by event (PostToolUse, Stop, ...), busiest first.
    pub hooks: Vec<HookStat>,
    /// Time Claude spent working, summed from turn durations (excludes idle time).
    pub active_ms: u64,
    /// Some turns record no duration (only the CLI writes them), so `active_ms` is a minimum.
    pub active_is_partial: bool,
    /// From Claude Code's own `cost-state` record, when the session wrote one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// The cost record covers much less work than the transcript shows, so it's a minimum.
    pub cost_is_partial: bool,
    /// Tokens this session's own subagents used. The fields above are the main thread only.
    pub agent_output_tokens: u64,
    pub agent_input_tokens: u64,
    /// Prompts a fork copied from its parent; `user_prompt_count` is the fork's own.
    pub inherited_prompts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_added: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_removed: Option<u64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ToolCount {
    pub name: String,
    pub count: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct HookStat {
    pub event: String,
    pub runs: u64,
    pub failures: u64,
}

/// Tally one line of hook activity into (runs, failures) per hook event. Stop hooks are
/// recorded twice, in a summary and in their own attachments, so summaries add up apart.
fn tally_hook_line(
    value: &Value,
    hook_counts: &mut std::collections::HashMap<String, (u64, u64)>,
    stop_summaries: &mut (u64, u64),
) {
    if let Some(attachment) = value.get("attachment") {
        let attachment_type = attachment.get("type").and_then(|kind| kind.as_str()).unwrap_or("");
        if !attachment_type.contains("hook") {
            return;
        }
        let event = attachment
            .get("hookEvent")
            .and_then(|event| event.as_str())
            .unwrap_or("unknown")
            .to_string();
        let exit_code = attachment.get("exitCode").and_then(|code| code.as_i64()).unwrap_or(0);
        let counts = hook_counts.entry(event).or_insert((0, 0));
        counts.0 += 1;
        if exit_code != 0 || is_failed_hook_type(attachment_type) {
            counts.1 += 1;
        }
    } else if value.get("subtype").and_then(|subtype| subtype.as_str()) == Some("stop_hook_summary") {
        let runs = value.get("hookCount").and_then(|count| count.as_u64()).unwrap_or(0);
        let failures = value.get("hookErrors").and_then(|errors| errors.as_array()).map_or(0, |errors| errors.len() as u64);
        stop_summaries.0 += runs;
        stop_summaries.1 += failures;
    }
}

fn is_failed_hook_type(attachment_type: &str) -> bool {
    attachment_type.contains("error") || attachment_type.contains("block") || attachment_type.contains("cancel")
}

#[tauri::command(async)]
pub fn get_session_stats(jsonl_path: String) -> Result<SessionStats, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }
    if codex::is_codex_path(&path) {
        return codex::stats(&path);
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

    let mut hook_counts: std::collections::HashMap<String, (u64, u64)> =
        std::collections::HashMap::new();
    let mut stop_summaries = (0, 0);
    let own_id = own_session_id(&path);
    let mut prompts = PromptCounter::for_session(&own_id);
    let mut saw_turn_duration = false;
    let mut saw_untimed_entrypoint = false;
    // Claude Code's counters reset whenever it restarts: within a run they grow (allowing
    // for rewinds), so a session's total is each run's highest value, added up.
    let mut cost_by_run: std::collections::HashMap<i64, (f64, f64, f64, f64)> =
        std::collections::HashMap::new();
    let mut cost_has_usage = false;

    for line in transcript_lines(reader) {
        if line.trim().is_empty() || is_api_record(&line) {
            continue;
        }

        prompts.add_queued_line(&line);

        let is_hook_line = (line.contains("\"type\":\"attachment\"") && line.contains("\"hook"))
            || (line.contains("\"type\":\"system\"") && line.contains("\"stop_hook_summary\""));
        if is_hook_line {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if !value_is_inherited(&value, &own_id) {
                    tally_hook_line(&value, &mut hook_counts, &mut stop_summaries);
                }
            }
            continue;
        }
        if line.contains("\"turn_duration\"") && line.contains("\"type\":\"system\"") {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if !value_is_inherited(&value, &own_id) {
                    saw_turn_duration = true;
                    stats.active_ms += value.get("durationMs").and_then(|ms| ms.as_u64()).unwrap_or(0);
                }
            }
            continue;
        }
        if line.contains("\"type\":\"cost-state\"") {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                let run_start = value.get("startTime").and_then(|start| start.as_i64()).unwrap_or(0);
                let run_totals = cost_by_run.entry(run_start).or_default();
                let number = |key: &str| value.get(key).and_then(|number| number.as_f64()).unwrap_or(0.0);
                run_totals.0 = run_totals.0.max(number("totalCostUSD"));
                run_totals.1 = run_totals.1.max(number("totalLinesAdded"));
                run_totals.2 = run_totals.2.max(number("totalLinesRemoved"));
                if let Some(usage) = value.get("modelUsage").and_then(|usage| usage.as_object()) {
                    cost_has_usage = true;
                    let output_tokens: f64 = usage.values().filter_map(|model| model.get("outputTokens")?.as_f64()).sum();
                    run_totals.3 = run_totals.3.max(output_tokens);
                }
            }
            continue;
        }

        // Fast pre-checks before JSON parse
        if line.contains("\"type\":\"assistant\"") {
            prompts.add_assistant_output();
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if value_is_inherited(&value, &own_id) {
                    continue;
                }
                if value.get("entrypoint").and_then(|entrypoint| entrypoint.as_str()).is_some_and(|entrypoint| entrypoint != "cli") {
                    saw_untimed_entrypoint = true;
                }
                let request_id = value
                    .get("requestId")
                    .and_then(|val| val.as_str())
                    .map(String::from);

                if let Some(message) = value.get("message") {
                    if let Some(model) = message.get("model").and_then(|val| val.as_str()) {
                        if model != "<synthetic>" {
                            models_seen.insert(model.to_string());
                        }
                    }
                    if let Some(usage) = message.get("usage") {
                        let tokens = |key: &str| usage.get(key).and_then(|count| count.as_u64()).unwrap_or(0);
                        let input_tokens = tokens("input_tokens");
                        let output_tokens = tokens("output_tokens");
                        let cache_read = tokens("cache_read_input_tokens");
                        let cache_creation = tokens("cache_creation_input_tokens");
                        if let Some(request) = &request_id {
                            // Replace any prior partial entry — last write per request wins
                            token_by_request.insert(
                                request.clone(),
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
                                // Most thinking blocks are signature-only, with nothing to read.
                                "thinking" => {
                                    let has_text = block
                                        .get("thinking")
                                        .and_then(|text| text.as_str())
                                        .is_some_and(|text| !text.trim().is_empty());
                                    if has_text {
                                        stats.thinking_block_count += 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }

                if let Some(request) = request_id {
                    if assistant_requests_seen.insert(request) {
                        stats.assistant_count += 1;
                    }
                }
            }
        } else if line.contains("\"type\":\"user\"") && !line.contains("\"toolUseResult\"") {
            if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
                if entry.entry_type.as_deref() == Some("user") && !entry.is_sidechain.unwrap_or(false) {
                    prompts.add_user_entry(&entry);
                }
            }
        }
    }
    stats.user_prompt_count = prompts.own;
    stats.inherited_prompts = prompts.inherited as u32;
    if !cost_by_run.is_empty() {
        stats.cost_usd = Some(cost_by_run.values().map(|totals| totals.0).sum());
        stats.lines_added = Some(cost_by_run.values().map(|totals| totals.1).sum::<f64>() as u64);
        stats.lines_removed = Some(cost_by_run.values().map(|totals| totals.2).sum::<f64>() as u64);
    }

    for (input_tokens, output_tokens, cache_read, cache_creation) in token_by_request.values() {
        stats.input_tokens += input_tokens;
        stats.output_tokens += output_tokens;
        stats.cache_read_tokens += cache_read;
        stats.cache_creation_tokens += cache_creation;
    }
    // The record also counts subagents, so anything under the main thread's own output means it missed work.
    const COST_COVERAGE: f64 = 0.8;
    let recorded_output: f64 = cost_by_run.values().map(|totals| totals.3).sum();
    stats.cost_is_partial = cost_has_usage && recorded_output < stats.output_tokens as f64 * COST_COVERAGE;
    stats.active_is_partial = saw_untimed_entrypoint || (!saw_turn_duration && stats.assistant_count > 0);

    // Only this session's own folder: a fork's parent ran those agents, not the fork.
    let own_agents_folder = path.with_file_name(&own_id);
    for agent in list_subagents(jsonl_path.clone())? {
        if Path::new(&agent.jsonl_path).starts_with(&own_agents_folder) {
            let (input_tokens, output_tokens) = agent_tokens(Path::new(&agent.jsonl_path));
            stats.agent_input_tokens += input_tokens;
            stats.agent_output_tokens += output_tokens;
        }
    }

    stats.models = models_seen.into_iter().collect();
    stats.models.sort();

    let mut tool_pairs: Vec<ToolCount> = tool_counts
        .into_iter()
        .map(|(name, count)| ToolCount { name, count })
        .collect();
    tool_pairs.sort_by_key(|tool| std::cmp::Reverse(tool.count));
    stats.tool_counts = tool_pairs;

    if stop_summaries != (0, 0) {
        let attachments = hook_counts.entry("Stop".to_string()).or_insert((0, 0));
        *attachments = (attachments.0.max(stop_summaries.0), attachments.1.max(stop_summaries.1));
    }
    stats.hooks = hook_counts
        .into_iter()
        .map(|(event, (runs, failures))| HookStat { event, runs, failures })
        .collect();
    stats.hooks.sort_by_key(|hook| std::cmp::Reverse(hook.runs));

    Ok(stats)
}

/// (input, output) tokens in one subagent's log, one figure per API request.
fn agent_tokens(transcript: &Path) -> (u64, u64) {
    let Ok(file) = fs::File::open(transcript) else { return (0, 0) };
    let mut by_request: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();
    for line in transcript_lines(BufReader::new(file)) {
        if !line.contains("\"type\":\"assistant\"") || !line.contains("\"usage\"") || is_api_record(&line) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        let usage = value.get("message").and_then(|message| message.get("usage"));
        let tokens = |key: &str| usage.and_then(|usage| usage.get(key)).and_then(|count| count.as_u64()).unwrap_or(0);
        if let Some(request) = value.get("requestId").and_then(|request| request.as_str()) {
            by_request.insert(request.to_string(), (tokens("input_tokens"), tokens("output_tokens")));
        }
    }
    by_request.values().fold((0, 0), |totals, tokens| (totals.0 + tokens.0, totals.1 + tokens.1))
}

#[derive(Debug, Serialize, Clone)]
pub struct ToolResultPayload {
    pub content: String,
    pub is_error: bool,
    pub persisted_path: Option<String>,
    /// Images in this result (browser screenshots etc). Their data can be enormous,
    /// so they're fetched on demand with `get_tool_result_images`.
    pub image_count: u32,
}

/// Claude Code appends reminders to the model at the end of tool output. They aren't
/// output. Only trailing ones go: a file or log can legitimately contain that tag.
fn clean_tool_output(text: &str) -> String {
    let mut kept = text.trim_end();
    while let Some(without_close) = kept.strip_suffix("</system-reminder>") {
        match without_close.rfind("<system-reminder>") {
            Some(open_start) => kept = kept[..open_start].trim_end(),
            None => break,
        }
    }
    strip_ansi(kept)
}

/// Flatten a tool_result's content blocks to text. Images are counted, not inlined;
/// tool references and documents become short labels so a result is never blank.
fn tool_result_text(content: Option<&Value>) -> (String, u32) {
    match content {
        Some(Value::String(text)) => (clean_tool_output(text), 0),
        Some(Value::Array(blocks)) => {
            let mut parts = Vec::new();
            let mut image_count = 0;
            for block in blocks {
                match block.get("type").and_then(|kind| kind.as_str()) {
                    Some("image") => image_count += 1,
                    Some("document") => match block.get("title").and_then(|title| title.as_str()) {
                        Some(title) => parts.push(format!("[document: {}]", title)),
                        None => parts.push("[document]".to_string()),
                    },
                    Some("tool_reference") => {
                        let tool_name = block.get("tool_name").and_then(|name| name.as_str()).unwrap_or("?");
                        parts.push(format!("Loaded tool: {}", tool_name));
                    }
                    _ => {
                        if let Some(text) = block.get("text").and_then(|text| text.as_str()) {
                            parts.push(clean_tool_output(text));
                        }
                    }
                }
            }
            (parts.join("\n").trim_end().to_string(), image_count)
        }
        _ => (String::new(), 0),
    }
}

/// Extract every tool_result from a session's JSONL into a map keyed by tool_use_id.
/// The frontend stores this map and renders results when a tool pill is expanded.
#[tauri::command(async)]
pub fn get_tool_results(
    jsonl_path: String,
) -> Result<std::collections::HashMap<String, ToolResultPayload>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }
    if codex::is_codex_path(&path) {
        return codex::tool_results(&path);
    }

    let file = fs::File::open(&path)
        .map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut results: std::collections::HashMap<String, ToolResultPayload> =
        std::collections::HashMap::new();

    for line in transcript_lines(reader) {
        // Fast pre-check — only user entries with tool_result blocks
        if !line.contains("\"tool_result\"") || is_api_record(&line) {
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
        let recorded_path = value
            .get("toolUseResult")
            .and_then(|result| result.get("persistedOutputPath"))
            .and_then(|path| path.as_str())
            .map(String::from);

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
            let (content, image_count) = tool_result_text(block.get("content"));
            let persisted_path = extract_persisted_path(&content).or_else(|| recorded_path.clone());

            results.insert(
                tool_use_id,
                ToolResultPayload {
                    content: readable_output(content),
                    is_error,
                    persisted_path,
                    image_count,
                },
            );
        }
    }

    Ok(results)
}

/// The images inside one tool result, as data URLs. Called when its pill is expanded.
#[tauri::command(async)]
pub fn get_tool_result_images(jsonl_path: String, tool_use_id: String) -> Result<Vec<String>, String> {
    if codex::is_codex_path(Path::new(&jsonl_path)) {
        return codex::tool_result_images(Path::new(&jsonl_path), &tool_use_id);
    }
    let file = fs::File::open(&jsonl_path)
        .map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let needle = format!("\"tool_use_id\":\"{}\"", tool_use_id);
    for line in transcript_lines(BufReader::new(file)) {
        if !line.contains(&needle) || is_api_record(&line) {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let blocks = value.get("message").and_then(|msg| msg.get("content")).and_then(|val| val.as_array());
        for block in blocks.into_iter().flatten() {
            if block.get("tool_use_id").and_then(|id| id.as_str()) != Some(tool_use_id.as_str()) {
                continue;
            }
            let images = block
                .get("content")
                .and_then(|content| content.as_array())
                .into_iter()
                .flatten()
                .filter(|inner| inner.get("type").and_then(|kind| kind.as_str()) == Some("image"))
                .filter_map(image_data_url)
                .collect();
            return Ok(images);
        }
    }
    Ok(Vec::new())
}

/// A tool call's whole input, for when the message's copy was cut.
#[tauri::command(async)]
pub fn get_tool_input(jsonl_path: String, tool_use_id: String) -> Result<String, String> {
    let path = Path::new(&jsonl_path);
    if codex::is_codex_path(path) {
        return codex::tool_input(path, &tool_use_id);
    }
    let file = fs::File::open(path).map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let needle = format!("\"id\":\"{}\"", tool_use_id);
    for line in transcript_lines(BufReader::new(file)) {
        if !line.contains(&needle) || !line.contains("\"type\":\"assistant\"") || is_api_record(&line) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        let blocks = value.get("message").and_then(|message| message.get("content")).and_then(|content| content.as_array());
        let call = blocks.into_iter().flatten().find(|block| block.get("id").and_then(|id| id.as_str()) == Some(tool_use_id.as_str()));
        if let Some(input) = call.and_then(|call| call.get("input")) {
            return Ok(full_tool_input(input));
        }
    }
    Err(format!("No tool call {} in this session", tool_use_id))
}

fn image_data_url(block: &Value) -> Option<String> {
    let source = block.get("source")?;
    let media_type = source.get("media_type").and_then(|value| value.as_str()).unwrap_or("image/png");
    let data = source.get("data").and_then(|value| value.as_str())?;
    Some(format!("data:{};base64,{}", media_type, data))
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
#[tauri::command(async)]
pub fn get_session_questions(
    jsonl_path: String,
) -> Result<std::collections::HashMap<String, Vec<AnsweredQuestion>>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }
    if codex::is_codex_path(&path) {
        return codex::questions(&path);
    }

    let file = fs::File::open(&path)
        .map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut results: std::collections::HashMap<String, Vec<AnsweredQuestion>> =
        std::collections::HashMap::new();

    for line in transcript_lines(reader) {
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
#[tauri::command(async)]
pub fn get_session_artifacts(
    jsonl_path: String,
) -> Result<std::collections::HashMap<String, SessionArtifact>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }
    // Artifacts are a Claude Code tool, and Codex files may be compressed anyway.
    if codex::is_codex_path(&path) {
        return Ok(Default::default());
    }

    let file = fs::File::open(&path)
        .map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader = BufReader::new(file);

    let mut results: std::collections::HashMap<String, SessionArtifact> =
        std::collections::HashMap::new();

    for line in transcript_lines(reader) {
        // Fast pre-check — published URLs always contain this path segment, whether
        // the older /code/artifact/<uuid> or the newer /artifact/<slug>.
        if !line.contains("/artifact/") {
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
            Some(url) if url.contains("/artifact/") => url.to_string(),
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

/// Resolve `path` (following `..` and symlinks) and accept it only if it sits inside
/// `projects_dir` and under a `tool-results` directory. The path comes from transcript
/// text, so a lexical component check alone would let `tool-results/../../x` through.
fn validate_tool_output_path(path: &Path, projects_dir: &Path) -> Result<PathBuf, String> {
    let resolved = fs::canonicalize(path)
        .map_err(|resolve_error| format!("Cannot resolve path: {}", resolve_error))?;
    let projects_root = fs::canonicalize(projects_dir)
        .map_err(|resolve_error| format!("Cannot resolve projects dir: {}", resolve_error))?;
    let inside_tool_results = resolved
        .strip_prefix(&projects_root)
        .map(|relative| relative.components().any(|component| component.as_os_str() == "tool-results"))
        .unwrap_or(false);
    if !inside_tool_results {
        return Err("Path is not inside a Claude Code tool-results directory".to_string());
    }
    Ok(resolved)
}

/// Read the full text of a persisted tool-result sidecar file. Capped at a generous
/// size to avoid yanking gigabytes into the renderer; the frontend warns when truncated.
/// Transcripts record the live path, so once a saved session's live copy has expired
/// the same file name is looked up in its archived `tool-results` folder.
#[tauri::command(async)]
pub fn read_tool_output_file(
    app: tauri::AppHandle,
    path: String,
    session_id: Option<String>,
) -> Result<String, String> {
    let live_path = PathBuf::from(&path);
    if live_path.exists() {
        let validated = validate_tool_output_path(&live_path, &get_claude_projects_dir()?)?;
        return read_capped_text(&validated);
    }
    let root = archive_root(&app)?;
    let archived_path = session_id
        .zip(live_path.file_name().map(|name| name.to_os_string()))
        .map(|(session_id, file_name)| {
            root.join(&session_id).join(&session_id).join("tool-results").join(file_name)
        })
        .filter(|candidate| candidate.exists())
        .ok_or_else(|| format!("File not found: {}", path))?;
    read_capped_text(&validate_tool_output_path(&archived_path, &root)?)
}

fn read_capped_text(path: &Path) -> Result<String, String> {
    const MAX_BYTES: u64 = 5 * 1024 * 1024; // 5 MB
    let metadata = path
        .metadata()
        .map_err(|metadata_error| format!("Cannot stat: {}", metadata_error))?;
    if metadata.len() > MAX_BYTES {
        // Read a 5 MB head and append a notice
        use std::io::Read;
        let mut file = fs::File::open(path)
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

    let bytes = fs::read(path).map_err(|read_error| format!("Cannot read: {}", read_error))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// Longest output kept per tool call in an export, stated at the top of a file it cuts.
const EXPORT_OUTPUT_LIMIT: usize = 8000;

fn longest_backtick_run(content: &str) -> usize {
    let mut longest_run = 0;
    let mut current_run = 0;
    for character in content.chars() {
        current_run = if character == '`' { current_run + 1 } else { 0 };
        longest_run = longest_run.max(current_run);
    }
    longest_run
}

/// A fenced code block whose fence is longer than any backtick run inside `content`,
/// so tool output containing ``` can't break out of it.
fn fenced(content: &str) -> String {
    let fence = "`".repeat((longest_backtick_run(content) + 1).max(3));
    format!("{fence}\n{}\n{fence}\n\n", content.trim_end_matches('\n').replace('\0', ""))
}

/// Inline code, using a backtick delimiter longer than any run inside the text.
fn inline_code(text: &str) -> String {
    let flattened = text.replace('\n', " ");
    let delimiter = "`".repeat(longest_backtick_run(&flattened) + 1);
    if flattened.starts_with('`') || flattened.ends_with('`') {
        format!("{delimiter} {flattened} {delimiter}")
    } else {
        format!("{delimiter}{flattened}{delimiter}")
    }
}

/// The opening or closing fence a line would be, as (fence character, length, what follows).
fn fence_on_line(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let fence_character = trimmed.chars().next().filter(|character| matches!(character, '`' | '~'))?;
    let length = trimmed.chars().take_while(|character| *character == fence_character).count();
    (length >= 3).then(|| (fence_character, length, &trimmed[length..]))
}

/// Escape `<` where a Markdown renderer would start raw HTML, skipping inline code.
fn escape_html_outside_code(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(position) = rest.find(['`', '<']) {
        output.push_str(&rest[..position]);
        rest = &rest[position..];
        if rest.starts_with('<') {
            let starts_tag = rest[1..].chars().next().is_some_and(|next| next.is_ascii_alphabetic() || matches!(next, '/' | '!' | '?'));
            output.push_str(if starts_tag { "\\<" } else { "<" });
            rest = &rest[1..];
            continue;
        }
        // A code span ends at the next run of exactly the same length.
        let run_length = rest.chars().take_while(|character| *character == '`').count();
        let closing = rest[run_length..].match_indices('`').find_map(|(offset, _)| {
            let start = run_length + offset;
            let is_run_start = !rest[..start].ends_with('`');
            let closing_run = rest[start..].chars().take_while(|character| *character == '`').count();
            (is_run_start && closing_run == run_length).then_some(start + run_length)
        });
        let span_end = closing.unwrap_or(run_length);
        output.push_str(&rest[..span_end]);
        rest = &rest[span_end..];
    }
    output.push_str(rest);
    output
}

/// Headings in message text sit below the file's own, so they can't pose as new turns.
fn demoted_heading(line: &str) -> Option<String> {
    const LEVELS_DOWN: usize = 2;
    let level = line.chars().take_while(|character| *character == '#').count();
    let is_heading = (1..=6).contains(&level) && line[level..].chars().next().is_none_or(|next| next == ' ' || next == '\t');
    is_heading.then(|| format!("{}{}", "#".repeat((level + LEVELS_DOWN).min(6)), &line[level..]))
}

/// Message text as Markdown that stays inside its own block: real code fences are kept, a
/// fence nothing closes becomes literal text, raw HTML is shown rather than rendered, and
/// headings move down.
fn contained_markdown(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut output = String::with_capacity(text.len());
    let mut open_fence: Option<(char, usize)> = None;
    let mut previous_blank = true;
    for (line_index, line) in lines.iter().enumerate() {
        let fence = fence_on_line(line);
        let closes = |character: char, length: usize, candidate: &str| {
            fence_on_line(candidate).is_some_and(|(found, found_length, after)| found == character && found_length >= length && after.trim().is_empty())
        };
        let rendered = if let Some((fence_character, fence_length)) = open_fence {
            if closes(fence_character, fence_length, line) {
                open_fence = None;
            }
            (*line).to_string()
        } else if let Some((fence_character, fence_length, _)) = fence.filter(|(found, _, info)| *found == '~' || !info.contains('`')) {
            if lines[line_index + 1..].iter().any(|candidate| closes(fence_character, fence_length, candidate)) {
                open_fence = Some((fence_character, fence_length));
                (*line).to_string()
            } else {
                format!("\\{}", line.trim_start())
            }
        } else if previous_blank && (line.starts_with("    ") || line.starts_with('\t')) {
            (*line).to_string() // indented code
        } else if let Some(heading) = demoted_heading(line) {
            escape_html_outside_code(&heading)
        } else if !previous_blank && !line.is_empty() && (line.chars().all(|character| character == '=') || line.chars().all(|character| character == '-')) {
            format!("\\{}", line) // would turn the line above it into a heading
        } else {
            escape_html_outside_code(line)
        };
        previous_blank = line.trim().is_empty();
        output.push_str(&rendered);
        if line_index + 1 < lines.len() {
            output.push('\n');
        }
    }
    output
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;")
}

fn collapsible(summary: &str, body: &str) -> String {
    format!("<details>\n<summary>{}</summary>\n\n{}</details>\n\n", escape_html(summary), body)
}

/// Local time with its zone, as the screen shows it. A timestamp that doesn't parse stays as written.
fn export_time(timestamp: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(timestamp).map_or_else(
        |_| timestamp.to_string(),
        |parsed| parsed.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S %:z").to_string(),
    )
}

/// `mcp__team-db__query` → `team-db / query`, the name the screen shows.
fn pretty_tool_name(name: &str) -> String {
    let Some(rest) = name.strip_prefix("mcp__") else { return name.to_string() };
    match rest.split_once("__") {
        Some((server, method)) => format!("{} / {}", server, method.replace("__", ".")),
        None => rest.to_string(),
    }
}

/// Terminal output with bytes in it is noise in a text file. Transcripts do hold PNGs and
/// other files that a command printed.
/// What a tool result panel shows. Raw bytes (a PNG sent to stdout) are unreadable noise.
fn readable_output(text: String) -> String {
    if is_binary_output(&text) {
        format!("[Binary output of {} characters, not shown]", text.chars().count())
    } else {
        text
    }
}

fn is_binary_output(text: &str) -> bool {
    let replacement_count = text.chars().filter(|character| *character == '\u{fffd}').count();
    text.contains('\0') || replacement_count * 100 > text.chars().count().max(800)
}

/// The preview inside a `<persisted-output>` wrapper, without the wrapper or its internal path.
fn persisted_preview(content: &str) -> Option<String> {
    let (before, rest) = content.split_once("<persisted-output>")?;
    let (inside, after) = rest.split_once("</persisted-output>")?;
    let preview = inside.split_once("):").filter(|(label, _)| label.contains("Preview (first")).map_or(inside, |(_, preview)| preview);
    let parts = [before.trim(), preview.trim(), after.trim()];
    Some(parts.iter().filter(|part| !part.is_empty()).copied().collect::<Vec<_>>().join("\n\n"))
}

/// Everything an export needs beyond the messages, all keyed by tool_use_id.
struct ExportContext {
    results: std::collections::HashMap<String, ToolResultPayload>,
    questions: std::collections::HashMap<String, Vec<AnsweredQuestion>>,
    artifacts: std::collections::HashMap<String, SessionArtifact>,
    full_inputs: std::collections::HashMap<String, String>,
    /// Agent and workflow-run ids whose transcripts this file includes.
    included_agents: std::collections::HashSet<String>,
    transcript: PathBuf,
    /// "Claude" or "Codex"; a subagent's own log says "Agent".
    agent_label: &'static str,
    is_agent_log: bool,
    /// How many outputs the cap cut, so the file can say so.
    cut_outputs: std::cell::Cell<u32>,
}

impl ExportContext {
    fn for_transcript(path: &str, messages: &[ConversationMessage], is_agent_log: bool) -> Result<Self, String> {
        let transcript = PathBuf::from(path);
        let is_codex = codex::is_codex_path(&transcript);
        Ok(ExportContext {
            results: get_tool_results(path.to_string())?,
            questions: get_session_questions(path.to_string())?,
            artifacts: get_session_artifacts(path.to_string())?,
            full_inputs: full_inputs_of_cut_tools(path, messages),
            included_agents: Default::default(),
            agent_label: match (is_agent_log, is_codex) {
                (true, _) => "Agent",
                (false, true) => "Codex",
                (false, false) => "Claude",
            },
            transcript,
            is_agent_log,
            cut_outputs: std::cell::Cell::new(0),
        })
    }

    fn capped(&self, text: &str) -> String {
        if is_binary_output(text) {
            return format!("[Binary output of {} characters, not exported]", text.chars().count());
        }
        let total = text.chars().count();
        if total <= EXPORT_OUTPUT_LIMIT {
            return text.to_string();
        }
        self.cut_outputs.set(self.cut_outputs.get() + 1);
        let head: String = text.chars().take(EXPORT_OUTPUT_LIMIT).collect();
        format!("{}\n\n[…{} more characters not exported]", head, total - EXPORT_OUTPUT_LIMIT)
    }

    /// A tool's output, with saved-aside output read back from its file. That file sits beside
    /// the transcript in live sessions and saved copies alike.
    fn output_text(&self, result: &ToolResultPayload) -> Option<String> {
        let preview = persisted_preview(&result.content)?;
        let recorded_path = result.persisted_path.as_deref().map(Path::new);
        let beside_transcript = recorded_path.and_then(|recorded| recorded.file_name()).and_then(|file_name| {
            read_capped_text(&self.transcript.with_file_name(own_session_id(&self.transcript)).join("tool-results").join(file_name)).ok()
        });
        let full_text = beside_transcript.or_else(|| {
            // A fork's output can live in its parent's folder.
            let projects = get_claude_projects_dir().ok()?;
            read_capped_text(&validate_tool_output_path(recorded_path?, &projects).ok()?).ok()
        });
        Some(full_text.unwrap_or_else(|| format!("{}\n\n[Full output is no longer on disk]", preview)))
    }
}

/// Whole inputs for the tool calls whose message copy was cut.
fn full_inputs_of_cut_tools(path: &str, messages: &[ConversationMessage]) -> std::collections::HashMap<String, String> {
    let mut cut_ids = std::collections::HashSet::new();
    for block in messages.iter().flat_map(|message| &message.blocks) {
        if let MessageBlock::Tool { input_truncated: true, tool_use_id: Some(id), .. } = block {
            cut_ids.insert(id.clone());
        }
    }
    let mut inputs = std::collections::HashMap::new();
    if cut_ids.is_empty() {
        return inputs;
    }
    if codex::is_codex_path(Path::new(path)) {
        for id in cut_ids {
            if let Ok(input) = codex::tool_input(Path::new(path), &id) {
                inputs.insert(id, input);
            }
        }
        return inputs;
    }
    let Ok(file) = fs::File::open(path) else { return inputs };
    for line in transcript_lines(BufReader::new(file)) {
        if !line.contains("\"type\":\"assistant\"") || !line.contains("\"tool_use\"") || is_api_record(&line) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        let blocks = value.get("message").and_then(|message| message.get("content")).and_then(|content| content.as_array());
        for block in blocks.into_iter().flatten() {
            if let (Some(id), Some(input)) = (block.get("id").and_then(|id| id.as_str()), block.get("input")) {
                if cut_ids.contains(id) {
                    inputs.insert(id.to_string(), full_tool_input(input));
                }
            }
        }
    }
    inputs
}

fn render_tool_for_export(
    name: &str,
    summary: &str,
    input: &str,
    tool_use_id: Option<&String>,
    agent_id: Option<&String>,
    context: &ExportContext,
) -> String {
    if let Some(artifact) = tool_use_id.and_then(|id| context.artifacts.get(id)).filter(|_| name == "Artifact") {
        let title = if artifact.title.is_empty() { "Untitled artifact" } else { &artifact.title };
        return format!("**Published artifact:** [{}]({})\n\n", contained_markdown(title), artifact.url);
    }
    if let Some(questions) = tool_use_id.and_then(|id| context.questions.get(id)).filter(|_| name == "AskUserQuestion") {
        let mut rendered = String::new();
        for question in questions {
            rendered.push_str(&format!("**{} asked:** {}\n\n", context.agent_label, contained_markdown(&question.question)));
            for option in &question.options {
                let mark = if option.chosen { "x" } else { " " };
                let suffix = if option.custom { " _(your answer)_" } else { "" };
                rendered.push_str(&format!("- [{}] {}{}\n", mark, contained_markdown(&option.label), suffix));
            }
            rendered.push('\n');
            if let Some(notes) = &question.notes {
                rendered.push_str(&format!("**Your note:** {}\n\n", contained_markdown(notes)));
            }
        }
        return rendered;
    }

    let shown_name = pretty_tool_name(name);
    let mut rendered = if summary.is_empty() {
        format!("**{}**\n\n", shown_name)
    } else {
        format!("**{}** {}\n\n", shown_name, inline_code(summary))
    };
    let whole_input = tool_use_id.and_then(|id| context.full_inputs.get(id)).map_or(input, String::as_str);
    if !whole_input.is_empty() {
        rendered.push_str(&collapsible("Input", &fenced(whole_input)));
    }
    if let Some(result) = tool_use_id.and_then(|id| context.results.get(id)) {
        let output = context.output_text(result).unwrap_or_else(|| result.content.clone());
        if !output.is_empty() {
            let label = if result.is_error { "Output (error)" } else { "Output" };
            // An agent's output is its report, usually the point of the call.
            let is_agent_report = matches!(name, "Agent" | "agent" | "Task") && !is_binary_output(&output);
            let body = if is_agent_report { output } else { context.capped(&output) };
            rendered.push_str(&collapsible(label, &fenced(&body)));
        }
        if result.image_count > 0 {
            rendered.push_str(&format!("_{} image(s) in this result, not embedded._\n\n", result.image_count));
        }
    }
    if let Some(agent_id) = agent_id.filter(|agent_id| context.included_agents.contains(*agent_id)) {
        rendered.push_str(&format!("_Agent {}: transcript at the end of this file._\n\n", agent_id));
    }
    rendered
}

/// A pasted image's cached file, in the live cache or beside a saved copy.
fn exported_image_file(transcript: &Path, image_number: u32) -> Option<PathBuf> {
    let session_id = own_session_id(transcript);
    let live_cache = dirs::home_dir().map(|home| home.join(".claude").join("image-cache"));
    let saved_cache = transcript.parent().and_then(|parent| parent.parent()).map(|root| root.join("image-cache"));
    [live_cache, saved_cache]
        .into_iter()
        .flatten()
        .find_map(|cache| find_image_file(&cache.join(&session_id), image_number))
}

fn event_label(kind: &str) -> Option<&'static str> {
    match kind {
        "api-error" => Some("API error"),
        "recap" => Some("Recap"),
        "notice" => Some("Notice"),
        "command" => Some("Command"),
        "files" => Some("Files"),
        "hook" => Some("Hook"),
        _ => None,
    }
}

fn quoted(text: &str) -> String {
    contained_markdown(text).replace('\n', "\n> ")
}

fn render_message_for_export(message: &ConversationMessage, context: &ExportContext) -> String {
    let mut rendered = String::new();
    let time = export_time(&message.timestamp);
    match message.role.as_str() {
        "user" => {
            let heading = match (context.is_agent_log, message.mid_turn) {
                (true, _) => "Brief",
                (false, true) => "You (mid-turn)",
                (false, false) => "You",
            };
            rendered.push_str(&format!("## {} — {}\n\n{}\n\n", heading, time, contained_markdown(&message.text)));
            for image in &message.images {
                match exported_image_file(&context.transcript, image.number) {
                    Some(file) => rendered.push_str(&format!("![Image #{}](<{}>)\n\n", image.number, file.display())),
                    None => rendered.push_str(&format!("_Image #{} attached, not embedded._\n\n", image.number)),
                }
            }
        }
        "assistant" => {
            rendered.push_str(&format!("## {} — {}\n\n", context.agent_label, time));
            for block in &message.blocks {
                match block {
                    MessageBlock::Text { text } => rendered.push_str(&format!("{}\n\n", contained_markdown(text))),
                    MessageBlock::Thinking { text } => {
                        rendered.push_str(&collapsible("Thinking…", &format!("{}\n\n", contained_markdown(text.trim()))));
                    }
                    MessageBlock::Tool { name, summary, input, tool_use_id, agent_id, .. } => {
                        rendered.push_str(&render_tool_for_export(name, summary, input, tool_use_id.as_ref(), agent_id.as_ref(), context));
                    }
                    MessageBlock::Plan { items } => {
                        for item in items {
                            let mark = if item.status == "completed" { "x" } else { " " };
                            let suffix = if item.status == "in_progress" { " _(in progress)_" } else { "" };
                            rendered.push_str(&format!("- [{}] {}{}\n", mark, contained_markdown(&item.content), suffix));
                        }
                        rendered.push('\n');
                    }
                    MessageBlock::Note { label, text } => {
                        rendered.push_str(&format!("> **{}:** {}\n\n", contained_markdown(label), quoted(text)));
                    }
                    // Successful hooks are hidden on screen too.
                    MessageBlock::Hook { label, text, is_error } => {
                        if *is_error {
                            rendered.push_str(&collapsible(label, &fenced(&context.capped(text))));
                        }
                    }
                }
            }
            if message.interrupted {
                rendered.push_str("_Interrupted by user._\n\n");
            }
        }
        "compaction" => {
            let summary = match &message.compaction_detail {
                Some(detail) => format!("Context compacted ({}) — {}", detail, time),
                None => format!("Context compacted — {}", time),
            };
            rendered.push_str(&collapsible(&summary, &format!("{}\n\n", contained_markdown(&message.text))));
        }
        "agent-notification" => {
            if let Some(note) = &message.notification {
                rendered.push_str(&format!(
                    "### {} ({}) — {}\n\n",
                    contained_markdown(&note.summary), note.status, time
                ));
                if !note.result.is_empty() {
                    rendered.push_str(&collapsible("Result", &fenced(&note.result)));
                }
            }
        }
        "shell" => {
            if let Some(run) = &message.shell {
                rendered.push_str(&format!("## You ran — {}\n\n", time));
                rendered.push_str(&fenced(&format!("$ {}", run.command)));
                if !run.stdout.is_empty() {
                    rendered.push_str(&fenced(&context.capped(&run.stdout)));
                }
                if !run.stderr.is_empty() {
                    rendered.push_str(&collapsible("stderr", &fenced(&context.capped(&run.stderr))));
                }
            }
        }
        "system-event" => {
            let event = message.event.as_ref();
            // Successful hooks are hidden on screen too.
            if event.is_some_and(|event| event.kind == "hook" && !event.is_error) {
                return rendered;
            }
            let label = event.and_then(|event| event_label(&event.kind)).map_or(String::new(), |label| format!("**{}:** ", label));
            rendered.push_str(&format!("> {}{} — {}\n\n", label, quoted(&message.text), time));
            if let Some(event) = event.filter(|event| !event.detail.is_empty()) {
                rendered.push_str(&collapsible("Details", &fenced(&context.capped(&event.detail))));
            }
        }
        _ => {}
    }
    rendered
}

/// Agents' own logs as collapsed sections. In a partial export, only the agents it mentions.
fn render_agent_transcripts(agents: &[SubagentInfo], cut_outputs: &std::cell::Cell<u32>) -> String {
    if agents.is_empty() {
        return String::new();
    }
    let mut rendered = String::from("## Agent transcripts\n\n");
    for agent in agents {
        let name = agent.description.as_deref().or(agent.agent_type.as_deref()).unwrap_or("Agent");
        let body = get_subagent_messages(agent.jsonl_path.clone()).and_then(|messages| {
            let context = ExportContext::for_transcript(&agent.jsonl_path, &messages, true)?;
            let text: String = messages.iter().map(|message| render_message_for_export(message, &context)).collect();
            cut_outputs.set(cut_outputs.get() + context.cut_outputs.get());
            Ok(text)
        });
        let body = body.unwrap_or_else(|read_error| format!("_This transcript couldn't be read: {}_\n\n", contained_markdown(&read_error)));
        rendered.push_str(&collapsible(&format!("{} · {}", name, agent.agent_id), &body));
    }
    rendered
}

#[tauri::command(async)]
pub fn export_session_markdown(
    jsonl_path: String,
    save_path: String,
    title: Option<String>,
    // 0-based index of the first message to include. None/0 exports the whole
    // session; a larger value exports from that message through the end (the end
    // is always the session end — only the start is trimmable).
    start_index: Option<usize>,
) -> Result<(), String> {
    let messages = get_session_messages(jsonl_path.clone())?;
    let start = start_index.unwrap_or(0).min(messages.len());
    let exported = &messages[start..];
    let mut context = ExportContext::for_transcript(&jsonl_path, exported, false)?;

    // In a partial export, only the agents the exported part called.
    let mut agents = list_subagents(jsonl_path)?;
    let mut called_tools = std::collections::HashSet::new();
    let mut called_agents = std::collections::HashSet::new();
    for block in exported.iter().flat_map(|message| &message.blocks) {
        if let MessageBlock::Tool { tool_use_id, agent_id, .. } = block {
            called_tools.extend(tool_use_id.clone());
            called_agents.extend(agent_id.clone());
        }
    }
    if start > 0 {
        agents.retain(|agent| {
            agent.tool_use_id.as_ref().is_some_and(|id| called_tools.contains(id))
                || agent.workflow_run.as_ref().is_some_and(|run| called_agents.contains(run))
        });
    }
    context.included_agents = agents.iter().flat_map(|agent| [Some(agent.agent_id.clone()), agent.workflow_run.clone()]).flatten().collect();

    let body: String = exported.iter().map(|message| render_message_for_export(message, &context)).collect();
    let agent_transcripts = render_agent_transcripts(&agents, &context.cut_outputs);

    let mut markdown = String::new();
    if let Some(title) = title.as_ref().filter(|title| !title.is_empty()) {
        markdown.push_str(&format!("# {}\n\n", contained_markdown(&title.replace('\n', " "))));
    }
    if start > 0 {
        let from = exported.iter().map(|message| message.timestamp.as_str()).find(|timestamp| !timestamp.is_empty()).unwrap_or_default();
        markdown.push_str(&format!("_Partial export: from {} to the end of the session._\n\n", export_time(from)));
    }
    if context.cut_outputs.get() > 0 {
        markdown.push_str(&format!(
            "_Output over {} characters is cut in this file ({} places). The app shows it in full._\n\n",
            EXPORT_OUTPUT_LIMIT,
            context.cut_outputs.get()
        ));
    }
    markdown.push_str(&body);
    markdown.push_str(&agent_transcripts);

    fs::write(&save_path, markdown)
        .map_err(|write_error| format!("Cannot write {}: {}", save_path, write_error))?;
    Ok(())
}
#[derive(Debug, Serialize, Clone)]
pub struct SubagentInfo {
    pub agent_id: String,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    pub jsonl_path: String,
    pub tool_use_id: Option<String>,
    /// The workflow run this agent belongs to, for agents a Workflow call spawned.
    pub workflow_run: Option<String>,
}

/// Builds a (tool_use_id → agent id or workflow run id) map by scanning
/// user/toolUseResult entries. The parent JSONL has `toolUseResult.agentId` (or
/// `runId`) and `message.content[].tool_use_id` in the same entry, so Agent and
/// Workflow calls can be correlated to their transcripts.
fn build_tool_to_agent_map(jsonl_path: &Path) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let file = match fs::File::open(jsonl_path) {
        Ok(file) => file,
        Err(_) => return map,
    };
    let reader = BufReader::new(file);
    for line in transcript_lines(reader) {
        // Fast pre-check before JSON parse — most lines aren't tool results
        if !line.contains("\"toolUseResult\"") || !(line.contains("\"agentId\"") || line.contains("\"runId\"")) {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let result = value.get("toolUseResult");
        let linked_id = result
            .and_then(|val| val.get("agentId"))
            .or_else(|| result.and_then(|val| val.get("runId")))
            .and_then(|val| val.as_str());
        let linked_id = match linked_id {
            Some(id) => id.to_string(),
            None => continue,
        };
        if let Some(tool_use_id) = find_tool_result_id(&value) {
            map.insert(tool_use_id, linked_id);
        }
    }
    map
}

/// Read the agent transcripts in one directory (direct subagents or one workflow run).
fn subagents_in(
    directory: &Path,
    workflow_run: Option<&str>,
    agent_to_tool: &std::collections::HashMap<String, String>,
) -> Vec<SubagentInfo> {
    let mut subagents = Vec::new();
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        let entry_path = entry.path();
        if !is_transcript_file(&entry_path) {
            continue;
        }
        // Workflow folders also hold bookkeeping JSONL; agent logs have this prefix.
        let agent_id = match entry_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.strip_prefix("agent-"))
        {
            Some(agent_id) => agent_id.to_string(),
            None => continue,
        };

        // Sidecar meta file has agentType + description
        let meta: Option<Value> = fs::read_to_string(entry_path.with_extension("meta.json"))
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok());
        let meta_text = |key: &str| {
            meta.as_ref().and_then(|value| value.get(key)).and_then(|val| val.as_str()).map(String::from)
        };

        subagents.push(SubagentInfo {
            tool_use_id: agent_to_tool.get(&agent_id).cloned(),
            agent_type: meta_text("agentType"),
            description: meta_text("description"),
            jsonl_path: entry_path.to_string_lossy().to_string(),
            workflow_run: workflow_run.map(String::from),
            agent_id,
        });
    }
    subagents
}

#[tauri::command(async)]
pub fn list_subagents(jsonl_path: String) -> Result<Vec<SubagentInfo>, String> {
    let path = PathBuf::from(&jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }
    if codex::is_codex_path(&path) {
        return Ok(without_duplicate_agents(codex::subagents(&path)));
    }

    Ok(without_duplicate_agents(claude_subagents(&path, &build_tool_to_agent_map(&path))))
}

fn claude_subagents(path: &Path, tool_to_agent: &std::collections::HashMap<String, String>) -> Vec<SubagentInfo> {
    // Subagent dir is `<parent_dir>/<session_id>/subagents/`, with one more folder per
    // workflow run at `subagents/workflows/<run_id>/`.
    let parent_dir = match path.parent() {
        Some(parent) => parent,
        None => return Vec::new(),
    };
    let session_id = own_session_id(path);
    // Reverse the (tool_use_id → agent_id) map to (agent_id → tool_use_id) so we can
    // attach the originating tool_use_id to each subagent if present.
    let agent_to_tool: std::collections::HashMap<String, String> = tool_to_agent
        .iter()
        .map(|(tool_use_id, agent_id)| (agent_id.clone(), tool_use_id.clone()))
        .collect();

    // A fork inherits its parent's history, including Agent calls whose transcripts live
    // in the parent's folder. Walk up the fork chain so those stay openable.
    let mut subagents = Vec::new();
    let mut visited_ids = std::collections::HashSet::new();
    let mut current_id = Some(session_id.clone());
    while let Some(id) = current_id.filter(|id| visited_ids.insert(id.clone())) {
        let subagents_dir = parent_dir.join(&id).join("subagents");
        let mut found = subagents_in(&subagents_dir, None, &agent_to_tool);
        for run_entry in fs::read_dir(subagents_dir.join("workflows")).into_iter().flatten().flatten() {
            let run_id = run_entry.file_name().to_string_lossy().to_string();
            found.extend(subagents_in(&run_entry.path(), Some(&run_id), &agent_to_tool));
        }
        // A parent keeps running after the fork, so only agents this transcript actually
        // called are part of its history.
        if id != session_id {
            found.retain(|agent| {
                agent_to_tool.contains_key(&agent.agent_id)
                    || agent.workflow_run.as_ref().is_some_and(|run_id| agent_to_tool.contains_key(run_id))
            });
        }
        subagents.extend(found);
        current_id = detect_fork_parent(&parent_dir.join(format!("{}.jsonl", id)));
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

    subagents
}

/// A fork's folder and its parent's can hold a copy of the same agent, and the UI keys on the id.
fn without_duplicate_agents(mut subagents: Vec<SubagentInfo>) -> Vec<SubagentInfo> {
    let mut seen_ids = std::collections::HashSet::new();
    subagents.retain(|agent| seen_ids.insert(agent.agent_id.clone()));
    subagents
}
#[tauri::command(async)]
pub fn get_subagent_messages(jsonl_path: String) -> Result<Vec<ConversationMessage>, String> {
    // A subagent log has the same shape as a session, except every entry is a sidechain.
    parse_transcript(&jsonl_path, true)
}

#[tauri::command(async)]
pub fn get_session_messages(jsonl_path: String) -> Result<Vec<ConversationMessage>, String> {
    parse_transcript(&jsonl_path, false)
}

fn parse_transcript(jsonl_path: &str, include_sidechain: bool) -> Result<Vec<ConversationMessage>, String> {
    let path = PathBuf::from(jsonl_path);
    if !path.exists() {
        return Err(format!("Session file not found: {}", jsonl_path));
    }
    if codex::is_codex_path(&path) {
        return codex::parse_messages(&path);
    }

    let tool_to_agent = build_tool_to_agent_map(&path);
    let mut parser = TranscriptParser::with_agents(&tool_to_agent);
    parser.is_subagent = include_sidechain;
    parser.own_session_id = own_session_id(&path);
    if !include_sidechain {
        for agent in claude_subagents(&path, &tool_to_agent) {
            if let Some(name) = agent.description.or(agent.agent_type) {
                parser.agent_names.insert(agent.agent_id, name);
            }
        }
    }

    let file =
        fs::File::open(&path).map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let mut lines = transcript_lines(BufReader::new(file));
    for line in lines.by_ref() {
        if line.trim().is_empty() || is_api_record(&line) {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
            parser.process(entry);
        }
    }
    if lines.read_failed {
        parser.note_early_end();
    }
    Ok(parser.finish())
}

/// Walks a transcript's entries in order, grouping assistant output into turns.
struct TranscriptParser<'a> {
    tool_to_agent: &'a std::collections::HashMap<String, String>,
    messages: Vec<ConversationMessage>,
    turn_blocks: Vec<MessageBlock>,
    turn_timestamp: String,
    turn_inherited: bool,
    /// The file's session id; lines from another session were inherited.
    own_session_id: String,
    /// The entry being processed was copied from a parent session.
    entry_inherited: bool,
    fork_point_shown: bool,
    /// Agent id → the name it was given, for hand-backs that only carry the id.
    agent_names: std::collections::HashMap<String, String>,
    /// The plan as of the last update, since most updates only name the steps that changed.
    current_plan: Vec<PlanItem>,
    /// A queued message can be stored as an attachment, a normal turn, or both, one right after
    /// the other. These hold what arrived since the agent last wrote anything.
    recent_typed: Vec<String>,
    recent_queued: Vec<String>,
    /// (parent, text) of each prompt: a retry is written as a sibling with the same parent.
    seen_prompts: std::collections::HashSet<(String, String)>,
    /// One API response is written as several lines sharing a requestId, each holding
    /// the next content block. Buffered and merged, then committed when another
    /// requestId (or any non-assistant entry) appears.
    pending_assistant: Option<JsonlEntry>,
    /// A task notification can be stored as both a delivered user entry and a queued
    /// attachment; this set ensures a given notification renders once.
    seen_notifications: std::collections::HashSet<String>,
    /// A misconfigured hook can fail thousands of times with the same error. Show each
    /// distinct output once; the Stats panel has the totals.
    seen_hook_output: std::collections::HashSet<String>,
    /// A compact_boundary's token counts and time, shown on the summary that follows it.
    pending_compaction: Option<(Option<String>, String)>,
    /// How many hook events the latest merged hook row holds (0 = not merged yet).
    merged_hook_rows: usize,
    /// Parsing a subagent's own log, where every entry is a sidechain.
    is_subagent: bool,
}

impl<'a> TranscriptParser<'a> {
    fn with_agents(tool_to_agent: &'a std::collections::HashMap<String, String>) -> Self {
        TranscriptParser {
            tool_to_agent,
            messages: Vec::new(),
            turn_blocks: Vec::new(),
            turn_timestamp: String::new(),
            turn_inherited: false,
            own_session_id: String::new(),
            entry_inherited: false,
            fork_point_shown: false,
            agent_names: Default::default(),
            current_plan: Vec::new(),
            recent_typed: Vec::new(),
            recent_queued: Vec::new(),
            seen_prompts: Default::default(),
            pending_assistant: None,
            seen_notifications: std::collections::HashSet::new(),
            seen_hook_output: std::collections::HashSet::new(),
            pending_compaction: None,
            merged_hook_rows: 0,
            is_subagent: false,
        }
    }

    fn process(&mut self, entry: JsonlEntry) {
        if entry.is_sidechain.unwrap_or(false) && !self.is_subagent {
            return;
        }
        if matches!(entry.entry_type.as_deref(), Some("assistant" | "user" | "attachment" | "system")) {
            self.track_fork_point(&entry);
        }
        if entry.message.as_ref().and_then(|message| message.model.as_deref()) == Some("<synthetic>") {
            self.process_synthetic(entry);
            return;
        }
        match entry.entry_type.as_deref() {
            Some("assistant") => self.process_assistant(entry),
            Some("user") => {
                self.commit_pending_assistant();
                self.process_user(entry);
            }
            Some("attachment") => {
                self.commit_pending_assistant();
                self.process_attachment(entry);
            }
            Some("system") => {
                self.commit_pending_assistant();
                self.process_system(entry);
            }
            // Every other entry type is Claude Code bookkeeping (api-request, mode, ...).
            _ => {}
        }
    }

    /// Mark where a fork's own work begins, the first time a copied entry is followed by one of its own.
    fn track_fork_point(&mut self, entry: &JsonlEntry) {
        // Every line of a subagent's log carries its parent's session id.
        let inherited = !self.is_subagent && entry_is_inherited(entry, &self.own_session_id);
        if self.entry_inherited && !inherited && !self.fork_point_shown {
            self.commit_pending_assistant();
            self.flush_turn();
            self.entry_inherited = false;
            self.fork_point_shown = true;
            self.push_event("fork", false, "Forked from another session here".to_string(), String::new(), entry.timestamp.clone().unwrap_or_default());
        }
        self.entry_inherited = inherited;
    }

    /// Reading stopped before the end of the file, so what's shown isn't the whole session.
    fn note_early_end(&mut self) {
        self.commit_pending_assistant();
        self.flush_turn();
        let timestamp = self.messages.last().map(|message| message.timestamp.clone()).unwrap_or_default();
        self.push_event("notice", true, "This transcript ended early: the rest of the file couldn't be read.".to_string(), String::new(), timestamp);
    }

    /// Claude Code writes its own assistant-shaped entries, most importantly API errors
    /// that ended a turn. "No response requested." is just a placeholder.
    fn process_synthetic(&mut self, entry: JsonlEntry) {
        let text = prose_text(&entry);
        let text = text.trim();
        if text.is_empty() || text == "No response requested." {
            return;
        }
        self.commit_pending_assistant();
        let is_error = text.starts_with("API Error");
        self.push_event(
            if is_error { "api-error" } else { "notice" },
            is_error,
            text.to_string(),
            String::new(),
            entry.timestamp.unwrap_or_default(),
        );
    }

    fn finish(mut self) -> Vec<ConversationMessage> {
        self.commit_pending_assistant();
        self.flush_turn();
        self.messages
    }

    fn process_assistant(&mut self, entry: JsonlEntry) {
        if let Some(pending) = self.pending_assistant.as_mut() {
            if entry.request_id.is_some() && pending.request_id == entry.request_id {
                merge_content_blocks(pending, entry);
                return;
            }
        }
        self.commit_pending_assistant();
        self.pending_assistant = Some(entry);
    }

    fn commit_pending_assistant(&mut self) {
        let entry = match self.pending_assistant.take() {
            Some(entry) => entry,
            None => return,
        };
        let content = entry.message.as_ref().and_then(|message| message.content.as_ref());
        let blocks = assistant_blocks(content, self.tool_to_agent, &mut self.current_plan);
        if blocks.is_empty() {
            return;
        }
        self.recent_typed.clear();
        self.recent_queued.clear();
        if self.turn_blocks.is_empty() {
            self.turn_inherited = !self.is_subagent && entry_is_inherited(&entry, &self.own_session_id);
            self.turn_timestamp = entry.timestamp.unwrap_or_default();
        }
        for block in blocks {
            self.add_to_turn(block);
        }
    }

    /// A plan whose steps haven't changed is the same plan moving along: it updates in place,
    /// so a turn reads as one plan rather than one per step.
    fn add_to_turn(&mut self, block: MessageBlock) {
        if let MessageBlock::Plan { items } = &block {
            let same_plan = self.turn_blocks.iter_mut().find_map(|earlier| match earlier {
                MessageBlock::Plan { items: earlier_items } if same_steps(earlier_items, items) => Some(earlier_items),
                _ => None,
            });
            if let Some(earlier_items) = same_plan {
                earlier_items.clone_from(items);
                return;
            }
        }
        self.turn_blocks.push(block);
    }

    /// Close the assistant turn in progress. Returns true if a message was pushed.
    fn flush_turn(&mut self) -> bool {
        if self.turn_blocks.is_empty() {
            return false;
        }
        let blocks = std::mem::take(&mut self.turn_blocks);
        self.messages.push(ConversationMessage {
            role: "assistant".to_string(),
            text: searchable_text(&blocks),
            timestamp: std::mem::take(&mut self.turn_timestamp),
            blocks,
            inherited: self.turn_inherited,
            ..Default::default()
        });
        true
    }

    fn push_message(&mut self, mut message: ConversationMessage) {
        self.flush_turn();
        message.inherited = self.entry_inherited;
        self.messages.push(message);
    }

    fn push_event(&mut self, kind: &str, is_error: bool, text: String, detail: String, timestamp: String) {
        self.push_message(ConversationMessage {
            role: "system-event".to_string(),
            text,
            timestamp,
            event: Some(SystemEvent { kind: kind.to_string(), is_error, detail }),
            ..Default::default()
        });
    }

    fn push_notification(&mut self, notification: AgentNotification, timestamp: String) {
        self.push_task_notification(notification, None, timestamp);
    }

    fn push_task_notification(&mut self, notification: AgentNotification, task_id: Option<String>, timestamp: String) {
        // Keyed on the task and content: an agent that stopped, resumed and stopped again with
        // a new result is two real events, and so are two runs of a command that fail alike,
        // but one notification delivered twice is not.
        let dedup_key = format!(
            "{}\u{1}{}\u{1}{}\u{1}{}",
            task_id.unwrap_or_default(), notification.summary, notification.status, notification.result
        );
        if !self.seen_notifications.insert(dedup_key) {
            return;
        }
        self.push_message(ConversationMessage {
            role: "agent-notification".to_string(),
            text: format!("{}\n{}", notification.summary, notification.result),
            timestamp,
            notification: Some(notification),
            ..Default::default()
        });
    }

    fn process_user(&mut self, entry: JsonlEntry) {
        // Compaction summaries are special
        if entry.is_compact_summary.unwrap_or(false) {
            let text = clean_compaction_text(&extract_user_text(&entry.message));
            if !text.is_empty() {
                let (compaction_detail, boundary_time) = self.pending_compaction.take().unzip();
                // The summary can be written long after the compaction it describes.
                let timestamp = boundary_time.filter(|time| !time.is_empty()).or(entry.timestamp).unwrap_or_default();
                self.push_message(ConversationMessage {
                    role: "compaction".to_string(),
                    text,
                    timestamp,
                    compaction_detail: compaction_detail.flatten(),
                    ..Default::default()
                });
            } else {
                self.flush_turn();
            }
            return;
        }

        // A subagent's hand-back or another session's message. Stored as a meta user
        // entry, but it's often the most useful thing in the turn, so it gets a card.
        if let Some(notification) = entry.origin.as_ref().and_then(|origin| parse_peer_message(origin, &self.agent_names)) {
            self.push_notification(notification, entry.timestamp.unwrap_or_default());
            return;
        }

        // A subagent's first message is its whole brief, even when it's flagged meta.
        let is_agent_brief = self.is_subagent && self.messages.is_empty() && self.turn_blocks.is_empty();

        // Skip meta and tool-result entries
        if (entry.is_meta.unwrap_or(false) && !is_agent_brief) || is_tool_result_content(&entry.message) {
            return;
        }

        let text = extract_user_text(&entry.message);

        // Interrupt marker: this isn't a real user message — it's the synthetic record
        // of the user pressing Esc. It marks the reply that was in flight.
        if is_interrupt_marker(&text) {
            self.mark_interrupted(entry.timestamp.unwrap_or_default());
            return;
        }

        // Task notification: a background task finished. Claude Code injects this as a
        // `user`-role entry, but the user didn't type it, so it must not look like they did.
        if let Some(notification) = parse_task_notification(&text) {
            self.push_task_notification(notification, extract_tag_inner(&text, "task-id"), entry.timestamp.unwrap_or_default());
            return;
        }

        if let Some(run) = parse_shell_run(&text) {
            self.push_shell_run(run, entry.timestamp.unwrap_or_default());
            return;
        }

        // A local command's output usually comes as a system entry, but not always.
        if text.trim_start().starts_with("<local-command-std") {
            self.process_local_command(&text, entry.timestamp.unwrap_or_default());
            return;
        }

        let cleaned = strip_system_tags(&text);
        let images = extract_user_images(&entry.message, &cleaned);
        let documents = user_document_names(&entry.message);
        let timestamp = entry.timestamp.unwrap_or_default();
        if cleaned.is_empty() && images.is_empty() && documents.is_empty() {
            return;
        }
        if !cleaned.is_empty() || !images.is_empty() {
            let normalized = normalize_for_dedup(&cleaned);
            // A retried prompt is the same message sent again by the tool, not by the person.
            let is_retry = !normalized.is_empty()
                && entry.parent_uuid.as_ref().is_some_and(|parent| !self.seen_prompts.insert((parent.clone(), normalized.clone())));
            if !is_retry {
                self.remove_queued_copy(&normalized);
                self.recent_typed.push(normalized);
                self.push_message(ConversationMessage {
                    role: "user".to_string(),
                    text: cleaned,
                    timestamp: timestamp.clone(),
                    images,
                    ..Default::default()
                });
            }
        }
        for document in documents {
            self.push_attached_file(&document, timestamp.clone());
        }
    }

    /// The same message just shown as queued is now here as a normal turn; keep this one.
    fn remove_queued_copy(&mut self, normalized: &str) {
        let Some(recent_index) = self.recent_queued.iter().position(|queued| queued == normalized) else { return };
        self.recent_queued.remove(recent_index);
        self.flush_turn();
        let queued_copy = self.messages.iter().rposition(|message| {
            message.role == "user" && message.mid_turn && normalize_for_dedup(&message.text) == normalized
        });
        if let Some(message_index) = queued_copy {
            self.messages.remove(message_index);
        }
    }

    /// Tag the reply the user pressed Esc on. Its turn may already be closed, because a queued
    /// prompt or a card was written before the marker. If the person's own prompt comes first
    /// walking back, no reply had started, so the interruption gets its own row.
    fn mark_interrupted(&mut self, timestamp: String) {
        if self.flush_turn() {
            if let Some(last) = self.messages.last_mut() {
                last.interrupted = true;
            }
            return;
        }
        let interrupted_reply = self.messages.iter().rposition(|message| {
            message.role == "assistant" || message.role == "shell" || (message.role == "user" && !message.mid_turn)
        });
        match interrupted_reply {
            Some(index) if self.messages[index].role == "assistant" => self.messages[index].interrupted = true,
            _ => self.push_event("notice", false, "Interrupted before a reply".to_string(), String::new(), timestamp),
        }
    }

    /// A `!command` is written as one entry or as two (input, then output). Output with
    /// no command belongs to the run right before it.
    fn push_shell_run(&mut self, run: ShellRun, timestamp: String) {
        if run.command.is_empty() {
            if let Some(previous) = self.messages.last_mut().filter(|message| self.turn_blocks.is_empty() && message.role == "shell") {
                if let Some(previous_run) = previous.shell.as_mut() {
                    previous_run.stdout = run.stdout;
                    previous_run.stderr = run.stderr;
                    previous.text = shell_run_text(previous_run);
                    return;
                }
            }
        }
        self.push_message(ConversationMessage {
            role: "shell".to_string(),
            text: shell_run_text(&run),
            timestamp,
            shell: Some(run),
            ..Default::default()
        });
    }

    fn process_attachment(&mut self, entry: JsonlEntry) {
        let attachment = match &entry.attachment {
            Some(attachment) => attachment,
            None => return,
        };
        let timestamp = entry.timestamp.clone().unwrap_or_default();
        let attachment_type = attachment.get("type").and_then(|kind| kind.as_str()).unwrap_or("");
        match attachment_type {
            "queued_command" => self.process_queued_command(attachment, timestamp),
            "file" | "pdf_reference" | "opened_file_in_ide" => {
                let shown_path = attachment
                    .get("displayPath")
                    .or_else(|| attachment.get("filename"))
                    .and_then(|path| path.as_str())
                    .unwrap_or("");
                self.push_attached_file(shown_path, timestamp);
            }
            hook_type if hook_type.contains("hook") => self.process_hook(attachment, hook_type, timestamp),
            // The rest (deferred tools, environment, reminders, ...) is context Claude
            // Code feeds the model, not part of the conversation.
            _ => {}
        }
    }

    /// Consecutive attached files collapse into one row.
    fn push_attached_file(&mut self, shown_path: &str, timestamp: String) {
        if shown_path.is_empty() {
            return;
        }
        if self.turn_blocks.is_empty() {
            if let Some(last) = self.messages.last_mut() {
                if let Some(event) = last.event.as_mut().filter(|event| event.kind == "files") {
                    // A single file is named in the row itself; a list needs the details.
                    if event.detail.is_empty() {
                        event.detail = last.text.trim_start_matches("Attached ").to_string();
                    }
                    event.detail.push('\n');
                    event.detail.push_str(shown_path);
                    last.text = format!("Attached {} files", event.detail.lines().count());
                    return;
                }
            }
        }
        self.push_event("files", false, format!("Attached {}", shown_path), String::new(), timestamp);
    }

    fn process_hook(&mut self, attachment: &Value, hook_type: &str, timestamp: String) {
        let text_field = |key: &str| attachment.get(key).and_then(|value| value.as_str()).unwrap_or("").trim();
        let hook_name = [text_field("hookName"), text_field("hookEvent"), "hook"]
            .into_iter()
            .find(|name| !name.is_empty())
            .unwrap_or("hook");
        let exit_code = attachment.get("exitCode").and_then(|code| code.as_i64()).unwrap_or(0);
        let failed = exit_code != 0 || is_failed_hook_type(hook_type);
        let content = match attachment.get("content") {
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|part| part.as_str())
                .collect::<Vec<_>>()
                .join("\n\n"),
            _ => text_field("content").to_string(),
        };
        let detail = [content.as_str(), text_field("blockingError"), text_field("stderr"), text_field("stdout"), text_field("message")]
            .into_iter()
            .find(|text| !text.trim().is_empty())
            .map(strip_ansi)
            .unwrap_or_default();
        if !failed && (detail.is_empty() || hook_type == "async_hook_response") {
            return;
        }
        let dedup_key = format!("{}\u{1}{}", text_field("hookEvent"), detail);
        if !self.seen_hook_output.insert(dedup_key) {
            return;
        }
        let text = if failed {
            format!("{} hook failed{}", hook_name, if exit_code != 0 { format!(" (exit {})", exit_code) } else { String::new() })
        } else if hook_type == "hook_additional_context" {
            format!("{} hook added context", hook_name)
        } else {
            format!("{} hook output", hook_name)
        };
        self.push_hook(text, detail, failed, timestamp);
    }

    /// During a turn, hook output stays inside it. Between turns, consecutive quiet hook
    /// rows merge into one; failures keep their own row so they stay visible.
    fn push_hook(&mut self, text: String, detail: String, is_error: bool, timestamp: String) {
        if !self.turn_blocks.is_empty() {
            self.turn_blocks.push(MessageBlock::Hook { label: text, text: detail, is_error });
            return;
        }
        if !is_error {
            if let Some(last) = self.messages.last_mut() {
                let merged_count = self.merged_hook_rows.max(1) + 1;
                if let Some(event) = last.event.as_mut().filter(|event| event.kind == "hook" && !event.is_error) {
                    let first_label = if self.merged_hook_rows == 0 { format!("── {} ──\n", last.text) } else { String::new() };
                    event.detail = format!("{}{}\n\n── {} ──\n{}", first_label, event.detail, text, detail);
                    last.text = format!("{} hook events", merged_count);
                    self.merged_hook_rows = merged_count;
                    return;
                }
            }
        }
        self.merged_hook_rows = 0;
        self.push_event("hook", is_error, text, detail, timestamp);
    }

    /// Surface a `queued_command` attachment — a message the user queued while Claude
    /// was still working. Tagged as mid-turn, since it was sent mid-response. Skipped
    /// when the same message also exists as a normal user turn. A queued entry can also
    /// be a task notification or a peer's message, which become cards instead.
    fn process_queued_command(&mut self, attachment: &Value, timestamp: String) {
        if let Some(notification) = attachment.get("origin").and_then(|origin| parse_peer_message(origin, &self.agent_names)) {
            self.push_notification(notification, timestamp);
            return;
        }
        // `prompt` is usually a bare string, but can be a [{type:text/image}] array
        // when the queued message included pasted images.
        let (text, image_blocks) = match attachment.get("prompt") {
            Some(Value::String(prompt_text)) => (prompt_text.clone(), Vec::new()),
            Some(Value::Array(parts)) => {
                let text = parts
                    .iter()
                    .filter(|part| part.get("type").and_then(|kind| kind.as_str()) == Some("text"))
                    .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");
                let image_blocks = parts
                    .iter()
                    .filter(|part| part.get("type").and_then(|kind| kind.as_str()) == Some("image"))
                    .collect();
                (text, image_blocks)
            }
            _ => return,
        };
        if let Some(notification) = parse_task_notification(&text) {
            self.push_task_notification(notification, extract_tag_inner(&text, "task-id"), timestamp);
            return;
        }
        if let Some(run) = parse_shell_run(&text) {
            self.push_shell_run(run, timestamp);
            return;
        }
        let cleaned = strip_system_tags(&text);
        let images = pair_images_with_references(&image_blocks, &cleaned);
        if cleaned.is_empty() && images.is_empty() {
            return;
        }
        // Just shown as a normal user turn — don't duplicate it. The same words later on
        // are a new message, though.
        let normalized = normalize_for_dedup(&cleaned);
        // Images have no words to compare, so two of them are never taken for one message.
        if !normalized.is_empty() {
            if self.recent_typed.contains(&normalized) {
                return;
            }
            self.recent_queued.push(normalized);
        }
        self.push_message(ConversationMessage {
            role: "user".to_string(),
            text: cleaned,
            timestamp,
            images,
            mid_turn: true,
            ..Default::default()
        });
    }

    fn process_system(&mut self, entry: JsonlEntry) {
        let timestamp = entry.timestamp.clone().unwrap_or_default();
        let content = entry.content.as_ref().and_then(|content| content.as_str()).unwrap_or("").trim();
        match entry.subtype.as_deref() {
            // Rolled up into the Stats panel.
            Some("turn_duration") => {}
            Some("compact_boundary") => {
                self.pending_compaction = Some((entry.compact_metadata.as_ref().map(describe_compaction), timestamp));
            }
            Some("stop_hook_summary") => {
                let errors = entry.hook_errors.as_ref().and_then(|errors| errors.as_array()).cloned().unwrap_or_default();
                let prevented = entry.prevented_continuation.unwrap_or(false);
                if errors.is_empty() && !prevented {
                    return;
                }
                let detail = errors
                    .iter()
                    .map(|error| error.as_str().map(String::from).unwrap_or_else(|| error.to_string()))
                    .collect::<Vec<_>>()
                    .join("\n");
                let text = if prevented {
                    let reason = entry.stop_reason.clone().unwrap_or_default();
                    format!("Stop hook kept Claude working{}", if reason.is_empty() { String::new() } else { format!(": {}", reason) })
                } else {
                    "Stop hook failed".to_string()
                };
                self.push_hook(text, detail, !prevented, timestamp);
            }
            Some("api_error") => {
                let formatted = entry
                    .error
                    .as_ref()
                    .and_then(|error| error.get("formatted").or_else(|| error.get("message")))
                    .and_then(|message| message.as_str())
                    .unwrap_or("API error");
                let attempt = match (entry.retry_attempt, entry.max_retries) {
                    (Some(attempt), Some(maximum)) => format!(" (retry {}/{})", attempt, maximum),
                    _ => String::new(),
                };
                let text = format!("{}{}", formatted, attempt);
                // Retries of the same failure collapse into one row showing the latest attempt.
                if self.turn_blocks.is_empty() {
                    if let Some(last) = self.messages.last_mut() {
                        let is_same_failure = last.event.as_ref().is_some_and(|event| event.kind == "api-error")
                            && last.text.starts_with(formatted);
                        if is_same_failure {
                            last.text = text;
                            return;
                        }
                    }
                }
                self.push_event("api-error", true, text, String::new(), timestamp);
            }
            Some("away_summary") => {
                self.push_event("recap", false, content.to_string(), String::new(), timestamp);
            }
            Some("local_command") => self.process_local_command(content, timestamp),
            // Unknown or purely informational subtypes still show up, so whatever Claude
            // Code adds next is visible rather than silently lost.
            _ => {
                if !content.is_empty() && !entry.is_meta.unwrap_or(false) {
                    self.push_event("notice", false, strip_ansi(content), String::new(), timestamp);
                }
            }
        }
    }

    /// `/command` echoes and their output arrive as two system entries; pair them.
    fn process_local_command(&mut self, content: &str, timestamp: String) {
        if let Some(output) = extract_tag_inner(content, "local-command-stdout")
            .or_else(|| extract_tag_inner(content, "local-command-stderr"))
        {
            let output = strip_ansi(output.trim());
            if output == "(no content)" {
                return;
            }
            if self.turn_blocks.is_empty() {
                if let Some(last) = self.messages.last_mut() {
                    if let Some(event) = last.event.as_mut().filter(|event| event.kind == "command" && event.detail.is_empty()) {
                        event.detail = output;
                        return;
                    }
                }
            }
            if !output.is_empty() {
                self.push_event("command", false, output, String::new(), timestamp);
            }
            return;
        }
        let command = strip_system_tags(content);
        if !command.is_empty() {
            self.push_event("command", false, command, String::new(), timestamp);
        }
    }
}

fn same_steps(first: &[PlanItem], second: &[PlanItem]) -> bool {
    first.len() == second.len() && first.iter().zip(second).all(|(one, other)| one.content == other.content)
}

/// Compaction summaries carry machine tags: injected reminders, an empty artifact marker, and
/// sometimes the close of a reminder that opened before the summary began.
fn clean_compaction_text(text: &str) -> String {
    const MARKERS: &[&str] = &["<artifact-content-authored-by-others/>", "</artifact-content-authored-by-others>"];
    let mut cleaned = strip_paired_tags(text, &["tools-used-before-summary", "system-reminder"], false);
    for marker in MARKERS {
        cleaned = cleaned.replace(marker, "");
    }
    let trimmed = cleaned.trim_start();
    trimmed.strip_prefix("</system-reminder>").unwrap_or(trimmed).trim().to_string()
}

/// Add `incoming`'s content blocks to `pending`, skipping any it already has.
fn merge_content_blocks(pending: &mut JsonlEntry, incoming: JsonlEntry) {
    let incoming_blocks = match incoming.message.and_then(|message| message.content) {
        Some(Value::Array(blocks)) => blocks,
        _ => return,
    };
    let pending_content = pending.message.as_mut().and_then(|message| message.content.as_mut());
    if let Some(Value::Array(pending_blocks)) = pending_content {
        for block in incoming_blocks {
            if !pending_blocks.contains(&block) {
                pending_blocks.push(block);
            }
        }
    }
}

/// The text of an assistant turn that in-session search should match.
fn searchable_text(blocks: &[MessageBlock]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for block in blocks {
        match block {
            MessageBlock::Text { text } | MessageBlock::Thinking { text } => parts.push(text.clone()),
            MessageBlock::Tool { name, summary, .. } => parts.push(format!("{} {}", name, summary)),
            MessageBlock::Plan { items } => {
                parts.extend(items.iter().map(|item| item.content.clone()));
            }
            MessageBlock::Note { label, text } => parts.push(format!("{} {}", label, text)),
            MessageBlock::Hook { label, .. } => parts.push(label.clone()),
        }
    }
    parts.join("\n\n")
}

fn describe_compaction(metadata: &Value) -> String {
    let tokens = |key: &str| metadata.get(key).and_then(|value| value.as_u64());
    let trigger = metadata.get("trigger").and_then(|value| value.as_str()).unwrap_or("manual");
    match (tokens("preTokens"), tokens("postTokens")) {
        (Some(before), Some(after)) => format!(
            "{} · {:.1}k → {:.1}k tokens",
            trigger,
            before as f64 / 1000.0,
            after as f64 / 1000.0
        ),
        _ => trigger.to_string(),
    }
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

/// Normalize message text for dedup (collapse whitespace + lowercase).
fn normalize_for_dedup(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// True for a user entry the person actually typed: not meta, a tool result, a
/// compaction summary, an interrupt marker, a task notification or a shell echo.
fn is_user_prompt(entry: &JsonlEntry) -> bool {
    if entry.is_meta.unwrap_or(false)
        || entry.is_compact_summary.unwrap_or(false)
        || is_tool_result_content(&entry.message)
    {
        return false;
    }
    let text = extract_user_text(&entry.message);
    if is_interrupt_marker(&text) || parse_task_notification(&text).is_some() || parse_shell_run(&text).is_some() {
        return false;
    }
    !strip_system_tags(&text).is_empty() || !extract_user_images(&entry.message, "").is_empty()
}

fn is_peer_origin(origin: &Value) -> bool {
    origin.get("kind").and_then(|kind| kind.as_str()) == Some("peer")
}

/// The text of a queued message the person typed, or None for a queued notification,
/// peer message or shell echo (those aren't prompts).
fn queued_prompt_text(attachment: &Value) -> Option<String> {
    if attachment.get("type").and_then(|kind| kind.as_str()) != Some("queued_command")
        || attachment.get("origin").is_some_and(is_peer_origin)
    {
        return None;
    }
    let text = match attachment.get("prompt") {
        Some(Value::String(prompt)) => prompt.clone(),
        Some(Value::Array(parts)) => {
            let has_image = parts.iter().any(|part| part.get("type").and_then(|kind| kind.as_str()) == Some("image"));
            let text = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            // A pasted image with no words is still something the person sent.
            if text.trim().is_empty() && has_image {
                return Some("[image]".to_string());
            }
            text
        }
        _ => return None,
    };
    if parse_task_notification(&text).is_some() || parse_shell_run(&text).is_some() {
        return None;
    }
    Some(strip_system_tags(&text)).filter(|cleaned| !cleaned.is_empty())
}

/// Counts what the person typed, by the same rules the conversation shows it. A queued message
/// stored as both an attachment and a normal turn counts once; a local command like `/clear`
/// went to the tool rather than the agent, and a retry is the same prompt again.
#[derive(Default)]
struct PromptCounter {
    own_session_id: String,
    own: u64,
    inherited: u64,
    recent_typed: Vec<String>,
    recent_queued: Vec<String>,
    seen_prompts: std::collections::HashSet<(String, String)>,
    after_local_command_notice: bool,
}

impl PromptCounter {
    fn for_session(own_session_id: &str) -> Self {
        PromptCounter { own_session_id: own_session_id.to_string(), ..Default::default() }
    }

    fn add_assistant_output(&mut self) {
        self.recent_typed.clear();
        self.recent_queued.clear();
    }

    fn add_user_entry(&mut self, entry: &JsonlEntry) {
        let text = extract_user_text(&entry.message);
        if entry.is_meta.unwrap_or(false) {
            self.after_local_command_notice = text.contains("<local-command-caveat>");
            return;
        }
        let is_local_command = std::mem::take(&mut self.after_local_command_notice) && text.trim_start().starts_with("<command-");
        if is_local_command || !is_user_prompt(entry) {
            return;
        }
        let normalized = normalize_for_dedup(&strip_system_tags(&text));
        let is_retry = !normalized.is_empty()
            && entry.parent_uuid.as_ref().is_some_and(|parent| !self.seen_prompts.insert((parent.clone(), normalized.clone())));
        if is_retry {
            return;
        }
        if let Some(index) = self.recent_queued.iter().position(|queued| *queued == normalized) {
            // Counted when it was queued.
            self.recent_queued.remove(index);
        } else {
            self.count(entry_is_inherited(entry, &self.own_session_id));
        }
        self.recent_typed.push(normalized);
    }

    /// Takes any line that might be a queued message; ignores the rest.
    fn add_queued_line(&mut self, line: &str) {
        if !line.contains("\"queued_command\"") || !line.contains("\"type\":\"attachment\"") {
            return;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => return,
        };
        if value.get("isSidechain").and_then(|flag| flag.as_bool()).unwrap_or(false) {
            return;
        }
        let Some(text) = value.get("attachment").and_then(queued_prompt_text) else { return };
        let normalized = normalize_for_dedup(&text);
        if !self.recent_typed.contains(&normalized) {
            self.count(value_is_inherited(&value, &self.own_session_id));
            self.recent_queued.push(normalized);
        }
    }

    fn count(&mut self, inherited: bool) {
        if inherited {
            self.inherited += 1;
        } else {
            self.own += 1;
        }
    }
}

/// Pair image blocks with `[Image #N]` references, in order of appearance. Extras
/// (blocks with no matching reference) are numbered after the highest referenced
/// number, so they can't collide.
fn pair_images_with_references(image_blocks: &[&Value], text: &str) -> Vec<MessageImage> {
    if image_blocks.is_empty() {
        return Vec::new();
    }
    let mut references: Vec<u32> = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find("[Image #") {
        let start = cursor + found + "[Image #".len();
        match text[start..].find(']') {
            Some(end_offset) => {
                if let Ok(number) = text[start..start + end_offset].parse::<u32>() {
                    references.push(number);
                }
                cursor = start + end_offset + 1;
            }
            None => break,
        }
    }
    let extras_base = references.iter().copied().max().unwrap_or(0);
    let mut images = Vec::new();
    for (image_index, block) in image_blocks.iter().enumerate() {
        let data_url = match image_data_url(block) {
            Some(data_url) => data_url,
            None => continue,
        };
        let number = references
            .get(image_index)
            .copied()
            .unwrap_or_else(|| extras_base + (image_index - references.len().min(image_index) + 1) as u32);
        images.push(MessageImage { number, data_url });
    }
    images
}

/// The names of documents pasted into a user message.
fn user_document_names(message: &Option<JsonlMessage>) -> Vec<String> {
    let Some(Value::Array(blocks)) = message.as_ref().and_then(|message| message.content.as_ref()) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(|kind| kind.as_str()) == Some("document"))
        .map(|block| block.get("title").and_then(|title| title.as_str()).unwrap_or("a document").to_string())
        .collect()
}

/// Extract image content blocks from a user message.
fn extract_user_images(message: &Option<JsonlMessage>, text: &str) -> Vec<MessageImage> {
    let blocks = match message.as_ref().and_then(|msg| msg.content.as_ref()) {
        Some(Value::Array(blocks)) => blocks,
        _ => return Vec::new(),
    };
    let image_blocks: Vec<&Value> = blocks
        .iter()
        .filter(|block| block.get("type").and_then(|block_type| block_type.as_str()) == Some("image"))
        .collect();
    pair_images_with_references(&image_blocks, text)
}

/// True when a user entry holds tool results and nothing else — a tool response, not
/// real input. A forked agent's task arrives as text *alongside* the parent's tool
/// result, and that text must still be shown.
fn is_tool_result_content(message: &Option<JsonlMessage>) -> bool {
    let blocks = match message.as_ref().and_then(|msg| msg.content.as_ref()) {
        Some(Value::Array(blocks)) => blocks,
        _ => return false,
    };
    let block_type = |block: &Value| block.get("type").and_then(|kind| kind.as_str()).map(String::from);
    blocks.iter().any(|block| block_type(block).as_deref() == Some("tool_result"))
        && !blocks.iter().any(|block| {
            block_type(block).as_deref() == Some("text")
                && block.get("text").and_then(|text| text.as_str()).is_some_and(|text| !text.trim().is_empty())
        })
}

/// Tags whose entire block is noise and should be removed wholesale.
const DROP_TAGS: &[&str] = &[
    "system-reminder",
    "browser_instruction",
    "fork-boilerplate",
    "local-command-caveat",
    "local-command-stdout",
    "local-command-stderr",
];

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

    // Tags whose wrapper is removed but inner text kept. command-args is included as
    // a safety net so any malformed command entry still keeps the user's content.
    const UNWRAP_TAGS: &[&str] = &["command-name", "command-message", "command-args"];

    let mut result = strip_paired_tags(text, DROP_TAGS, false);
    result = strip_paired_tags(&result, UNWRAP_TAGS, true);
    result = unwrap_pasted_content(&result);
    strip_ansi(&result).trim().to_string()
}

/// Claude Code wraps pasted text as `<pasted_content id="x">…</pasted_content id="x">`.
/// Keep the text, drop the wrapper. An unclosed tag is left as literal text.
fn unwrap_pasted_content(text: &str) -> String {
    const OPEN: &str = "<pasted_content";
    const CLOSE: &str = "</pasted_content";
    let mut output = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(open_start) = remaining.find(OPEN) {
        let after_open = &remaining[open_start..];
        let inner_start = after_open.find('>').map(|offset| offset + 1);
        let close_start = after_open.find(CLOSE);
        let close_end = close_start.and_then(|start| {
            after_open[start..].find('>').map(|offset| start + offset + 1)
        });
        match (inner_start, close_start, close_end) {
            (Some(inner_start), Some(close_start), Some(close_end)) if inner_start <= close_start => {
                output.push_str(&remaining[..open_start]);
                output.push_str(after_open[inner_start..close_start].trim_matches('\n'));
                remaining = &after_open[close_end..];
            }
            _ => break,
        }
    }
    output.push_str(remaining);
    output
}

/// If `text` is a slash-command invocation, rebuild it as "<name> <args>". Only a
/// message that starts with the command tags counts — a prompt that merely mentions
/// them (pasted logs, code) must keep its content. Returns None otherwise.
fn reconstruct_slash_command(text: &str) -> Option<String> {
    if !text.trim_start().starts_with("<command-") {
        return None;
    }
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

/// Parse a `!command` entry: `<bash-input>`, `<bash-stdout>` and `<bash-stderr>`, in
/// one entry or split across two. Returns None for anything else.
fn parse_shell_run(text: &str) -> Option<ShellRun> {
    let without_noise = strip_paired_tags(text, DROP_TAGS, false);
    let trimmed = without_noise.trim_start();
    if !trimmed.starts_with("<bash-input>") && !trimmed.starts_with("<bash-stdout>") && !trimmed.starts_with("<bash-stderr>") {
        return None;
    }
    let field = |tag: &str| {
        let raw = extract_tag_inner(trimmed, tag).unwrap_or_default();
        decode_xml_entities(strip_ansi(&raw).trim())
    };
    let stdout = field("bash-stdout");
    Some(ShellRun {
        command: field("bash-input"),
        stdout: if stdout == "(Bash completed with no output)" { String::new() } else { stdout },
        stderr: field("bash-stderr"),
    })
}

fn shell_run_text(run: &ShellRun) -> String {
    format!("$ {}\n{}\n{}", run.command, run.stdout, run.stderr)
}

/// Parse a `<task-notification>` transcript entry into an `AgentNotification`.
/// Returns None when `text` isn't one. A single wrapper covers all variants: single
/// subagents (usage = subagent_tokens/tool_uses/duration_ms), background shells,
/// Monitor events (`<event>`), dynamic workflows, and multi-agent fan-outs
/// (agent_count/agents_done/agents_error).
fn parse_task_notification(text: &str) -> Option<AgentNotification> {
    if !text.trim_start().starts_with("<task-notification>") {
        return None;
    }
    let parse_u64 =
        |tag: &str| extract_tag_inner(text, tag).and_then(|value| value.trim().parse::<u64>().ok());
    let decoded = |tag: &str| {
        extract_tag_inner(text, tag).map(|body| decode_xml_entities(strip_ansi(&body).trim()))
    };
    let summary = extract_tag_inner(text, "summary").unwrap_or_default().trim().to_string();
    let event = decoded("event");
    let tokens = parse_u64("subagent_tokens");
    let agent_count = parse_u64("agent_count");
    let kind = if event.is_some() {
        "monitor"
    } else if tokens.is_some() || agent_count.is_some() || summary.starts_with("Agent") {
        "agent"
    } else if summary.starts_with("Background") {
        "shell"
    } else {
        "task"
    };
    let result = [decoded("result"), event.clone(), decoded("note")]
        .into_iter()
        .flatten()
        .filter(|body| !body.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let status = extract_tag_inner(text, "status").unwrap_or_default().trim().to_string();
    Some(AgentNotification {
        summary,
        status: if status.is_empty() && event.is_some() { "event".to_string() } else { status },
        kind: kind.to_string(),
        result,
        tokens,
        tool_uses: parse_u64("tool_uses"),
        duration_ms: parse_u64("duration_ms"),
        agent_count,
        agents_done: parse_u64("agents_done"),
        agents_error: parse_u64("agents_error"),
    })
}

/// A message from a subagent or another session, from a user entry's `origin`. Returns
/// None for any other origin. A hand-back carries the agent's clean report separately.
fn parse_peer_message(origin: &Value, agent_names: &std::collections::HashMap<String, String>) -> Option<AgentNotification> {
    if !is_peer_origin(origin) {
        return None;
    }
    let field = |key: &str| origin.get(key).and_then(|value| value.as_str()).unwrap_or("");
    let is_hand_back = origin.get("handback").and_then(|flag| flag.as_bool()).unwrap_or(false);
    // A hand-back's `from` is the agent's raw id, which means nothing on its own.
    let sender = [field("name"), agent_names.get(field("from")).map_or("", String::as_str), if field("senderTaskId").is_empty() { field("from") } else { "" }]
        .into_iter()
        .find(|name| !name.is_empty());
    let report = field("handbackReportText");
    let result = if report.is_empty() { strip_hand_back_frame(field("body")) } else { report.to_string() };
    Some(AgentNotification {
        summary: match (is_hand_back, sender) {
            (true, Some(sender)) => format!("\"{}\" reported back", sender),
            (true, None) => "An agent reported back".to_string(),
            (false, Some(sender)) => format!("Message from \"{}\"", sender),
            (false, None) => "Message from another session".to_string(),
        },
        status: if is_hand_back { "completed" } else { "message" }.to_string(),
        kind: if is_hand_back { "hand-back" } else { "message" }.to_string(),
        result: decode_xml_entities(result.trim()),
        ..Default::default()
    })
}

/// Claude Code wraps a report as `[Subagent hand-back] <advice to the model> The report
/// follows:` and indents it. Only the report is for reading.
fn strip_hand_back_frame(body: &str) -> String {
    const FRAME_START: &str = "[Subagent hand-back]";
    const FRAME_END: &str = "The report follows:";
    let report = body.strip_prefix(FRAME_START).and_then(|framed| framed.split_once(FRAME_END)).map(|(_, report)| report);
    match report {
        Some(report) => report.lines().map(|line| line.strip_prefix("  ").unwrap_or(line)).collect::<Vec<_>>().join("\n"),
        None => body.to_string(),
    }
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

/// Strip terminal escape sequences (colours, cursor moves, OSC hyperlinks and titles) and
/// control characters. NUL stays: it's how an export recognises binary output.
fn strip_ansi(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            match characters.peek().copied() {
                Some('[') => {
                    characters.next();
                    for sequence_character in characters.by_ref() {
                        if sequence_character.is_ascii_alphabetic() || sequence_character == '~' {
                            break;
                        }
                    }
                }
                // String sequences end at BEL or ESC \\. A broken one stops at the line's end,
                // so it can't swallow the output after it.
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    characters.next();
                    while let Some(sequence_character) = characters.peek().copied() {
                        if sequence_character == '\n' {
                            break;
                        }
                        characters.next();
                        if sequence_character == '\u{7}' {
                            break;
                        }
                        if sequence_character == '\u{1b}' {
                            if characters.peek() == Some(&'\\') {
                                characters.next();
                            }
                            break;
                        }
                    }
                }
                // Short sequences like `ESC 7` and `ESC ( B`. Anything else after a lone ESC is text.
                Some(' '..='/') => {
                    while characters.next_if(|next| matches!(next, ' '..='/')).is_some() {}
                    characters.next();
                }
                Some('0'..='~') => {
                    characters.next();
                }
                _ => {}
            }
        } else if !character.is_ascii_control() || matches!(character, '\n' | '\t' | '\r' | '\0') {
            output.push(character);
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
/// A last resort: `recorded_project_path` is right whenever a transcript has a cwd.
/// `-Users-alex-Documents-my-project` could be `/Users/alex/Documents/my-project`
/// or `/Users/alex/Documents/my/project` — project names with dashes are ambiguous.
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

/// Turn an assistant entry's raw content blocks into message blocks. Thinking blocks
/// with no text (signature only) carry nothing to show and are skipped.
fn assistant_blocks(
    content: Option<&Value>,
    tool_to_agent: &std::collections::HashMap<String, String>,
    current_plan: &mut Vec<PlanItem>,
) -> Vec<MessageBlock> {
    let raw_blocks = match content {
        Some(Value::Array(raw_blocks)) => raw_blocks,
        _ => return Vec::new(),
    };
    let mut blocks = Vec::new();
    for raw_block in raw_blocks {
        let text_of = |key: &str| raw_block.get(key).and_then(|text| text.as_str()).unwrap_or("");
        match text_of("type") {
            "text" if !text_of("text").trim().is_empty() => {
                blocks.push(MessageBlock::Text { text: text_of("text").to_string() });
            }
            "thinking" if !text_of("thinking").trim().is_empty() => {
                blocks.push(MessageBlock::Thinking { text: text_of("thinking").to_string() });
            }
            "tool_use" | "server_tool_use" | "mcp_tool_use" => {
                blocks.extend(tool_block(raw_block, tool_to_agent, current_plan));
            }
            _ => {}
        }
    }
    blocks
}

/// Longest tool input carried per call; `get_tool_input` has the rest.
const TOOL_INPUT_LIMIT: usize = 6000;
const TOOL_SUMMARY_LIMIT: usize = 200;

fn tool_block(
    block: &Value,
    tool_to_agent: &std::collections::HashMap<String, String>,
    current_plan: &mut Vec<PlanItem>,
) -> Option<MessageBlock> {
    let tool_name = block.get("name").and_then(|name| name.as_str())?;
    let input = block.get("input")?;
    let text_input = |key: &str| input.get(key).and_then(|value| value.as_str()).unwrap_or("");

    // Tools that are really a piece of the conversation rather than an action.
    match tool_name {
        "mcp__ccd_turn__update_plan" | "TodoWrite" => {
            let key = if tool_name == "TodoWrite" { "todos" } else { "items" };
            // If this tool's shape ever changes, it falls through to an ordinary pill.
            if let Some(items) = input.get(key).and_then(|items| items.as_array()) {
                *current_plan = items
                    .iter()
                    .map(|item| PlanItem {
                        content: item.get("content").and_then(|text| text.as_str()).unwrap_or("").to_string(),
                        status: item.get("status").and_then(|text| text.as_str()).unwrap_or("pending").to_string(),
                    })
                    .collect();
                return Some(MessageBlock::Plan { items: current_plan.clone() });
            }
            if apply_plan_update(current_plan, input) {
                return Some(MessageBlock::Plan { items: current_plan.clone() });
            }
        }
        "mcp__ccd_turn__wrap_up" if !text_input("recap").is_empty() => {
            return Some(MessageBlock::Note { label: "Recap".to_string(), text: text_input("recap").to_string() });
        }
        "mcp__ccd_session__mark_chapter" if !text_input("title").is_empty() => {
            return Some(MessageBlock::Note {
                label: format!("Chapter: {}", text_input("title")),
                text: text_input("summary").to_string(),
            });
        }
        _ => {}
    }

    let summary: String = tool_summary(tool_name, input).chars().take(TOOL_SUMMARY_LIMIT).collect();
    let tool_use_id = block.get("id").and_then(|val| val.as_str()).map(String::from);
    let agent_id = tool_use_id.as_ref().filter(|_| matches!(tool_name, "Agent" | "agent" | "Task" | "Workflow")).and_then(|id| tool_to_agent.get(id)).cloned();

    let full_input = full_tool_input(input);
    let total_chars = full_input.chars().count();
    // The pill ellipsizes its summary, so anything but a short input needs showing in full.
    const FULLY_VISIBLE_SUMMARY: usize = 60;
    let summary_shows_everything = full_input == summary && total_chars <= FULLY_VISIBLE_SUMMARY;
    let input_hidden = summary_shows_everything || matches!(tool_name, "Read" | "read");
    let input_truncated = !input_hidden && total_chars > TOOL_INPUT_LIMIT;
    let input_text = if input_hidden {
        String::new()
    } else if input_truncated {
        let head: String = full_input.chars().take(TOOL_INPUT_LIMIT).collect();
        format!("{}\n…[{} more characters]", head, total_chars - TOOL_INPUT_LIMIT)
    } else {
        full_input
    };

    Some(MessageBlock::Tool {
        name: tool_name.to_string(),
        summary,
        input: input_text,
        input_truncated,
        tool_use_id,
        agent_id,
    })
}

/// What a tool call's input panel shows, before any cut.
fn full_tool_input(input: &Value) -> String {
    match input.get("command").and_then(|value| value.as_str()) {
        Some(command) => command.to_string(),
        None => serde_json::to_string_pretty(input).unwrap_or_default(),
    }
}

/// `mcp__ccd_turn__update_plan` sends the steps once, then only what moved: `advance`, or
/// `done` and `start` with 1-based step numbers. False if this isn't an update to a known plan.
fn apply_plan_update(plan: &mut [PlanItem], input: &Value) -> bool {
    let is_advance = input.get("advance").and_then(|flag| flag.as_bool()) == Some(true);
    let done_steps = input.get("done").and_then(|steps| steps.as_array());
    let start_step = input.get("start").and_then(|step| step.as_u64());
    if plan.is_empty() || (!is_advance && done_steps.is_none() && start_step.is_none()) {
        return false;
    }
    let step_at = |number: u64| (number as usize).checked_sub(1);
    for number in done_steps.into_iter().flatten().filter_map(|step| step.as_u64()) {
        if let Some(step) = step_at(number).and_then(|index| plan.get_mut(index)) {
            step.status = "completed".to_string();
        }
    }
    if is_advance {
        let running = plan.iter().position(|step| step.status == "in_progress");
        if let Some(index) = running {
            plan[index].status = "completed".to_string();
        }
        if let Some(next) = plan.iter_mut().find(|step| step.status == "pending") {
            next.status = "in_progress".to_string();
        }
    }
    if let Some(index) = start_step.and_then(step_at).filter(|index| *index < plan.len()) {
        // One step runs at a time.
        for step in plan.iter_mut().filter(|step| step.status == "in_progress") {
            step.status = "pending".to_string();
        }
        plan[index].status = "in_progress".to_string();
    }
    true
}

/// A one-line description of a tool call. New and MCP tools are handled by the generic
/// key list, so a tool we've never heard of still gets a useful line.
fn tool_summary(tool_name: &str, input: &Value) -> String {
    let text_input = |key: &str| input.get(key).and_then(|value| value.as_str()).unwrap_or("");
    match tool_name {
        "Grep" | "grep" => {
            let path = text_input("path");
            return format!("{} in {}", text_input("pattern"), if path.is_empty() { "." } else { path });
        }
        "Workflow" => return extract_workflow_label(input),
        "TaskUpdate" if text_input("subject").is_empty() => {
            return format!("#{} {}", text_input("taskId"), text_input("status")).trim().to_string();
        }
        "AskUserQuestion" => {
            // New shape: { questions: [{ question, ... }] }; older shape: { question }.
            return input
                .get("questions")
                .and_then(|val| val.as_array())
                .and_then(|questions| questions.first())
                .and_then(|first| first.get("question"))
                .and_then(|val| val.as_str())
                .unwrap_or_else(|| text_input("question"))
                .to_string();
        }
        "SendUserFile" => {
            let files: Vec<&str> = input
                .get("files")
                .and_then(|files| files.as_array())
                .into_iter()
                .flatten()
                .filter_map(|file| file.as_str())
                .map(|file| file.rsplit('/').next().unwrap_or(file))
                .collect();
            let caption = text_input("caption");
            return if caption.is_empty() { files.join(", ") } else { format!("{} ({})", caption, files.join(", ")) };
        }
        _ => {}
    }
    // Browser batches: the action names say more than the first url would.
    if let Some(actions) = input.get("actions").and_then(|actions| actions.as_array()) {
        let names: Vec<&str> = actions.iter().filter_map(|action| action.get("name")?.as_str()).collect();
        if !names.is_empty() {
            return names.join(", ");
        }
    }
    const SUMMARY_KEYS: &[&str] = &[
        "file_path", "notebook_path", "command", "pattern", "query", "url", "sql", "skill",
        "subject", "reason", "task_id", "description", "title", "name", "path", "prompt",
        "action", "text", "selector", "to",
    ];
    for key in SUMMARY_KEYS {
        let value = text_input(key);
        if !value.is_empty() {
            return value.to_string();
        }
    }
    // Anything else: the first string field, whatever it's called.
    if let Some(text) = input.as_object().and_then(|fields| fields.values().find_map(|value| value.as_str())) {
        return text.to_string();
    }
    // Only numbers and flags, like {"tabId": 42}: show them as they are.
    let fields: Vec<String> = input
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, value)| format!("{}: {}", key, value))
        .collect();
    fields.join(", ")
}

/// Pick the best human label for a Workflow tool call. Inline scripts start with
/// `export const meta = { name: '...' }`, so prefer that name; otherwise fall back
/// to the workflow's name, description, the script file's basename, or a generic label.
fn extract_workflow_label(input: &Value) -> String {
    if let Some(script) = input.get("script").and_then(|val| val.as_str()) {
        if let Some(name) = extract_meta_name(script) {
            return name;
        }
    }
    for key in ["name", "description"] {
        if let Some(label) = input.get(key).and_then(|val| val.as_str()) {
            if !label.is_empty() {
                return label.to_string();
            }
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

/// What the list knows about every Claude Code transcript, kept across launches.
static CLAUDE_INDEX: FileIndex<SessionQuickMetadata> = FileIndex::new("claude-index");

/// Facts worked out from a file, remembered until the file changes, including across launches.
///
/// Reading every transcript on each launch means going through gigabytes that almost never
/// changed. Checking a file's date and size takes microseconds, so the list opens from this
/// and only re-reads what's different. It can't go stale the way a saved list could, because
/// each entry is checked against its file every time. It's only a cache: losing or corrupting
/// it costs one slow load, so it lives in the OS's cache folder and any problem means ignoring it.
struct FileIndex<Facts> {
    name: &'static str,
    entries: std::sync::Mutex<Option<std::collections::HashMap<PathBuf, IndexedFile<Facts>>>>,
    changed: std::sync::atomic::AtomicBool,
}

#[derive(Serialize, Deserialize)]
struct IndexedFile<Facts> {
    modified: std::time::SystemTime,
    size: u64,
    facts: Facts,
}

#[derive(Serialize, Deserialize)]
struct SavedIndex<Facts> {
    build: String,
    files: std::collections::HashMap<PathBuf, IndexedFile<Facts>>,
}

/// Identifies this build of the app. A new build may read files differently, so an index
/// from another one is thrown away rather than trusted.
fn build_identity() -> String {
    let executable = std::env::current_exe().and_then(fs::metadata);
    let signature = executable.ok().and_then(|info| Some((info.modified().ok()?, info.len())));
    format!("{}:{:?}", env!("CARGO_PKG_VERSION"), signature)
}

impl<Facts: Clone + Serialize + serde::de::DeserializeOwned> FileIndex<Facts> {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            entries: std::sync::Mutex::new(None),
            changed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn location(&self) -> Option<PathBuf> {
        Some(dirs::cache_dir()?.join("com.claude-sessions.app").join(format!("{}.json", self.name)))
    }

    fn load_saved(&self) -> std::collections::HashMap<PathBuf, IndexedFile<Facts>> {
        self.location()
            .and_then(|location| fs::read(location).ok())
            .and_then(|bytes| serde_json::from_slice::<SavedIndex<Facts>>(&bytes).ok())
            .filter(|saved| saved.build == build_identity())
            .map(|saved| saved.files)
            .unwrap_or_default()
    }

    /// The date and size are read before the file is, so a write landing during the read
    /// makes the next check miss and read it again, rather than keeping half an answer.
    fn get_or_read(&self, path: &Path, read: impl FnOnce(&Path) -> Option<Facts>) -> Option<Facts> {
        let info = fs::metadata(path).ok()?;
        let (modified, size) = (info.modified().ok()?, info.len());
        {
            let mut guard = self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let entries = guard.get_or_insert_with(|| self.load_saved());
            if let Some(known) = entries.get(path).filter(|known| known.modified == modified && known.size == size) {
                return Some(known.facts.clone());
            }
        }
        let facts = read(path)?;
        let mut guard = self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let entries = guard.get_or_insert_with(|| self.load_saved());
        entries.insert(path.to_path_buf(), IndexedFile { modified, size, facts: facts.clone() });
        self.changed.store(true, std::sync::atomic::Ordering::Relaxed);
        Some(facts)
    }

    /// Writes the index if anything changed. Written beside its final place and renamed, so
    /// quitting midway can't leave a broken one. Failing is fine: the next launch reads more.
    fn save(&self) {
        if !self.changed.swap(false, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let Some(location) = self.location() else { return };
        let Some(folder) = location.parent() else { return };
        let mut guard = self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entries) = guard.take() else { return };
        let mut saved = SavedIndex { build: build_identity(), files: entries };
        saved.files.retain(|path, _| path.exists());
        if fs::create_dir_all(folder).is_ok() {
            let temporary = location.with_extension(format!("{}.tmp", std::process::id()));
            if let Ok(bytes) = serde_json::to_vec(&saved) {
                if fs::write(&temporary, bytes).is_err() || fs::rename(&temporary, &location).is_err() {
                    let _ = fs::remove_file(&temporary);
                }
            }
        }
        *guard = Some(saved.files);
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct SessionQuickMetadata {
    custom_title: Option<String>,
    ai_title: Option<String>,
    first_prompt: Option<String>,
    first_timestamp: Option<String>,
    last_timestamp: Option<String>,
    conversation_count: u64,
}

/// The two fields of a line the session list needs for every line. Reading only these skips
/// over tool output without building it in memory.
#[derive(Deserialize)]
struct LineHead {
    #[serde(rename = "type", default, deserialize_with = "loose")]
    entry_type: Option<String>,
    #[serde(default, deserialize_with = "loose")]
    timestamp: Option<String>,
}

/// Metadata per transcript, kept until the file changes. Listing every session on each
/// visit would otherwise re-read gigabytes of unchanged logs.
fn extract_quick_metadata(jsonl_path: &Path) -> SessionQuickMetadata {
    CLAUDE_INDEX.get_or_read(jsonl_path, |path| Some(read_quick_metadata(path))).unwrap_or_else(|| read_quick_metadata(jsonl_path))
}

fn read_quick_metadata(jsonl_path: &Path) -> SessionQuickMetadata {
    let mut metadata = SessionQuickMetadata::default();
    let mut prompts = PromptCounter::for_session(&own_session_id(jsonl_path));

    let file = match fs::File::open(jsonl_path) {
        Ok(file) => file,
        Err(_) => return metadata,
    };
    let reader = BufReader::new(file);

    // Single pass: extract head fields on first sighting, update tail fields
    // (custom_title, last_timestamp) continuously, and count user messages
    // using fast substring checks to skip irrelevant lines.
    for line in transcript_lines(reader) {
        if line.trim().is_empty() || is_api_record(&line) {
            continue;
        }

        // Custom title (can appear anywhere after /rename)
        if line.contains("\"type\":\"custom-title\"") {
            if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
                if let Some(title) = entry.custom_title {
                    metadata.custom_title = Some(title);
                }
            }
            continue;
        }

        // AI-generated title (Claude Code emits this once it has enough context)
        if line.contains("\"type\":\"ai-title\"") {
            if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
                if let Some(title) = entry.ai_title {
                    metadata.ai_title = Some(title);
                }
            }
            continue;
        }

        update_timestamps_from_line(&line, &mut metadata);

        if line.contains("\"type\":\"assistant\"") {
            prompts.add_assistant_output();
            continue;
        }

        // User message counting + first_prompt extraction. The substring can also match
        // inside bookkeeping entries, so confirm the type after parsing.
        // Tool-result lines can hold megabytes of screenshots, so they're skipped unparsed.
        // (Only a fork worker's log mixes a typed task with a result, and sidechains aren't
        // counted here.)
        if line.contains("\"toolUseResult\"") {
            continue;
        }
        prompts.add_queued_line(&line);
        if !line.contains("\"type\":\"user\"") {
            continue;
        }
        let entry: JsonlEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.entry_type.as_deref() != Some("user") || entry.is_sidechain.unwrap_or(false) {
            continue;
        }
        prompts.add_user_entry(&entry);

        if metadata.first_prompt.is_none() && is_user_prompt(&entry) {
            let cleaned = strip_system_tags(&extract_user_text(&entry.message));
            metadata.first_prompt = Some(if cleaned.is_empty() { "Image".to_string() } else { cleaned.chars().take(200).collect() });
        }
    }

    metadata.conversation_count = prompts.own;
    metadata
}

/// Claude Code keeps appending bookkeeping long after a session ends, and bookkeeping can hold
/// timestamps of its own. Only conversation lines date a session, and only real times count:
/// the session list groups and sorts by these.
fn update_timestamps_from_line(line: &str, metadata: &mut SessionQuickMetadata) {
    const CONVERSATION_TYPES: [&str; 4] = ["user", "assistant", "system", "attachment"];
    if !CONVERSATION_TYPES.iter().any(|kind| line.contains(&format!("\"type\":\"{}\"", kind))) {
        return;
    }
    let Ok(head) = serde_json::from_str::<LineHead>(line) else { return };
    if !head.entry_type.is_some_and(|kind| CONVERSATION_TYPES.contains(&kind.as_str())) {
        return;
    }
    let Some(timestamp) = head.timestamp.filter(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).is_ok()) else { return };
    if metadata.first_timestamp.is_none() {
        metadata.first_timestamp = Some(timestamp.clone());
    }
    metadata.last_timestamp = Some(timestamp);
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
        let tools = Default::default();
        let mut parser = TranscriptParser::with_agents(&tools);
        parser.push_notification(note_card("Agent \"X\" finished", "completed", "body"), "t1".to_string());
        parser.push_notification(note_card("Agent \"X\" finished", "completed", "body"), "t2".to_string());
        assert_eq!(parser.finish().len(), 1);
    }

    #[test]
    fn task_notifications_with_different_content_both_survive() {
        // A task-id that notifies twice with a different result is two real events.
        let tools = Default::default();
        let mut parser = TranscriptParser::with_agents(&tools);
        parser.push_notification(note_card("Agent \"X\" finished", "completed", "first run"), "t1".to_string());
        parser.push_notification(note_card("Agent \"X\" finished", "completed", "second run"), "t2".to_string());
        assert_eq!(parser.finish().len(), 2);
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
            !messages.iter().any(|message| message.text.contains("[Request interrupted by user]")),
            "marker leaked into output: {messages:#?}"
        );

        let roles: Vec<&str> = messages.iter().map(|message| message.role.as_str()).collect();
        assert_eq!(
            roles,
            ["assistant", "user", "user", "assistant", "user"],
            "unexpected message sequence: {messages:#?}"
        );

        // The cut-off assistant reply is flagged interrupted; the resumed one is not.
        assert!(messages[0].interrupted, "first assistant reply should be interrupted");
        assert!(!messages[3].interrupted, "resumed assistant reply should not be interrupted");

        // Prompts typed after Esc went to an idle agent; only queued ones were sent mid-turn.
        assert!(messages.iter().all(|message| !message.mid_turn));
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
        let array_msg = messages.iter().find(|message| message.text.contains("branch support")).expect("array-prompt queued message");
        assert_eq!(array_msg.role, "user");
        assert!(array_msg.mid_turn);
        let string_msg = messages.iter().find(|message| message.text.contains("full, not dimmed")).expect("string-prompt queued message");
        assert!(string_msg.mid_turn, "string-prompt queued message should be mid_turn");
        // No duplication from the queue-operation bookkeeping entry.
        assert_eq!(messages.iter().filter(|message| message.text.contains("branch support")).count(), 1);
        // The task_reminder attachment must NOT become a message.
        assert!(!messages.iter().any(|message| message.text.contains("itemCount")));
        // Dedup: the message stored as BOTH a user turn and an attachment appears once.
        assert_eq!(
            messages.iter().filter(|message| message.text.contains("dark theme toggle")).count(),
            1,
            "a queued message stored as both a user turn and an attachment must not duplicate"
        );
    }

    #[test]
    fn export_start_index_trims_the_beginning_and_keeps_the_end() {
        // Three user turns; exporting from index 1 must drop the first and keep the
        // rest through the end, with a "partial export" note at the top.
        let lines = [
            r#"{"type":"user","timestamp":"2026-08-12T05:00:00Z","message":{"role":"user","content":"first message alpha"}}"#,
            r#"{"type":"user","timestamp":"2026-08-12T05:01:00Z","message":{"role":"user","content":"second message beta"}}"#,
            r#"{"type":"user","timestamp":"2026-08-12T05:02:00Z","message":{"role":"user","content":"third message gamma"}}"#,
        ];
        let dir = std::env::temp_dir();
        let source = dir.join(format!("cs_export_src_{}.jsonl", std::process::id()));
        let dest = dir.join(format!("cs_export_out_{}.md", std::process::id()));
        fs::write(&source, lines.join("\n")).unwrap();

        export_session_markdown(
            source.to_string_lossy().to_string(),
            dest.to_string_lossy().to_string(),
            Some("My Session".to_string()),
            Some(1),
        )
        .unwrap();
        let output = fs::read_to_string(&dest).unwrap();
        let _ = fs::remove_file(&source);
        let _ = fs::remove_file(&dest);

        assert!(!output.contains("first message alpha"), "start=1 must drop the first message");
        assert!(output.contains("second message beta"), "must keep the second message");
        assert!(output.contains("third message gamma"), "must keep through the end");
        assert!(output.contains("Partial export"), "a partial export must be noted");
        assert!(output.contains("# My Session"), "the title is still written");
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

    #[test]
    fn archived_sessions_are_listed_from_archive_with_meta_project() {
        let session_id = format!("sess-listed-{}", std::process::id());
        let base = std::env::temp_dir().join(format!("cs_archived_list_test_{}", std::process::id()));
        let live_project = base.join("live");
        let archive_root = base.join("archive");
        fs::create_dir_all(&live_project).unwrap();
        let source_jsonl = live_project.join(format!("{}.jsonl", session_id));
        fs::write(
            &source_jsonl,
            r#"{"type":"user","timestamp":"2026-08-01T10:00:00.000Z","message":{"role":"user","content":"find the flaky test"}}
{"type":"custom-title","customTitle":"Flaky test hunt"}
"#,
        )
        .unwrap();
        let meta = serde_json::json!({ "session_id": session_id, "project_path": "/Users/me/app", "project_name": "app" });
        archive_session_to(&archive_root, &source_jsonl, &session_id, None, &meta).unwrap();

        // The live transcript expiring must not hide the saved copy.
        fs::remove_file(&source_jsonl).unwrap();
        let listed = archived_sessions_in(&archive_root);
        let _ = fs::remove_dir_all(&base);

        assert_eq!(listed.len(), 1);
        let session = &listed[0];
        assert_eq!(session.session_id, session_id);
        assert_eq!(session.project_path, "/Users/me/app");
        assert_eq!(session.project_name, "app");
        assert_eq!(session.custom_title.as_deref(), Some("Flaky test hunt"));
        assert!(session.jsonl_path.contains("archive"), "must point at the archived copy: {}", session.jsonl_path);
        assert_eq!(session.modified.as_deref(), Some("2026-08-01T10:00:00.000Z"));
    }

    #[test]
    fn tool_output_path_rejects_traversal_outside_projects() {
        let base = std::env::temp_dir().join(format!("cs_tool_output_test_{}", std::process::id()));
        let projects = base.join("projects");
        let tool_results = projects.join("proj").join("sess").join("tool-results");
        fs::create_dir_all(&tool_results).unwrap();
        fs::write(tool_results.join("out.txt"), "tool output").unwrap();
        fs::write(base.join("secret.txt"), "not yours").unwrap();

        let legit = tool_results.join("out.txt");
        assert!(validate_tool_output_path(&legit, &projects).is_ok());

        // Lexically contains "tool-results" but resolves outside the projects dir.
        let escaping = tool_results.join("..").join("..").join("..").join("..").join("secret.txt");
        assert!(escaping.exists(), "fixture should exist so only the guard can reject it");
        assert!(validate_tool_output_path(&escaping, &projects).is_err());

        // Inside projects but not under a tool-results directory.
        fs::write(projects.join("proj").join("notes.txt"), "x").unwrap();
        assert!(validate_tool_output_path(&projects.join("proj").join("notes.txt"), &projects).is_err());

        let _ = fs::remove_dir_all(&base);
    }

    /// Write `lines` to a temp transcript, parse it with the real parser, clean up.
    fn parse_lines(test_name: &str, lines: &[&str]) -> Vec<ConversationMessage> {
        let path = std::env::temp_dir().join(format!("cs_{}_{}.jsonl", test_name, std::process::id()));
        fs::write(&path, lines.join("\n")).unwrap();
        let messages = get_session_messages(path.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_file(&path);
        messages
    }

    fn block_kinds(message: &ConversationMessage) -> Vec<&'static str> {
        message
            .blocks
            .iter()
            .map(|block| match block {
                MessageBlock::Text { .. } => "text",
                MessageBlock::Thinking { .. } => "thinking",
                MessageBlock::Tool { .. } => "tool",
                MessageBlock::Plan { .. } => "plan",
                MessageBlock::Note { .. } => "note",
                MessageBlock::Hook { .. } => "hook",
            })
            .collect()
    }

    #[test]
    fn blocks_of_one_request_written_on_separate_lines_are_all_kept() {
        // Real shape: each content block of one API response is its own line.
        let messages = parse_lines("merge", &[
            r#"{"type":"assistant","requestId":"req_1","timestamp":"t1","message":{"content":[{"type":"thinking","thinking":"I should read it"}]}}"#,
            r#"{"type":"assistant","requestId":"req_1","timestamp":"t2","message":{"content":[{"type":"text","text":"Let me read the file."}]}}"#,
            r#"{"type":"assistant","requestId":"req_1","timestamp":"t3","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a.txt"}}]}}"#,
            r#"{"type":"assistant","requestId":"req_1","timestamp":"t4","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a.txt"}}]}}"#,
        ]);
        assert_eq!(messages.len(), 1);
        assert_eq!(block_kinds(&messages[0]), ["thinking", "text", "tool"], "a repeated block must not duplicate");
        assert_eq!(messages[0].timestamp, "t1");
    }

    #[test]
    fn non_ascii_text_survives_ansi_stripping() {
        assert_eq!(strip_ansi("fix \u{1b}[2mthe\u{1b}[0m bug — café 🎉"), "fix the bug — café 🎉");
    }

    #[test]
    fn pasted_content_wrapper_is_removed_and_text_kept() {
        let input = "look at this\n\n<pasted_content id=\"b4eb\">\nthe pasted text\n</pasted_content id=\"b4eb\">\nthanks";
        assert_eq!(strip_system_tags(input), "look at this\n\nthe pasted text\nthanks");
        // Without a closing tag it's literal text, not something to swallow.
        assert_eq!(strip_system_tags("<pasted_content id=\"x\">oops"), "<pasted_content id=\"x\">oops");
    }

    #[test]
    fn prompt_mentioning_command_tags_is_not_treated_as_a_command() {
        let input = "why does this log say <command-name>/foo</command-name> here?";
        assert!(strip_system_tags(input).starts_with("why does this log say"));
    }

    #[test]
    fn tool_summaries_keep_pipes_and_braces() {
        let block = serde_json::json!({"id":"toolu_1","name":"Bash","input":{"command":"git log | head -30 && echo ${HOME}"}});
        match tool_block(&block, &Default::default(), &mut Vec::new()) {
            Some(MessageBlock::Tool { summary, .. }) => assert_eq!(summary, "git log | head -30 && echo ${HOME}"),
            other => panic!("expected a tool block, got {other:?}"),
        }
    }

    #[test]
    fn unknown_tools_still_get_a_summary() {
        let block = serde_json::json!({"id":"toolu_1","name":"mcp__new__thing","input":{"count":3,"whatever":"useful line"}});
        match tool_block(&block, &Default::default(), &mut Vec::new()) {
            Some(MessageBlock::Tool { summary, .. }) => assert_eq!(summary, "useful line"),
            other => panic!("expected a tool block, got {other:?}"),
        }
    }

    #[test]
    fn plan_recap_and_chapter_tools_become_conversation_blocks() {
        let messages = parse_lines("ccd", &[
            r#"{"type":"assistant","requestId":"r","timestamp":"t","message":{"content":[{"type":"tool_use","id":"a","name":"mcp__ccd_turn__update_plan","input":{"items":[{"content":"Map repo","status":"completed"}]}},{"type":"tool_use","id":"b","name":"mcp__ccd_turn__wrap_up","input":{"recap":"All done"}},{"type":"tool_use","id":"c","name":"mcp__ccd_session__mark_chapter","input":{"title":"Part 2","summary":"next"}}]}}"#,
        ]);
        assert_eq!(block_kinds(&messages[0]), ["plan", "note", "note"]);
        assert!(messages[0].text.contains("Map repo") && messages[0].text.contains("All done"));
    }

    #[test]
    fn shell_commands_split_across_two_entries_become_one_card() {
        let messages = parse_lines("shell", &[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<bash-input>ls &gt; out</bash-input>"}}"#,
            r#"{"type":"user","timestamp":"t2","message":{"content":"<bash-stdout>a — b</bash-stdout><bash-stderr></bash-stderr>"}}"#,
            r#"{"type":"user","timestamp":"t3","message":{"content":"<bash-input>pwd</bash-input><bash-stdout>/tmp</bash-stdout><bash-stderr>warn</bash-stderr>"}}"#,
        ]);
        assert_eq!(messages.iter().map(|message| message.role.as_str()).collect::<Vec<_>>(), ["shell", "shell"]);
        let first = messages[0].shell.as_ref().unwrap();
        assert_eq!((first.command.as_str(), first.stdout.as_str()), ("ls > out", "a — b"));
        assert_eq!(messages[1].shell.as_ref().unwrap().stderr, "warn");
    }

    #[test]
    fn subagent_hand_back_is_surfaced_despite_being_meta() {
        let messages = parse_lines("peer", &[
            r#"{"type":"user","isMeta":true,"timestamp":"t","origin":{"kind":"peer","from":"abc","name":"general-purpose","handback":true,"body":"[Subagent hand-back] noise","handbackReportText":"The clean report"},"message":{"content":"Another Claude session sent a message:\n<agent-message from=\"abc\">noise</agent-message>"}}"#,
        ]);
        let note = messages[0].notification.as_ref().expect("hand-back card");
        assert_eq!(note.result, "The clean report");
        assert_eq!(note.kind, "hand-back");
    }

    #[test]
    fn monitor_event_notification_keeps_its_event_and_strips_ansi() {
        let note = parse_task_notification("<task-notification>\n<task-id>x</task-id>\n<summary>Monitor event: \"sync\"</summary>\n<event>\u{1b}[36mfetched\u{1b}[0m 25 -&gt; ok</event>\nIf this event matters</task-notification>").unwrap();
        assert_eq!(note.result, "fetched 25 -> ok");
        assert_eq!((note.kind.as_str(), note.status.as_str()), ("monitor", "event"));
    }

    #[test]
    fn system_entries_surface_and_unknown_subtypes_are_not_lost() {
        let messages = parse_lines("system", &[
            r#"{"type":"system","subtype":"turn_duration","durationMs":5,"timestamp":"t"}"#,
            r#"{"type":"system","subtype":"api_error","error":{"formatted":"Connection dropped"},"retryAttempt":1,"maxRetries":10,"timestamp":"t"}"#,
            r#"{"type":"system","subtype":"api_error","error":{"formatted":"Connection dropped"},"retryAttempt":2,"maxRetries":10,"timestamp":"t"}"#,
            r#"{"type":"system","subtype":"away_summary","content":"We were doing X","timestamp":"t"}"#,
            r#"{"type":"system","subtype":"brand_new_thing","content":"A future notice","timestamp":"t"}"#,
            r#"{"type":"system","subtype":"local_command","content":"<command-name>/branch</command-name><command-args>x</command-args>","timestamp":"t"}"#,
            r#"{"type":"system","subtype":"local_command","content":"<local-command-stdout>Branched</local-command-stdout>","timestamp":"t"}"#,
        ]);
        let texts: Vec<&str> = messages.iter().map(|message| message.text.as_str()).collect();
        assert_eq!(texts, ["Connection dropped (retry 2/10)", "We were doing X", "A future notice", "/branch x"]);
        assert_eq!(messages[3].event.as_ref().unwrap().detail, "Branched");
    }

    #[test]
    fn repeated_hook_failures_collapse_and_quiet_success_is_hidden() {
        let failing = r#"{"type":"attachment","timestamp":"t","attachment":{"type":"async_hook_response","hookName":"PostToolUse:Bash","hookEvent":"PostToolUse","stderr":"Cannot find module","exitCode":1}}"#;
        let messages = parse_lines("hooks", &[
            failing,
            failing,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"async_hook_response","hookName":"Stop","hookEvent":"Stop","stderr":"","exitCode":0}}"#,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"hook_additional_context","hookName":"SessionStart","hookEvent":"SessionStart","content":["Be lazy"]}}"#,
        ]);
        assert_eq!(messages.len(), 2);
        assert!(messages[0].event.as_ref().unwrap().is_error);
        assert_eq!(messages[1].text, "SessionStart hook added context");
        assert_eq!(messages[1].event.as_ref().unwrap().detail, "Be lazy");
    }

    #[test]
    fn newer_artifact_url_shape_is_extracted() {
        let path = std::env::temp_dir().join(format!("cs_new_artifact_{}.jsonl", std::process::id()));
        fs::write(&path, r#"{"type":"user","toolUseResult":{"url":"https://claude.ai/artifact/EMvsJY5","artifact_id":"6c32","title":"Plan","path":"/tmp/p.html"},"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_9","content":"ok"}]}}"#).unwrap();
        let artifacts = get_session_artifacts(path.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(artifacts["toolu_9"].title, "Plan");
    }

    #[test]
    fn tool_results_report_images_tool_references_and_recorded_paths() {
        let path = std::env::temp_dir().join(format!("cs_results_{}.jsonl", std::process::id()));
        fs::write(&path, r#"{"type":"user","toolUseResult":{"persistedOutputPath":"/x/tool-results/a.txt"},"message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"shot"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"QUJD"}},{"type":"tool_reference","tool_name":"Monitor"}]}]}}"#).unwrap();
        let jsonl_path = path.to_string_lossy().to_string();
        let results = get_tool_results(jsonl_path.clone()).unwrap();
        let images = get_tool_result_images(jsonl_path, "t1".to_string()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(results["t1"].image_count, 1);
        assert_eq!(results["t1"].content, "shot\nLoaded tool: Monitor");
        assert_eq!(results["t1"].persisted_path.as_deref(), Some("/x/tool-results/a.txt"));
        assert_eq!(images, ["data:image/png;base64,QUJD"]);
    }

    #[test]
    fn bash_edit_diffs_appear_in_file_changes() {
        let path = std::env::temp_dir().join(format!("cs_bashdiff_{}.jsonl", std::process::id()));
        fs::write(&path, r#"{"type":"user","timestamp":"t","toolUseResult":{"stdout":"","bashEditDiff":{"files":[{"filePath":"/p/a.json","hunks":[{"lines":["-old","+new"," same"]}]}]}},"message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#).unwrap();
        let changes = get_session_file_changes(path.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(changes[0].path, "/p/a.json");
        assert_eq!(changes[0].edits[0].action, "bash");
        assert_eq!(changes[0].edits[0].old_string.as_deref(), Some("old"));
        assert_eq!(changes[0].edits[0].new_string.as_deref(), Some("new"));
    }

    #[test]
    fn export_keeps_every_kind_of_content_and_never_leaks_markers() {
        let lines = [
            r#"{"type":"user","timestamp":"t0","message":{"content":"héllo <pasted_content id=\"a\">pasted</pasted_content id=\"a\">"}}"#,
            r#"{"type":"assistant","requestId":"r","timestamp":"t1","message":{"content":[{"type":"thinking","thinking":"secret plan"}]}}"#,
            r#"{"type":"assistant","requestId":"r","timestamp":"t2","message":{"content":[{"type":"text","text":"Prose before"}]}}"#,
            r#"{"type":"assistant","requestId":"r","timestamp":"t3","message":{"content":[{"type":"tool_use","id":"t9","name":"Bash","input":{"command":"cat f | grep ${X}","description":"d"}}]}}"#,
            r#"{"type":"user","timestamp":"t4","toolUseResult":{"stdout":"x"},"message":{"content":[{"type":"tool_result","tool_use_id":"t9","content":"has ``` fence inside"}]}}"#,
            r#"{"type":"user","isMeta":true,"timestamp":"t5","origin":{"kind":"peer","name":"helper","handback":true,"handbackReportText":"agent report"},"message":{"content":"x"}}"#,
            r#"{"type":"user","timestamp":"t6","message":{"content":"<bash-input>ls</bash-input><bash-stdout>out</bash-stdout><bash-stderr></bash-stderr>"}}"#,
        ];
        let dir = std::env::temp_dir();
        let source = dir.join(format!("cs_fidelity_src_{}.jsonl", std::process::id()));
        let dest = dir.join(format!("cs_fidelity_out_{}.md", std::process::id()));
        fs::write(&source, lines.join("\n")).unwrap();
        export_session_markdown(source.to_string_lossy().to_string(), dest.to_string_lossy().to_string(), None, None).unwrap();
        let output = fs::read_to_string(&dest).unwrap();
        let _ = fs::remove_file(&source);
        let _ = fs::remove_file(&dest);
        for expected in ["héllo pasted", "secret plan", "Prose before", "cat f | grep ${X}", "````\nhas ``` fence inside", "agent report", "$ ls", "out"] {
            assert!(output.contains(expected), "missing {expected:?} in:\n{output}");
        }
        for leak in ["{{TOOL", "{{THINKING", "<pasted_content", "<bash-input"] {
            assert!(!output.contains(leak), "leaked {leak:?} in:\n{output}");
        }
    }

    #[test]
    fn archive_keeps_persisted_tool_output_and_workflow_transcripts() {
        let session_id = format!("sess-full-{}", std::process::id());
        let base = std::env::temp_dir().join(format!("cs_archive_full_{}", std::process::id()));
        let live_session = base.join("live").join(&session_id);
        fs::create_dir_all(live_session.join("tool-results")).unwrap();
        fs::create_dir_all(live_session.join("subagents/workflows/wf_1")).unwrap();
        fs::write(live_session.join("tool-results/out.txt"), "full output").unwrap();
        fs::write(live_session.join("subagents/workflows/wf_1/agent-w.jsonl"), "{}").unwrap();
        let source = base.join("live").join(format!("{}.jsonl", session_id));
        fs::write(&source, "{}\n").unwrap();
        let root = base.join("archive");
        archive_session_to(&root, &source, &session_id, None, &Value::Null).unwrap();

        let archived_output = root.join(&session_id).join(&session_id).join("tool-results/out.txt");
        assert!(validate_tool_output_path(&archived_output, &root).is_ok());

        let archived_jsonl = root.join(&session_id).join(format!("{}.jsonl", session_id));
        let subagents = list_subagents(archived_jsonl.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_dir_all(&base);
        assert_eq!(subagents.len(), 1);
        assert_eq!(subagents[0].agent_id, "w");
        assert_eq!(subagents[0].workflow_run.as_deref(), Some("wf_1"));
    }

    #[test]
    fn recorded_cwd_resolves_paths_the_folder_name_cannot() {
        assert_eq!(encode_project_path("/Users/me/my-repo_v2/sub"), "-Users-me-my-repo-v2-sub");
        let dir = std::env::temp_dir().join(format!("cs_cwd_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.jsonl"), "{\"type\":\"mode\"}\n{\"cwd\":\"/elsewhere\"}\n{\"cwd\":\"/Users/me/my-repo/sub\"}\n").unwrap();
        let resolved = recorded_project_path(&dir, "-Users-me-my-repo-sub");
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(resolved.as_deref(), Some("/Users/me/my-repo/sub"));
    }

    #[test]
    fn prompt_count_ignores_everything_the_person_did_not_type() {
        let path = std::env::temp_dir().join(format!("cs_prompts_{}.jsonl", std::process::id()));
        fs::write(&path, [
            r#"{"type":"user","timestamp":"t","message":{"content":"a real prompt"}}"#,
            r#"{"type":"user","timestamp":"t","message":{"content":"[Request interrupted by user]"}}"#,
            r#"{"type":"user","timestamp":"t","message":{"content":"<task-notification><summary>s</summary></task-notification>"}}"#,
            r#"{"type":"user","isMeta":true,"timestamp":"t","message":{"content":"skill body"}}"#,
            r#"{"type":"user","timestamp":"t","message":{"content":"<system-reminder>only noise</system-reminder>"}}"#,
        ].join("\n")).unwrap();
        let metadata = extract_quick_metadata(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(metadata.conversation_count, 1);
        assert_eq!(metadata.first_prompt.as_deref(), Some("a real prompt"));
    }

    #[test]
    fn match_snippet_centers_on_the_match_in_non_ascii_text() {
        let text = format!("{} needle — café", "é".repeat(300));
        let snippet = match_snippet(&text, &["needle"]);
        assert!(snippet.starts_with('…') && snippet.contains("needle — café"), "got {snippet}");
    }

    #[test]
    fn hook_output_during_a_turn_does_not_split_it() {
        let messages = parse_lines("hookturn", &[
            r#"{"type":"assistant","requestId":"a","timestamp":"t","message":{"content":[{"type":"text","text":"before"}]}}"#,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"hook_success","hookName":"PostToolUse:Write","hookEvent":"PostToolUse","content":"formatted"}}"#,
            r#"{"type":"assistant","requestId":"b","timestamp":"t","message":{"content":[{"type":"text","text":"after"}]}}"#,
        ]);
        assert_eq!(messages.len(), 1);
        assert_eq!(block_kinds(&messages[0]), ["text", "hook", "text"]);
    }

    #[test]
    fn consecutive_hook_rows_between_turns_merge() {
        let messages = parse_lines("hookmerge", &[
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"hook_success","hookName":"SessionStart","hookEvent":"SessionStart","content":"one"}}"#,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"hook_success","hookName":"SessionStart","hookEvent":"SessionStart","content":"two"}}"#,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"hook_success","hookName":"SessionStart","hookEvent":"SessionStart","content":"three"}}"#,
        ]);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "3 hook events");
        let detail = &messages[0].event.as_ref().unwrap().detail;
        assert!(detail.contains("one") && detail.contains("two") && detail.contains("three"));
    }

    #[test]
    fn synthetic_api_errors_are_shown_and_placeholders_are_not() {
        let messages = parse_lines("synthetic", &[
            r#"{"type":"assistant","timestamp":"t","message":{"model":"<synthetic>","content":[{"type":"text","text":"API Error: Connection lost mid-response."}]}}"#,
            r#"{"type":"assistant","timestamp":"t","message":{"model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}}"#,
        ]);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].event.as_ref().unwrap().is_error);
    }

    #[test]
    fn long_commands_keep_their_full_text_as_input() {
        let command = format!("echo {}", "long ".repeat(30));
        let block = serde_json::json!({"id":"t","name":"Bash","input":{"command":command}});
        match tool_block(&block, &Default::default(), &mut Vec::new()) {
            Some(MessageBlock::Tool { input, .. }) => assert_eq!(input, command),
            other => panic!("expected a tool block, got {other:?}"),
        }
    }

    #[test]
    fn search_matches_clean_prompt_text_and_ignores_tool_input() {
        let path = std::env::temp_dir().join(format!("cs_search_{}.jsonl", std::process::id()));
        fs::write(&path, [
            r#"{"type":"assistant","requestId":"a","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"x","name":"Bash","input":{"command":"grep zebra"}}]}}"#,
            r#"{"type":"user","timestamp":"t2","message":{"content":"<pasted_content id=\"a\">the café giraffe</pasted_content id=\"a\">"}}"#,
            r#"{"type":"custom-title","customTitle":"Named session"}"#,
        ].join("\n")).unwrap();
        let hit = search_transcript(&path, "/p/app", &["giraffe"]);
        let miss = search_transcript(&path, "/p/app", &["zebra"]);
        let _ = fs::remove_file(&path);
        let hit = hit.expect("prompt text should match");
        assert_eq!(hit.matched_text, "the café giraffe");
        assert_eq!(hit.project_name, "app");
        assert!(miss.is_none(), "tool input isn't conversation text");
    }

    #[test]
    fn only_web_links_can_be_opened() {
        assert!(open_external_url("file:///etc/passwd".to_string()).is_err());
        assert!(open_external_url("/Applications/Calculator.app".to_string()).is_err());
    }

    #[test]
    fn forks_can_open_agents_from_their_parent_and_bookkeeping_files_are_not_agents() {
        let base = std::env::temp_dir().join(format!("cs_fork_agents_{}", std::process::id()));
        fs::create_dir_all(base.join("parent/subagents/workflows/wf")).unwrap();
        fs::write(base.join("parent.jsonl"), "{}\n").unwrap();
        fs::write(base.join("parent/subagents/agent-a.jsonl"), "{}\n").unwrap();
        fs::write(base.join("parent/subagents/workflows/wf/journal.jsonl"), "{}\n").unwrap();
        let fork = base.join("fork.jsonl");
        fs::write(base.join("parent/subagents/agent-after-fork.jsonl"), "{}\n").unwrap();
        fs::write(&fork, r#"{"type":"mode"}
{"type":"user","forkedFrom":{"sessionId":"parent"},"toolUseResult":{"agentId":"a"},"message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"done"}]}}"#).unwrap();
        let listed = list_subagents(fork.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_dir_all(&base);
        assert_eq!(listed.iter().map(|agent| agent.agent_id.as_str()).collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn a_subagents_brief_is_shown_even_when_flagged_meta() {
        let path = std::env::temp_dir().join(format!("cs_brief_{}.jsonl", std::process::id()));
        fs::write(&path, [
            r#"{"type":"user","isSidechain":true,"isMeta":true,"timestamp":"t","message":{"content":"Review the code"}}"#,
            r#"{"type":"user","isSidechain":true,"isMeta":true,"timestamp":"t","message":{"content":"skill body"}}"#,
        ].join("\n")).unwrap();
        let messages = get_subagent_messages(path.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(messages.iter().map(|message| message.text.as_str()).collect::<Vec<_>>(), ["Review the code"]);
    }

    #[test]
    fn forked_agent_task_beside_a_tool_result_is_shown_without_boilerplate() {
        let messages = parse_lines("fork", &[
            r#"{"type":"user","isSidechain":false,"timestamp":"t","toolUseResult":{"status":"ok"},"message":{"content":[{"type":"tool_result","tool_use_id":"p","content":"parent"},{"type":"text","text":"<fork-boilerplate>You are a worker fork.</fork-boilerplate>\nBuild the dock."}]}}"#,
            r#"{"type":"user","timestamp":"t","toolUseResult":{"status":"ok"},"message":{"content":[{"type":"tool_result","tool_use_id":"q","content":"only a result"}]}}"#,
        ]);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "Build the dock.");
    }

    #[test]
    fn browser_instruction_is_removed_and_the_real_message_kept() {
        let input = "<browser_instruction># Claude in Chrome\nlong instructions</browser_instruction>\nwhats wrong here?";
        assert_eq!(strip_system_tags(input), "whats wrong here?");
    }

    #[test]
    fn tool_output_loses_injected_reminders() {
        let content = serde_json::json!("real output\n<system-reminder>note to the model</system-reminder>");
        assert_eq!(tool_result_text(Some(&content)).0.trim(), "real output");
    }

    #[test]
    fn a_reminder_tag_inside_real_tool_output_is_kept() {
        let file_text = "let s = \"<system-reminder>a test fixture</system-reminder>\";\nmore code";
        assert_eq!(clean_tool_output(file_text), file_text);
        let with_trailing = format!("{}\n<system-reminder>one</system-reminder>\n<system-reminder>two</system-reminder>\n", file_text);
        assert_eq!(clean_tool_output(&with_trailing), file_text);
    }

    #[test]
    fn compaction_summaries_lose_their_tool_list_tag() {
        let messages = parse_lines("compact_tag", &[
            r#"{"type":"user","isCompactSummary":true,"timestamp":"t","message":{"content":"Continued.\n<tools-used-before-summary>Bash, Read</tools-used-before-summary>\nSummary: done"}}"#,
        ]);
        assert!(!messages[0].text.contains("tools-used-before-summary") && !messages[0].text.contains("Bash, Read"), "got {:?}", messages[0].text);
        assert!(messages[0].text.contains("Summary: done"));
    }

    #[test]
    fn tools_with_only_numeric_input_still_get_a_summary() {
        let block = serde_json::json!({"id":"t","name":"mcp__x__close_tab","input":{"tabId":42}});
        match tool_block(&block, &Default::default(), &mut Vec::new()) {
            Some(MessageBlock::Tool { summary, .. }) => assert_eq!(summary, "tabId: 42"),
            other => panic!("expected a tool block, got {other:?}"),
        }
    }

    #[test]
    fn queued_prompts_count_once_and_notifications_do_not_count() {
        let path = std::env::temp_dir().join(format!("cs_queued_count_{}.jsonl", std::process::id()));
        fs::write(&path, [
            r#"{"type":"user","timestamp":"t","message":{"content":"typed turn"}}"#,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"queued_command","prompt":"typed turn"}}"#,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"queued_command","prompt":"queued only"}}"#,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"queued_command","prompt":"<task-notification><summary>s</summary></task-notification>"}}"#,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"queued_command","prompt":"hi","origin":{"kind":"peer","name":"x","body":"hi"}}}"#,
        ].join("\n")).unwrap();
        let jsonl_path = path.to_string_lossy().to_string();
        let stats = get_session_stats(jsonl_path).unwrap();
        let metadata = extract_quick_metadata(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(stats.user_prompt_count, 2);
        assert_eq!(metadata.conversation_count, 2);
    }

    #[test]
    fn assistant_text_mentioning_hook_terms_keeps_its_token_counts() {
        let path = std::env::temp_dir().join(format!("cs_hook_words_{}.jsonl", std::process::id()));
        fs::write(&path, r#"{"type":"assistant","requestId":"r","message":{"usage":{"input_tokens":5,"output_tokens":7},"content":[{"type":"text","text":"the \"hook field and stop_hook_summary"}]}}"#).unwrap();
        let stats = get_session_stats(path.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!((stats.input_tokens, stats.output_tokens), (5, 7));
    }

    #[test]
    fn file_changes_include_subagent_edits() {
        let base = std::env::temp_dir().join(format!("cs_sub_edits_{}", std::process::id()));
        fs::create_dir_all(base.join("s1/subagents")).unwrap();
        fs::write(base.join("s1.jsonl"), r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"a","name":"Edit","input":{"file_path":"/p/main.rs","old_string":"x","new_string":"y"}}]}}"#).unwrap();
        fs::write(base.join("s1/subagents/agent-z.jsonl"), r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"tool_use","id":"b","name":"Write","input":{"file_path":"/p/sub.rs","content":"z"}}]}}"#).unwrap();
        let changes = get_session_file_changes(base.join("s1.jsonl").to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_dir_all(&base);
        let paths: std::collections::HashSet<&str> = changes.iter().map(|change| change.path.as_str()).collect();
        assert!(paths.contains("/p/main.rs") && paths.contains("/p/sub.rs"), "got {paths:?}");
    }

    #[test]
    fn a_fork_does_not_claim_edits_from_agents_its_parent_ran_later() {
        let base = std::env::temp_dir().join(format!("cs_fork_files_{}", std::process::id()));
        fs::create_dir_all(base.join("parent/subagents")).unwrap();
        fs::write(base.join("parent.jsonl"), "{}\n").unwrap();
        let edit = |path: &str| format!(r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"e","name":"Write","input":{{"file_path":"{}","content":"x"}}}}]}}}}"#, path);
        fs::write(base.join("parent/subagents/agent-inherited.jsonl"), edit("/p/inherited.rs")).unwrap();
        fs::write(base.join("parent/subagents/agent-later.jsonl"), edit("/p/later.rs")).unwrap();
        let fork = base.join("fork.jsonl");
        fs::write(&fork, r#"{"type":"user","forkedFrom":{"sessionId":"parent"},"toolUseResult":{"agentId":"inherited"},"message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"done"}]}}"#).unwrap();
        let changes = get_session_file_changes(fork.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_dir_all(&base);
        let paths: Vec<&str> = changes.iter().map(|change| change.path.as_str()).collect();
        assert_eq!(paths, ["/p/inherited.rs"]);
    }

    #[test]
    fn older_archives_get_folders_they_were_missing_even_when_the_transcript_is_unchanged() {
        let session_id = format!("sess-old-{}", std::process::id());
        let base = std::env::temp_dir().join(format!("cs_archive_backfill_{}", std::process::id()));
        let live_dir = base.join("live");
        fs::create_dir_all(live_dir.join(&session_id).join("tool-results")).unwrap();
        fs::write(live_dir.join(&session_id).join("tool-results/out.txt"), "output").unwrap();
        let source = live_dir.join(format!("{}.jsonl", session_id));
        fs::write(&source, "{}\n").unwrap();
        // An archive made by an older version: a current transcript, no session folder.
        let root = base.join("archive");
        fs::create_dir_all(root.join(&session_id)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(root.join(&session_id).join(format!("{}.jsonl", session_id)), "{}\n").unwrap();

        archive_session_to(&root, &source, &session_id, None, &Value::Null).unwrap();
        let backfilled = root.join(&session_id).join(&session_id).join("tool-results/out.txt").exists();
        let _ = fs::remove_dir_all(&base);
        assert!(backfilled);
    }

    #[test]
    fn archiving_a_session_opened_from_its_archive_leaves_it_intact() {
        let session_id = format!("sess-self-{}", std::process::id());
        let root = std::env::temp_dir().join(format!("cs_archive_self_{}", std::process::id()));
        let session_dir = root.join(&session_id);
        fs::create_dir_all(session_dir.join(&session_id).join("subagents")).unwrap();
        let archived = session_dir.join(format!("{}.jsonl", session_id));
        fs::write(&archived, "kept\n").unwrap();
        archive_session_to(&root, &archived, &session_id, None, &Value::Null).unwrap();
        let text = fs::read_to_string(&archived).unwrap();
        let _ = fs::remove_dir_all(&root);
        assert_eq!(text, "kept\n");
    }

    #[test]
    fn a_failed_copy_is_reported_but_the_transcript_is_still_saved() {
        let session_id = format!("sess-fail-{}", std::process::id());
        let base = std::env::temp_dir().join(format!("cs_archive_fail_{}", std::process::id()));
        fs::create_dir_all(base.join("live").join(&session_id)).unwrap();
        let source = base.join("live").join(format!("{}.jsonl", session_id));
        fs::write(&source, "{}\n").unwrap();
        fs::write(base.join("live").join(&session_id).join("out.txt"), "x").unwrap();
        // A file where the session folder should go makes the folder copy fail.
        let root = base.join("archive");
        fs::create_dir_all(root.join(&session_id)).unwrap();
        fs::write(root.join(&session_id).join(&session_id), "in the way").unwrap();

        let result = archive_session_to(&root, &source, &session_id, None, &Value::Null);
        let saved = root.join(&session_id).join(format!("{}.jsonl", session_id)).exists();
        let _ = fs::remove_dir_all(&base);
        assert!(result.unwrap_err().contains("session files"));
        assert!(saved);
    }

    #[test]
    fn cost_adds_up_across_restarts_and_survives_rewinds() {
        let path = std::env::temp_dir().join(format!("cs_cost_{}.jsonl", std::process::id()));
        fs::write(&path, [
            r#"{"type":"cost-state","startTime":1,"totalCostUSD":100.0,"totalLinesAdded":10,"totalLinesRemoved":1}"#,
            r#"{"type":"cost-state","startTime":1,"totalCostUSD":80.0,"totalLinesAdded":8,"totalLinesRemoved":1}"#,
            r#"{"type":"cost-state","startTime":1,"totalCostUSD":120.0,"totalLinesAdded":12,"totalLinesRemoved":2}"#,
            r#"{"type":"cost-state","startTime":2,"totalCostUSD":0.0,"totalLinesAdded":0,"totalLinesRemoved":0}"#,
            r#"{"type":"cost-state","startTime":2,"totalCostUSD":30.0,"totalLinesAdded":3,"totalLinesRemoved":0}"#,
        ].join("\n")).unwrap();
        let stats = get_session_stats(path.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(stats.cost_usd, Some(150.0));
        assert_eq!((stats.lines_added, stats.lines_removed), (Some(15), Some(2)));
    }

    #[test]
    fn empty_thinking_blocks_are_not_counted() {
        let path = std::env::temp_dir().join(format!("cs_thinking_{}.jsonl", std::process::id()));
        fs::write(&path, r#"{"type":"assistant","requestId":"r","message":{"content":[{"type":"thinking","thinking":"","signature":"x"},{"type":"thinking","thinking":"real"}]}}"#).unwrap();
        let stats = get_session_stats(path.to_string_lossy().to_string()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(stats.thinking_block_count, 1);
    }

    #[test]
    fn tools_with_an_unexpected_shape_stay_visible_as_pills() {
        for (name, input) in [
            ("mcp__ccd_turn__update_plan", serde_json::json!({"steps":["x"]})),
            ("mcp__ccd_turn__wrap_up", serde_json::json!({"summary":"x"})),
            ("mcp__ccd_session__mark_chapter", serde_json::json!({"name":"x"})),
        ] {
            let block = serde_json::json!({"id":"t","name":name,"input":input});
            assert!(matches!(tool_block(&block, &Default::default(), &mut Vec::new()), Some(MessageBlock::Tool { .. })), "{name}");
        }
    }

    #[test]
    fn a_saved_index_is_used_until_the_file_changes_and_bad_ones_are_ignored() {
        // Rerun alone with its own cache folder: this saves a real index file.
        if std::env::var_os("EVERTUSK_INDEX_TEST").is_none() {
            let folder = TestFolder::new("index_test_home");
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "commands::tests::a_saved_index_is_used_until_the_file_changes_and_bad_ones_are_ignored", "--nocapture"])
                .env("EVERTUSK_INDEX_TEST", "1")
                .env("HOME", &folder.0)
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
            return;
        }
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let transcript = home.join("s.jsonl");
        fs::write(&transcript, "one").unwrap();
        let reads = std::cell::Cell::new(0);
        let read = |_: &Path| {
            reads.set(reads.get() + 1);
            Some(reads.get())
        };

        let first_run: FileIndex<i32> = FileIndex::new("test-index");
        assert_eq!(first_run.get_or_read(&transcript, read), Some(1));
        assert_eq!(first_run.get_or_read(&transcript, read), Some(1), "unchanged, so not read twice");
        first_run.save();

        let next_launch: FileIndex<i32> = FileIndex::new("test-index");
        assert_eq!(next_launch.get_or_read(&transcript, read), Some(1), "a new launch reuses the saved answer");
        assert_eq!(reads.get(), 1);

        fs::write(&transcript, "one and more").unwrap();
        assert_eq!(next_launch.get_or_read(&transcript, read), Some(2), "a changed file is read again");

        let location = first_run.location().unwrap();
        let other_build = fs::read_to_string(&location).unwrap().replacen(&serde_json::to_string(&build_identity()).unwrap(), "\"another build\"", 1);
        fs::write(&location, other_build).unwrap();
        let after_update: FileIndex<i32> = FileIndex::new("test-index");
        assert_eq!(after_update.get_or_read(&transcript, read), Some(3), "another build's index isn't trusted");

        fs::write(&location, "not json").unwrap();
        let after_damage: FileIndex<i32> = FileIndex::new("test-index");
        assert_eq!(after_damage.get_or_read(&transcript, read), Some(4), "a damaged index just costs a read");
    }

    #[test]
    fn binary_tool_output_is_described_rather_than_shown() {
        assert!(readable_output("\u{89}PNG\0\0data".to_string()).starts_with("[Binary output of"));
        assert_eq!(readable_output("plain text".to_string()), "plain text");
    }

    #[test]
    fn export_only_treats_the_artifact_tool_as_a_published_artifact() {
        let context = ExportContext {
            results: [("t".to_string(), ToolResultPayload { content: "page body".to_string(), is_error: false, persisted_path: None, image_count: 0 })].into_iter().collect(),
            questions: Default::default(),
            artifacts: [("t".to_string(), SessionArtifact { url: "https://claude.ai/artifact/x".to_string(), title: "Plan".to_string(), path: String::new() })].into_iter().collect(),
            full_inputs: Default::default(),
            included_agents: Default::default(),
            transcript: PathBuf::from("/nowhere/t.jsonl"),
            agent_label: "Claude",
            is_agent_log: false,
            cut_outputs: Default::default(),
        };
        let id = "t".to_string();
        assert!(render_tool_for_export("WebFetch", "url", "", Some(&id), None, &context).contains("page body"));
        assert!(render_tool_for_export("Artifact", "", "", Some(&id), None, &context).contains("Published artifact"));
    }

    #[test]
    fn a_single_attached_file_has_no_details_and_several_list_every_path() {
        let single = parse_lines("attach_one", &[
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"file","filename":"/a.md"}}"#,
        ]);
        assert_eq!(single[0].text, "Attached /a.md");
        assert_eq!(single[0].event.as_ref().unwrap().detail, "");
        let several = parse_lines("attach_two", &[
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"file","filename":"/a.md"}}"#,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"file","filename":"/b.md"}}"#,
        ]);
        assert_eq!(several.len(), 1);
        assert_eq!(several[0].text, "Attached 2 files");
        assert_eq!(several[0].event.as_ref().unwrap().detail, "/a.md\n/b.md");
    }

    #[test]
    fn expired_saved_sessions_are_searched_and_live_copies_win() {
        let base = std::env::temp_dir().join(format!("cs_search_archived_{}", std::process::id()));
        let line = |word: &str| format!(r#"{{"type":"user","timestamp":"t","message":{{"content":"please find {} here"}}}}
{{"type":"custom-title","customTitle":"Named"}}"#, word);
        let project = base.join("projects/-p-app");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("live-id.jsonl"), line("wombat")).unwrap();
        fs::create_dir_all(base.join("archive/expired-id")).unwrap();
        fs::create_dir_all(base.join("archive/live-id")).unwrap();
        fs::write(base.join("archive/expired-id/expired-id.jsonl"), line("wombat")).unwrap();
        fs::write(base.join("archive/live-id/live-id.jsonl"), line("wombat")).unwrap();
        let meta = r#"{"project_path":"/saved/app","project_name":"app"}"#;
        fs::write(base.join("archive/expired-id/meta.json"), meta).unwrap();
        fs::write(base.join("archive/live-id/meta.json"), meta).unwrap();

        let archived = archived_sessions_in(&base.join("archive"));
        let results = search_all_transcripts("wombat", Some(&base.join("projects")), &archived);
        let without_live_dir = search_all_transcripts("wombat", None, &archived);
        let _ = fs::remove_dir_all(&base);

        let mut ids: Vec<&str> = results.iter().map(|hit| hit.session_id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["expired-id", "live-id"], "the live copy of a saved session appears once");
        assert!(results.iter().find(|hit| hit.session_id == "expired-id").unwrap().jsonl_path.contains("archive"));
        assert_eq!(without_live_dir.len(), 2, "search works with no live projects folder");
    }

    /// A session folder of its own, removed when the test ends.
    struct TestFolder(PathBuf);

    impl TestFolder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("cs_{}_{}", name, std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TestFolder(path)
        }

        fn write(&self, relative: &str, lines: &[&str]) -> String {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, lines.join("\n")).unwrap();
            path.to_string_lossy().to_string()
        }
    }

    impl Drop for TestFolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn export_text(path: &str, title: Option<&str>, start_index: Option<usize>) -> String {
        let output = format!("{}.md", path);
        export_session_markdown(path.to_string(), output.clone(), title.map(String::from), start_index).unwrap();
        fs::read_to_string(&output).unwrap()
    }

    #[test]
    fn api_records_do_not_count_as_edits_or_reads() {
        let folder = TestFolder::new("api_records");
        let call = r#"{"type":"assistant","timestamp":"2026-09-01T10:00:00Z","message":{"content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/p/a.rs","old_string":"x","new_string":"y"}},{"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"/p/a.rs"}}]}}"#;
        let blob = r#"{"type":"api-request-blob","sessionId":"s","blob":{"messages":[{"content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/p/a.rs","old_string":"x","new_string":"y"}},{"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"/p/a.rs"}}]}]},"message":{"content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/p/a.rs"}}]}}"#;
        let changes = get_session_file_changes(folder.write("s.jsonl", &[call, blob])).unwrap();
        assert_eq!((changes[0].edit_count, changes[0].read_count), (1, 1));
    }

    #[test]
    fn rejected_edits_stay_listed_but_marked_and_writes_show_what_they_replaced() {
        let folder = TestFolder::new("edit_outcomes");
        let changes = get_session_file_changes(folder.write("s.jsonl", &[
            r#"{"type":"assistant","timestamp":"2026-09-01T10:00:02Z","message":{"content":[{"type":"tool_use","id":"bad","name":"Edit","input":{"file_path":"/p/a.rs","old_string":"x","new_string":"y"}}]}}"#,
            r#"{"type":"user","timestamp":"2026-09-01T10:00:03Z","toolUseResult":"Error: String to replace not found","message":{"content":[{"type":"tool_result","tool_use_id":"bad","is_error":true,"content":"String to replace not found"}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-09-01T10:00:04Z","message":{"content":[{"type":"tool_use","id":"over","name":"Write","input":{"file_path":"/p/a.rs","content":"new file"}}]}}"#,
            r#"{"type":"user","timestamp":"2026-09-01T10:00:05Z","toolUseResult":{"type":"update","filePath":"/p/a.rs","content":"new file","originalFile":"old file"},"message":{"content":[{"type":"tool_result","tool_use_id":"over","content":"ok"}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-09-01T10:00:06Z","message":{"content":[{"type":"tool_use","id":"fresh","name":"Write","input":{"file_path":"/p/b.rs","content":"created"}}]}}"#,
            r#"{"type":"user","timestamp":"2026-09-01T10:00:07Z","toolUseResult":{"type":"create","filePath":"/p/b.rs","content":"created","originalFile":null},"message":{"content":[{"type":"tool_result","tool_use_id":"fresh","content":"ok"}]}}"#,
        ])).unwrap();
        let file = |path: &str| changes.iter().find(|change| change.path == path).unwrap();
        assert_eq!(file("/p/a.rs").edits.len(), 2);
        assert_eq!(file("/p/a.rs").edit_count, 1, "only what was applied counts");
        assert!(file("/p/a.rs").edits[0].failed);
        assert_eq!(file("/p/a.rs").edits[1].old_string.as_deref(), Some("old file"));
        assert_eq!(file("/p/b.rs").edits[0].old_string, None, "a new file has nothing before it");
    }

    #[test]
    fn agent_edits_are_put_in_time_order() {
        let folder = TestFolder::new("edit_order");
        let edit = |time: &str, id: &str| format!(r#"{{"type":"assistant","timestamp":"{time}","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"Write","input":{{"file_path":"/p/a.rs","content":"x"}}}}]}}}}"#);
        let path = folder.write("s.jsonl", &[&edit("2026-09-01T12:00:00Z", "later")]);
        folder.write("s/subagents/agent-a.jsonl", &[&edit("2026-09-01T11:00:00Z", "earlier")]);
        let changes = get_session_file_changes(path).unwrap();
        let order: Vec<_> = changes[0].edits.iter().map(|edit| edit.tool_use_id.clone().unwrap()).collect();
        assert_eq!(order, ["earlier", "later"]);
    }

    #[test]
    fn session_dates_come_from_the_conversation_not_the_files_time() {
        let folder = TestFolder::new("dates");
        let path = folder.write("s.jsonl", &[
            r#"{"type":"user","timestamp":"2026-06-30T10:00:00Z","message":{"content":"hello"}}"#,
            r#"{"type":"system","subtype":"informational","timestamp":"2026-06-30T10:05:00Z","content":"note"}"#,
            r#"{"type":"file-history-snapshot","snapshot":{"timestamp":"2026-10-25T00:00:00Z"}}"#,
            r#"{"type":"mode","timestamp":"2026-10-25T00:00:00Z"}"#,
            r#"{"type":"assistant","timestamp":"not a time","message":{"content":[{"type":"text","text":"odd"}]}}"#,
        ]);
        let session = session_from_transcript(Path::new(&path), "/p");
        assert_eq!(session.modified.as_deref(), Some("2026-06-30T10:05:00Z"));
        assert_eq!(session.created.as_deref(), Some("2026-06-30T10:00:00Z"));
        assert!(session.last_written.is_some());
        assert_ne!(session.last_written, session.modified);
    }

    #[test]
    fn plan_progress_updates_apply_to_the_current_plan() {
        let plan = |input: &str| format!(r#"{{"type":"assistant","requestId":"r","timestamp":"t","message":{{"content":[{{"type":"tool_use","id":"p","name":"mcp__ccd_turn__update_plan","input":{input}}}]}}}}"#);
        let lines = [
            plan(r#"{"items":[{"content":"one","status":"in_progress"},{"content":"two","status":"pending"},{"content":"three","status":"pending"},{"content":"four","status":"pending"}]}"#),
            plan(r#"{"advance":true}"#),
            plan(r#"{"done":[2,3],"start":4}"#),
        ];
        let references: Vec<&str> = lines.iter().map(String::as_str).collect();
        let messages = parse_lines("plan_updates", &references);
        let plans: Vec<&Vec<PlanItem>> = messages[0].blocks.iter().filter_map(|block| match block {
            MessageBlock::Plan { items } => Some(items),
            _ => None,
        }).collect();
        assert_eq!(plans.len(), 1, "one plan that updates");
        assert_eq!(messages[0].blocks.len(), 1, "no raw pills");
        let statuses: Vec<&str> = plans[0].iter().map(|item| item.status.as_str()).collect();
        assert_eq!(statuses, ["completed", "completed", "completed", "in_progress"]);

        let mut finished = vec![PlanItem { content: "only".to_string(), status: "in_progress".to_string() }];
        assert!(apply_plan_update(&mut finished, &serde_json::json!({"advance": true})));
        assert_eq!(finished[0].status, "completed");
        assert!(!apply_plan_update(&mut [], &serde_json::json!({"advance": true})), "no plan to update stays a pill");
    }

    #[test]
    fn separate_background_runs_each_keep_a_card_and_redeliveries_do_not() {
        let note = |id: &str| format!("<task-notification>\n<task-id>{id}</task-id>\n<status>failed</status>\n<summary>Background command failed</summary>\n</task-notification>");
        let line = |id: &str| format!(r#"{{"type":"user","timestamp":"t","message":{{"content":{}}}}}"#, serde_json::to_string(&note(id)).unwrap());
        let (first, again, second) = (line("aaa"), line("aaa"), line("bbb"));
        let messages = parse_lines("task_ids", &[&first, &again, &second]);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn a_queued_prompt_survives_the_same_words_typed_later_but_a_copy_beside_it_does_not() {
        let queued = |text: &str| format!(r#"{{"type":"attachment","timestamp":"t","attachment":{{"type":"queued_command","prompt":"{text}","commandMode":"prompt"}}}}"#);
        let typed = |text: &str| format!(r#"{{"type":"user","timestamp":"t","message":{{"content":"{text}"}}}}"#);
        let reply = r#"{"type":"assistant","requestId":"r","timestamp":"t","message":{"content":[{"type":"text","text":"ok"}]}}"#;
        let later = [queued("whats happening"), reply.to_string(), typed("whats happening")];
        let references: Vec<&str> = later.iter().map(String::as_str).collect();
        let users: Vec<_> = parse_lines("queued_later", &references).into_iter().filter(|message| message.role == "user").collect();
        assert_eq!(users.len(), 2, "two messages, sent at different times");
        assert!(users[0].mid_turn && !users[1].mid_turn);

        let folder = TestFolder::new("queued_counts");
        let counted = extract_quick_metadata(Path::new(&folder.write("later.jsonl", &references)));
        assert_eq!(counted.conversation_count, 2);

        let beside = [queued("same delivery"), typed("same delivery")];
        let references: Vec<&str> = beside.iter().map(String::as_str).collect();
        let users = parse_lines("queued_beside", &references);
        assert_eq!(users.len(), 1, "one message stored twice");
        assert!(!users[0].mid_turn);
        let counted = extract_quick_metadata(Path::new(&folder.write("beside.jsonl", &references)));
        assert_eq!(counted.conversation_count, 1);
    }

    #[test]
    fn stats_separate_a_forks_own_work_from_what_it_inherited() {
        let folder = TestFolder::new("fork_stats");
        let path = folder.write("own.jsonl", &[
            r#"{"type":"user","sessionId":"own","forkedFrom":{"sessionId":"parent","messageUuid":"m"},"timestamp":"t","message":{"content":"copied prompt"}}"#,
            r#"{"type":"assistant","sessionId":"own","forkedFrom":{"sessionId":"parent"},"requestId":"r1","timestamp":"t","entrypoint":"cli","message":{"usage":{"input_tokens":5,"output_tokens":50},"content":[{"type":"tool_use","id":"a","name":"Bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"system","sessionId":"own","forkedFrom":{"sessionId":"parent"},"subtype":"turn_duration","durationMs":1000,"timestamp":"t"}"#,
            r#"{"type":"user","sessionId":"own","timestamp":"t","message":{"content":"my prompt"}}"#,
            r#"{"type":"assistant","sessionId":"own","requestId":"r2","timestamp":"t","entrypoint":"cli","message":{"usage":{"input_tokens":1,"output_tokens":10},"content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"type":"system","sessionId":"own","subtype":"turn_duration","durationMs":200,"timestamp":"t"}"#,
        ]);
        folder.write("own/subagents/agent-x.jsonl", &[
            r#"{"type":"assistant","requestId":"q","timestamp":"t","message":{"usage":{"input_tokens":3,"output_tokens":30},"content":[]}}"#,
        ]);
        let stats = get_session_stats(path.clone()).unwrap();
        assert_eq!((stats.output_tokens, stats.input_tokens, stats.active_ms), (10, 1, 200));
        assert_eq!((stats.user_prompt_count, stats.inherited_prompts), (1, 1));
        assert!(stats.tool_counts.is_empty(), "the Bash call was the parent's");
        assert_eq!((stats.agent_output_tokens, stats.agent_input_tokens), (30, 3));

        let messages = get_session_messages(path).unwrap();
        let shape: Vec<(&str, bool)> = messages.iter().map(|message| (message.role.as_str(), message.inherited)).collect();
        assert_eq!(shape, [("user", true), ("assistant", true), ("system-event", false), ("user", false), ("assistant", false)]);
        let marker = messages[2].event.as_ref().unwrap();
        assert_eq!((marker.kind.as_str(), marker.is_error), ("fork", false));
        assert_eq!(messages[2].text, "Forked from another session here");
    }

    #[test]
    fn a_continuation_copy_is_recognised_by_its_foreign_session_id() {
        let folder = TestFolder::new("continuation");
        let path = folder.write("copy.jsonl", &[
            r#"{"type":"user","sessionId":"original","timestamp":"t","message":{"content":"old prompt"}}"#,
            r#"{"type":"user","sessionId":"copy","timestamp":"t","message":{"content":"new prompt"}}"#,
        ]);
        assert_eq!(detect_fork_parent(Path::new(&path)).as_deref(), Some("original"));
        let messages = get_session_messages(path).unwrap();
        assert!(messages[0].inherited && !messages[2].inherited);
        assert_eq!(messages[1].event.as_ref().unwrap().kind, "fork");
    }

    #[test]
    fn a_subagents_log_is_not_mistaken_for_inherited_work() {
        let folder = TestFolder::new("agent_session_id");
        let path = folder.write("s/subagents/agent-a.jsonl", &[
            r#"{"type":"user","isSidechain":true,"sessionId":"s","timestamp":"t","message":{"content":"brief"}}"#,
            r#"{"type":"assistant","isSidechain":true,"sessionId":"s","requestId":"r","timestamp":"t","message":{"content":[{"type":"text","text":"done"}]}}"#,
        ]);
        let messages = get_subagent_messages(path).unwrap();
        assert_eq!(messages.len(), 2);
        assert!(messages.iter().all(|message| !message.inherited));
    }

    #[test]
    fn partial_activity_and_cost_are_flagged() {
        let folder = TestFolder::new("partial_stats");
        let desktop = folder.write("desktop.jsonl", &[
            r#"{"type":"assistant","requestId":"r","timestamp":"t","entrypoint":"claude-desktop","message":{"usage":{"input_tokens":1,"output_tokens":1000},"content":[]}}"#,
            r#"{"type":"cost-state","startTime":1,"totalCostUSD":0.5,"modelUsage":{"m":{"outputTokens":40}}}"#,
        ]);
        let stats = get_session_stats(desktop).unwrap();
        assert!(stats.active_is_partial && stats.cost_is_partial);

        let complete = folder.write("complete.jsonl", &[
            r#"{"type":"assistant","requestId":"r","timestamp":"t","entrypoint":"cli","message":{"usage":{"input_tokens":1,"output_tokens":1000},"content":[]}}"#,
            r#"{"type":"system","subtype":"turn_duration","durationMs":5,"timestamp":"t"}"#,
            r#"{"type":"cost-state","startTime":1,"totalCostUSD":5.0,"modelUsage":{"m":{"outputTokens":1200}}}"#,
        ]);
        let stats = get_session_stats(complete).unwrap();
        assert!(!stats.active_is_partial && !stats.cost_is_partial);
    }

    #[test]
    fn long_tool_input_is_cut_in_the_message_and_whole_on_request() {
        let folder = TestFolder::new("tool_input");
        let command = "x".repeat(TOOL_INPUT_LIMIT + 50);
        let call = format!(r#"{{"type":"assistant","requestId":"r","timestamp":"t","message":{{"content":[{{"type":"tool_use","id":"long","name":"Bash","input":{{"command":"{command}"}}}}]}}}}"#);
        let path = folder.write("s.jsonl", &[&call]);
        let messages = get_session_messages(path.clone()).unwrap();
        match &messages[0].blocks[0] {
            MessageBlock::Tool { input, input_truncated, .. } => {
                assert!(*input_truncated);
                assert!(input.len() < command.len());
            }
            other => panic!("expected a tool, got {other:?}"),
        }
        assert_eq!(get_tool_input(path.clone(), "long".to_string()).unwrap(), command);
        assert!(get_tool_input(path.clone(), "missing".to_string()).is_err());
        assert!(export_text(&path, None, None).contains(&command), "an export has the whole input");
    }

    #[test]
    fn interrupts_mark_the_reply_or_say_no_reply_had_started() {
        let reply = r#"{"type":"assistant","requestId":"r","timestamp":"t","message":{"content":[{"type":"text","text":"working"}]}}"#;
        let marker = r#"{"type":"user","timestamp":"t","message":{"content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#;
        let queued = r#"{"type":"attachment","timestamp":"t","attachment":{"type":"queued_command","prompt":"also this"}}"#;
        let prompt = r#"{"type":"user","timestamp":"t","message":{"content":"do it"}}"#;

        let written_after_queue = parse_lines("interrupt_queue", &[reply, queued, marker]);
        assert!(written_after_queue[0].interrupted, "the reply before the queued prompt");

        let before_reply = parse_lines("interrupt_early", &[reply, prompt, marker]);
        assert!(!before_reply[0].interrupted, "the earlier, finished reply isn't the one stopped");
        assert_eq!(before_reply[2].event.as_ref().unwrap().kind, "notice");
    }

    #[test]
    fn stop_hooks_recorded_twice_count_once() {
        let folder = TestFolder::new("stop_hooks");
        let attachment = r#"{"type":"attachment","timestamp":"t","attachment":{"type":"hook_success","hookEvent":"Stop","exitCode":0}}"#;
        let summary = r#"{"type":"system","subtype":"stop_hook_summary","hookCount":2,"hookErrors":[],"timestamp":"t"}"#;
        let stats = get_session_stats(folder.write("s.jsonl", &[attachment, attachment, summary])).unwrap();
        assert_eq!(stats.hooks.iter().find(|hook| hook.event == "Stop").unwrap().runs, 2);
    }

    #[test]
    fn compaction_cards_use_the_boundarys_time_and_lose_machine_tags() {
        let messages = parse_lines("compaction_tags", &[
            r#"{"type":"system","subtype":"compact_boundary","timestamp":"boundary","compactMetadata":{"trigger":"auto","preTokens":2000,"postTokens":1000}}"#,
            r#"{"type":"user","isCompactSummary":true,"timestamp":"much later","message":{"content":"<artifact-content-authored-by-others/>\nSummary:\n1. Primary request\n<system-reminder>\n</system-reminder>\nmore"}}"#,
            r#"{"type":"user","isCompactSummary":true,"timestamp":"own time","message":{"content":"</system-reminder>\nSecond summary"}}"#,
        ]);
        assert_eq!(messages[0].timestamp, "boundary");
        assert_eq!(messages[0].text, "Summary:\n1. Primary request\n\nmore");
        assert_eq!(messages[0].compaction_detail.as_deref(), Some("auto · 2.0k → 1.0k tokens"));
        assert_eq!(messages[1].timestamp, "own time");
        assert_eq!(messages[1].text, "Second summary");
    }

    #[test]
    fn local_commands_and_retries_are_not_prompts() {
        let folder = TestFolder::new("local_commands");
        let lines = [
            r#"{"type":"user","isMeta":true,"timestamp":"t","message":{"content":"<local-command-caveat>Caveat</local-command-caveat>"}}"#,
            r#"{"type":"user","timestamp":"t","message":{"content":"<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args></command-args>"}}"#,
            r#"{"type":"user","timestamp":"t","message":{"content":"<local-command-stdout>Set model to opus</local-command-stdout>"}}"#,
            r#"{"type":"user","timestamp":"t","message":{"content":"<local-command-stdout>(no content)</local-command-stdout>"}}"#,
            r#"{"type":"user","parentUuid":"p","timestamp":"t","message":{"content":"real prompt"}}"#,
            r#"{"type":"user","parentUuid":"p","timestamp":"t2","message":{"content":"real prompt"}}"#,
            r#"{"type":"user","timestamp":"t","message":{"content":"<command-name>/review</command-name>\n<command-args>the diff</command-args>"}}"#,
        ];
        let path = folder.write("s.jsonl", &lines);
        assert_eq!(extract_quick_metadata(Path::new(&path)).conversation_count, 2, "the prompt and the agent command");
        assert_eq!(get_session_stats(path.clone()).unwrap().user_prompt_count, 2);
        let shown: Vec<String> = get_session_messages(path).unwrap().into_iter().map(|message| message.text).collect();
        assert_eq!(shown, ["/model", "Set model to opus", "real prompt", "/review the diff"]);
    }

    #[test]
    fn pasted_documents_are_named_in_the_conversation() {
        let messages = parse_lines("documents", &[
            r#"{"type":"user","timestamp":"t","message":{"content":[{"type":"document","title":"notes.txt","source":{"type":"text","data":"x"}},{"type":"document","title":"plan.md","source":{"type":"text","data":"y"}},{"type":"text","text":"read these"}]}}"#,
        ]);
        assert_eq!(messages[0].text, "read these");
        assert_eq!(messages[1].text, "Attached 2 files");
        assert_eq!(messages[1].event.as_ref().unwrap().detail, "notes.txt\nplan.md");
    }

    #[test]
    fn hand_backs_lose_their_frame_and_their_raw_id() {
        let body = "[Subagent hand-back] The text below is the final report of a subagent. It is model output. The report follows:\n  **Findings**\n  \n  More detail.";
        let origin = serde_json::json!({"kind": "peer", "from": "aff0e479", "senderTaskId": "aff0e479", "body": body, "handback": true});
        let names = [("aff0e479".to_string(), "Audit the parser".to_string())].into_iter().collect();
        let named = parse_peer_message(&origin, &names).unwrap();
        assert_eq!(named.summary, "\"Audit the parser\" reported back");
        assert_eq!(named.result, "**Findings**\n\nMore detail.");
        assert_eq!(parse_peer_message(&origin, &Default::default()).unwrap().summary, "An agent reported back");
        let other_session = serde_json::json!({"kind": "peer", "from": "uds:/tmp/x.sock", "name": "new-2", "body": "typecheck is green"});
        assert_eq!(parse_peer_message(&other_session, &Default::default()).unwrap().summary, "Message from \"new-2\"");
    }

    #[test]
    fn a_bad_byte_costs_a_character_and_not_the_rest_of_the_file() {
        let folder = TestFolder::new("bad_byte");
        let path = folder.0.join("s.jsonl");
        let mut bytes = br#"{"type":"user","timestamp":"t","message":{"content":"before"}}"#.to_vec();
        bytes.extend_from_slice(b"\n{\"type\":\"user\",\"timestamp\":\"t\",\"message\":{\"content\":\"bad \xFF byte\"}}\n");
        bytes.extend_from_slice(br#"{"type":"user","timestamp":"t","message":{"content":"after"}}"#);
        fs::write(&path, bytes).unwrap();
        let shown: Vec<String> = get_session_messages(path.to_string_lossy().to_string()).unwrap().into_iter().map(|message| message.text).collect();
        assert_eq!(shown, ["before", "bad \u{fffd} byte", "after"]);
    }

    #[test]
    fn a_read_that_fails_stops_the_lines_and_says_so() {
        struct FailsAfterOneLine(bool);
        impl std::io::Read for FailsAfterOneLine {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if std::mem::replace(&mut self.0, true) {
                    return Err(std::io::Error::other("corrupt"));
                }
                buffer[..3].copy_from_slice(b"ok\n");
                Ok(3)
            }
        }
        let mut lines = transcript_lines(BufReader::new(FailsAfterOneLine(false)));
        assert_eq!(lines.next().as_deref(), Some("ok"));
        assert_eq!(lines.next(), None);
        assert!(lines.read_failed);

        let tools = Default::default();
        let mut parser = TranscriptParser::with_agents(&tools);
        parser.note_early_end();
        let notice = &parser.finish()[0];
        assert_eq!(notice.text, "This transcript ended early: the rest of the file couldn't be read.");
        assert!(notice.event.as_ref().unwrap().is_error);
    }

    #[test]
    fn a_wrong_typed_field_does_not_lose_the_entry() {
        let messages = parse_lines("wrong_types", &[
            r#"{"type":"user","timestamp":1790000000,"isMeta":"no","requestId":7,"message":{"content":"kept","model":12}}"#,
        ]);
        assert_eq!(messages[0].text, "kept");
        assert_eq!(messages[0].timestamp, "");
    }

    #[test]
    fn terminal_codes_and_control_characters_are_stripped() {
        assert_eq!(strip_ansi("Finished \u{1b}]8;;https://doc.rust-lang.org\u{7}link\u{1b}]8;;\u{7} done"), "Finished link done");
        assert_eq!(strip_ansi("a\u{1b}]0;title\u{1b}\\b"), "ab");
        assert_eq!(strip_ansi("\u{1b}[2mdim\u{1b}[0m\ttab\r\nnext\u{8}\u{7f}"), "dim\ttab\r\nnext");
        assert_eq!(strip_ansi("broken \u{1b}]8;;no end\nnext line"), "broken \nnext line", "a broken code can't eat what follows");
        assert_eq!(strip_ansi("keeps é and 日本語"), "keeps é and 日本語");
        assert_eq!(strip_ansi("\u{1b}7\u{1b}(Bshell"), "shell");
        assert_eq!(strip_ansi("cut \u{1b}… off"), "cut … off", "a lone ESC doesn't take real text with it");
    }

    #[test]
    fn body_text_stays_inside_its_block_in_an_export() {
        // An underline of tildes in a pasted error: nothing closes it, so it isn't a fence.
        assert_eq!(contained_markdown("error here\n~~~~\nmore text"), "error here\n\\~~~~\nmore text");
        // A real code block is left alone, whatever is in it.
        let code = "```html\n<style>x</style>\n# not a heading\n```";
        assert_eq!(contained_markdown(code), code);
        assert_eq!(contained_markdown("```\nnever closed\n## still text"), "\\```\nnever closed\n#### still text");
        // HTML shows rather than renders, except inside code.
        assert_eq!(contained_markdown("use <style> and `<b>` and 1 < 2"), "use \\<style> and `<b>` and 1 < 2");
        assert_eq!(contained_markdown("</details>"), "\\</details>");
        assert_eq!(contained_markdown("getOrThrow<number>('X')"), "getOrThrow\\<number>('X')");
        // Headings move below the file's own.
        assert_eq!(contained_markdown("# Title\n## Sub\n###### Deep\n#hashtag"), "### Title\n#### Sub\n###### Deep\n#hashtag");
        assert_eq!(contained_markdown("Title\n===\n\n---"), "Title\n\\===\n\n---");
    }

    #[test]
    fn inline_code_outgrows_any_backticks_inside_it() {
        assert_eq!(inline_code("plain"), "`plain`");
        assert_eq!(inline_code("has `one`"), "`` has `one` ``");
        assert_eq!(inline_code("a ``two`` b"), "```a ``two`` b```");
    }

    #[test]
    fn export_labels_the_tool_that_wrote_the_session() {
        let folder = TestFolder::new("export_label");
        let path = folder.write("rollout-2026-10-01T10-00-00-abc.jsonl", &[
            r#"{"timestamp":"2026-10-01T10:00:00Z","type":"session_meta","payload":{"id":"abc","cwd":"/work/app"}}"#,
        ]);
        let context = ExportContext::for_transcript(&path, &[], false).unwrap();
        assert_eq!(context.agent_label, "Codex");
        let question = AnsweredQuestion { question: "Which?".to_string(), header: String::new(), multi_select: false, options: Vec::new(), notes: None };
        let mut context = context;
        context.questions.insert("q".to_string(), vec![question]);
        let id = "q".to_string();
        assert!(render_tool_for_export("AskUserQuestion", "", "", Some(&id), None, &context).contains("**Codex asked:**"));

        let claude = folder.write("session.jsonl", &[
            r#"{"type":"assistant","requestId":"r","timestamp":"2026-10-01T10:00:00Z","message":{"content":[{"type":"text","text":"hi"}]}}"#,
        ]);
        assert!(export_text(&claude, None, None).contains("## Claude — "));
    }

    #[test]
    fn export_reads_saved_aside_output_and_never_leaks_its_wrapper() {
        let folder = TestFolder::new("export_persisted");
        let wrapper = |file: &str| format!("<persisted-output>\nOutput too large (30KB). Full output saved to: /Users/me/.claude/projects/x/s/tool-results/{file}\n\nPreview (first 2KB):\npreview text\n</persisted-output>");
        let result = |id: &str, file: &str| format!(r#"{{"type":"user","timestamp":"t","message":{{"content":[{{"type":"tool_result","tool_use_id":"{id}","content":{}}}]}}}}"#, serde_json::to_string(&wrapper(file)).unwrap());
        let call = |id: &str| format!(r#"{{"type":"assistant","requestId":"{id}","timestamp":"t","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"Bash","input":{{"command":"ls"}}}}]}}}}"#);
        let path = folder.write("s.jsonl", &[&call("kept"), &result("kept", "kept.txt"), &call("gone"), &result("gone", "gone.txt")]);
        folder.write("s/tool-results/kept.txt", &["the whole output"]);
        let output = export_text(&path, None, None);
        assert!(output.contains("the whole output"));
        assert!(output.contains("preview text\n\n[Full output is no longer on disk]"));
        assert!(!output.contains("<persisted-output>"));
        assert!(!output.contains("/Users/me"));
    }

    #[test]
    fn export_keeps_agent_reports_whole_states_the_cap_and_includes_agent_transcripts() {
        let folder = TestFolder::new("export_agents");
        let long_report = "r".repeat(EXPORT_OUTPUT_LIMIT + 10);
        let long_output = "o".repeat(EXPORT_OUTPUT_LIMIT + 10);
        let notification = format!(r#"{{"type":"user","timestamp":"t","message":{{"content":"<task-notification><status>completed</status><summary>Agent done</summary><result>{long_report}</result></task-notification>"}}}}"#);
        let bash_call = r#"{"type":"assistant","requestId":"r1","timestamp":"t","message":{"content":[{"type":"tool_use","id":"b","name":"mcp__team-db__query","input":{"sql":"select 1"}}]}}"#;
        let bash_result = format!(r#"{{"type":"user","timestamp":"t","message":{{"content":[{{"type":"tool_result","tool_use_id":"b","content":"{long_output}"}}]}}}}"#);
        let agent_call = r#"{"type":"assistant","requestId":"r2","timestamp":"t","message":{"content":[{"type":"tool_use","id":"a","name":"Agent","input":{"description":"Audit it","prompt":"look"}}]}}"#;
        let agent_result = r#"{"type":"user","timestamp":"t","toolUseResult":{"agentId":"abc"},"message":{"content":[{"type":"tool_result","tool_use_id":"a","content":"agent says done"}]}}"#;
        let path = folder.write("s.jsonl", &[&notification, bash_call, &bash_result, agent_call, agent_result]);
        folder.write("s/subagents/agent-abc.jsonl", &[
            r#"{"type":"user","isSidechain":true,"timestamp":"t","message":{"content":"the brief"}}"#,
            r#"{"type":"assistant","isSidechain":true,"requestId":"q","timestamp":"t","message":{"content":[{"type":"text","text":"inner agent words"}]}}"#,
        ]);
        folder.write("s/subagents/agent-abc.meta.json", &[r#"{"agentType":"general-purpose","description":"Audit it"}"#]);
        let output = export_text(&path, Some("A title"), None);
        assert!(output.contains(&long_report), "agent reports aren't capped");
        assert!(!output.contains(&long_output), "other output is");
        assert!(output.contains("[…10 more characters not exported]"));
        let cap_note = output.find("is cut in this file (1 places)").expect("the cap is stated");
        assert!(cap_note < output.find("## ").unwrap(), "at the top");
        assert!(output.contains("**team-db / query**"), "tool names as on screen");
        assert!(output.contains("_Agent abc: transcript at the end of this file._"));
        assert!(output.contains("<summary>Audit it · abc</summary>"));
        assert!(output.contains("## Brief"));
        assert!(output.contains("## Agent — "));
        assert!(output.contains("inner agent words"));

        let partial = export_text(&path, None, Some(1));
        assert!(partial.contains("inner agent words"), "the agent is called in the exported part");
        assert!(partial.contains("_Partial export: from t to the end of the session._"));
        let after_the_call = export_text(&path, None, Some(2));
        assert!(!after_the_call.contains("inner agent words"), "not called in the exported part");
    }

    #[test]
    fn export_matches_the_screen_on_hooks_labels_times_and_binary_output() {
        let folder = TestFolder::new("export_screen");
        let path = folder.write("s.jsonl", &[
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"hook_success","hookEvent":"SessionStart","hookName":"SessionStart","content":"injected instructions"}}"#,
            r#"{"type":"attachment","timestamp":"t","attachment":{"type":"hook_non_blocking_error","hookEvent":"Stop","hookName":"Stop","stderr":"real failure","exitCode":1}}"#,
            r#"{"type":"system","subtype":"api_error","timestamp":"t","error":{"message":"overloaded"}}"#,
            r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-09-10T07:59:56Z","compactMetadata":{"trigger":"auto","preTokens":967800,"postTokens":53600}}"#,
            r#"{"type":"user","isCompactSummary":true,"timestamp":"t","message":{"content":"summary"}}"#,
            r#"{"type":"assistant","requestId":"r","timestamp":"2026-09-10T07:59:56.478Z","message":{"content":[{"type":"tool_use","id":"png","name":"Bash","input":{"command":"cat a.png"}}]}}"#,
            r#"{"type":"user","timestamp":"t","message":{"content":[{"type":"tool_result","tool_use_id":"png","content":"PNG\u0000\u0001bytes"}]}}"#,
        ]);
        let output = export_text(&path, None, None);
        assert!(!output.contains("injected instructions"));
        assert!(output.contains("real failure"));
        assert!(output.contains("**API error:** overloaded"));
        assert!(output.contains("Context compacted (auto · 967.8k → 53.6k tokens)"));
        assert!(output.contains("[Binary output of"));
        assert!(!output.contains('\0'));
        let local_hour = chrono::DateTime::parse_from_rfc3339("2026-09-10T07:59:56Z").unwrap().with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string();
        assert!(output.contains(&format!("## Claude — {}", local_hour)));
        assert!(!output.contains("07:59:56.478Z"));
    }

    #[test]
    fn search_finds_words_with_quotes_and_backslashes_and_project_names() {
        let folder = TestFolder::new("search_escapes");
        folder.write("projects/-work-app/s.jsonl", &[
            r#"{"type":"assistant","requestId":"r","timestamp":"2026-09-01T10:00:00Z","message":{"content":[{"type":"text","text":"set \"session_id\" in C:\\temp\\thing"}]}}"#,
        ]);
        let projects = folder.0.join("projects");
        let hits = |query: &str| search_all_transcripts(query, Some(&projects), &[]);
        assert_eq!(hits("\"session_id\"").len(), 1);
        assert_eq!(hits("C:\\temp\\thing").len(), 1);
        assert_eq!(hits("\"missing\"").len(), 0);
        let by_folder = hits("app");
        assert_eq!(by_folder.len(), 1, "the folder's name matches, as it does while typing");
        assert_eq!(by_folder[0].match_source, "session_name");
    }

    #[test]
    fn snippets_stay_on_the_match_when_lowercasing_changes_length() {
        let text = format!("{}needle after", "İ".repeat(85));
        let snippet = match_snippet(&text, &["needle"]);
        assert!(snippet.contains("needle after"), "got {snippet:?}");
    }

    #[test]
    fn duplicate_sessions_and_agents_are_dropped_and_the_newest_kept() {
        let session = |id: &str, path: &str, modified: &str| SessionInfo {
            session_id: id.to_string(),
            tool: "claude".to_string(),
            archived_in_tool: false,
            summary: None,
            custom_title: None,
            ai_title: None,
            first_prompt: None,
            project_path: String::new(),
            project_name: String::new(),
            created: None,
            modified: Some(modified.to_string()),
            last_written: None,
            conversation_count: 0,
            git_branch: None,
            forked_from_session_id: None,
            jsonl_path: path.to_string(),
        };
        let kept = without_duplicates(vec![session("a", "/one/a.jsonl", "new"), session("a", "/two/a.jsonl", "old"), session("b", "/one/a.jsonl", "old")]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].jsonl_path, "/one/a.jsonl");

        let agent = || SubagentInfo { agent_id: "same".to_string(), agent_type: None, description: None, jsonl_path: String::new(), tool_use_id: None, workflow_run: None };
        assert_eq!(without_duplicate_agents(vec![agent(), agent()]).len(), 1);
    }

    #[test]
    fn project_paths_are_normalized_and_home_is_a_real_parent() {
        assert_eq!(normalize_project_path("/work/app/"), "/work/app");
        assert_eq!(normalize_project_path("/"), "/");
        assert_eq!(normalize_project_path(""), "");
        assert_eq!(project_name_of("/work/app"), "app");
        assert_eq!(project_name_of("/"), "/", "the root still has a name");
        assert_eq!(home_relative_path("/Users/me/app", "/Users/me"), "~/app");
        assert_eq!(home_relative_path("/Users/me", "/Users/me"), "~");
        assert_eq!(home_relative_path("/Users/me-work/app", "/Users/me"), "/Users/me-work/app");
        assert_eq!(home_relative_path("/app", ""), "/app");
        assert_eq!(encode_project_path("/a/🚀"), "-a---", "an emoji is two UTF-16 units");
    }

    #[test]
    fn only_files_count_as_transcripts() {
        let folder = TestFolder::new("folder_named_jsonl");
        fs::create_dir_all(folder.0.join("-p/x.jsonl")).unwrap();
        folder.write("-p/real.jsonl", &["{}"]);
        assert!(!is_transcript_file(&folder.0.join("-p/x.jsonl")));
        assert!(is_transcript_file(&folder.0.join("-p/real.jsonl")));
    }

    #[test]
    fn locked_folders_and_files_are_reported_in_plain_words() {
        use std::os::unix::fs::PermissionsExt;
        let folder = TestFolder::new("read_problems");
        assert!(claude_read_problems(&folder.0.join("missing")).is_empty(), "not installed is fine");
        folder.write("locked_home/projects/-p/s.jsonl", &["{}"]);
        let file = folder.write("projects/-p/s.jsonl", &["{}"]);
        folder.write("projects/-locked/s.jsonl", &["{}"]);
        let projects = folder.0.join("projects");
        assert!(claude_read_problems(&projects).is_empty());

        let set_mode = |path: &Path, mode: u32| fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        set_mode(&folder.0.join("locked_home"), 0o000);
        let home_problems = claude_read_problems(&folder.0.join("locked_home/projects"));
        set_mode(&folder.0.join("locked_home"), 0o755);
        assert_eq!(home_problems.len(), 1, "a locked parent isn't the same as not installed");

        set_mode(Path::new(&file), 0o000);
        set_mode(&projects.join("-locked"), 0o000);
        let problems = claude_read_problems(&projects);
        set_mode(Path::new(&file), 0o644);
        set_mode(&projects.join("-locked"), 0o755);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|problem| problem.contains("doesn't have permission") && problem.contains("-locked")));
        assert!(problems.iter().any(|problem| problem.starts_with("1 Claude Code session couldn't be opened")));
        assert!(!problems.iter().any(|problem| problem.contains("os error")));
    }

    #[test]
    fn a_damaged_live_file_cannot_replace_a_good_saved_copy() {
        let folder = TestFolder::new("saved_guard");
        let saved_copy = folder.write("archive/s/s.jsonl", &["{\"type\":\"user\"}", "{\"type\":\"assistant\"}"]);
        let good_text = fs::read_to_string(&saved_copy).unwrap();
        let root = folder.0.join("archive");
        std::thread::sleep(std::time::Duration::from_millis(20));

        let truncated = folder.write("live/s.jsonl", &["{}"]);
        archive_session_to(&root, Path::new(&truncated), "s", None, &Value::Null).unwrap();
        assert_eq!(fs::read_to_string(&saved_copy).unwrap(), good_text, "shrank");

        let corrupt = folder.write("live/s.jsonl", &["not json at all, and longer than the saved copy was, by quite a lot of bytes"]);
        archive_session_to(&root, Path::new(&corrupt), "s", None, &Value::Null).unwrap();
        assert_eq!(fs::read_to_string(&saved_copy).unwrap(), good_text, "corrupt");

        let grown = folder.write("live/s.jsonl", &["{\"type\":\"user\"}", "{\"type\":\"assistant\"}", "{\"type\":\"user\",\"more\":1}"]);
        archive_session_to(&root, Path::new(&grown), "s", None, &Value::Null).unwrap();
        assert!(fs::read_to_string(&saved_copy).unwrap().contains("more"), "real growth still saves");
        assert!(fs::read_dir(root.join("s")).unwrap().flatten().all(|entry| !entry.file_name().to_string_lossy().contains("tmp")), "no temporary left behind");
    }

    #[test]
    fn saved_copies_do_not_light_the_running_lamp_and_find_their_folder() {
        let folder = TestFolder::new("saved_rows");
        folder.write("archive/s/s.jsonl", &[
            r#"{"type":"user","cwd":"/work/app/","timestamp":"2026-09-01T10:00:00Z","message":{"content":"hi"}}"#,
            r#"{"type":"assistant","timestamp":"garbage","message":{"content":[]}}"#,
        ]);
        let sessions = archived_sessions_in(&folder.0.join("archive"));
        assert_eq!(sessions[0].project_path, "/work/app");
        assert_eq!(sessions[0].project_name, "app");
        assert_eq!(sessions[0].last_written, None);
        assert_eq!(sessions[0].modified.as_deref(), Some("2026-09-01T10:00:00Z"), "a garbage time can't date the session");
    }

    #[test]
    fn reveal_refuses_anything_but_an_existing_file() {
        let folder = TestFolder::new("reveal");
        assert!(reveal_in_finder(folder.0.join("missing.md").to_string_lossy().to_string()).is_err());
        assert!(reveal_in_finder(folder.0.to_string_lossy().to_string()).is_err(), "a folder");
        assert!(reveal_in_finder("-a".to_string()).is_err(), "a relative path can't pose as a flag");
    }

    #[test]
    fn local_images_are_read_only_when_they_are_images() {
        let folder = TestFolder::new("local_image");
        let image = folder.write("shot.png", &["bytes"]);
        let secret = folder.write("secret.txt", &["bytes"]);
        assert!(read_local_image(image.clone()).unwrap().starts_with("data:image/png;base64,"));
        assert!(read_local_image(format!("file://{}", image)).is_some());
        assert_eq!(read_local_image(secret), None);
        assert_eq!(read_local_image("relative/shot.png".to_string()), None);
        // Tests run from src-tauri, so this one exists and is a real image.
        assert!(Path::new("icons/32x32.png").exists());
        assert_eq!(read_local_image("icons/32x32.png".to_string()), None, "only absolute paths, even for a real file");
    }

    /// Listing and search across tools read the real home folder. A test that uses them runs in a
    /// child process pointed at a made-up one, so neither the owner's history nor other tests' threads
    /// are involved. The body runs in the child; the fixture is laid out by the parent.
    fn in_fake_home(test_name: &str, lay_out: impl FnOnce(&TestFolder), body: impl FnOnce()) {
        if std::env::var_os("EVERTUSK_FAKE_HOME").is_some() {
            body();
            return;
        }
        let home = TestFolder::new(test_name);
        lay_out(&home);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &format!("commands::tests::{test_name}"), "--nocapture"])
            .env("HOME", &home.0)
            .env("CODEX_HOME", home.0.join(".codex"))
            .env("EVERTUSK_FAKE_HOME", "1")
            .output()
            .unwrap();
        let report = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success() && report.contains("1 passed"), "{report}");
    }

    fn claude_transcript(cwd: &str, text: &str) -> String {
        format!(r#"{{"type":"user","cwd":"{cwd}","timestamp":"2026-09-01T10:00:00Z","message":{{"content":"{text}"}}}}"#)
    }

    fn codex_rollout(id: &str, cwd: &str, source: &str, text: &str) -> [String; 2] {
        [
            format!(r#"{{"timestamp":"2026-10-01T10:00:00Z","type":"session_meta","payload":{{"id":"{id}","cwd":"{cwd}","source":{source}}}}}"#),
            format!(r#"{{"timestamp":"2026-10-01T10:00:01Z","type":"event_msg","payload":{{"type":"item_completed","item":{{"type":"UserMessage","id":"u","content":[{{"type":"text","text":"{text}"}}]}}}}}}"#),
        ]
    }

    /// Claude and Codex in /work/app, Claude alone in /work/other, and a Codex helper that must stay hidden.
    fn two_tools(home: &TestFolder) {
        home.write(".claude/projects/-work-app/aaaaaaaa-0000-0000-0000-000000000001.jsonl", &[&claude_transcript("/work/app", "claude needle")]);
        home.write(".claude/projects/-work-other/aaaaaaaa-0000-0000-0000-000000000002.jsonl", &[&claude_transcript("/work/other", "other needle")]);
        let main = codex_rollout("bbbbbbbb-0000-0000-0000-000000000001", "/work/app", r#""vscode""#, "codex needle");
        home.write(".codex/sessions/2026/10/01/rollout-2026-10-01T10-00-00-bbbbbbbb-0000-0000-0000-000000000001.jsonl", &[&main[0], &main[1]]);
        let helper = codex_rollout("cccccccc-0000-0000-0000-000000000001", "/work/app", r#"{"subagent":{"thread_spawn":{"parent_thread_id":"bbbbbbbb-0000-0000-0000-000000000001"}}}"#, "helper needle");
        home.write(".codex/sessions/2026/10/01/rollout-2026-10-01T10-00-00-cccccccc-0000-0000-0000-000000000001.jsonl", &[&helper[0], &helper[1]]);
    }

    #[test]
    fn a_folder_used_by_two_tools_is_one_project_counting_both() {
        in_fake_home("a_folder_used_by_two_tools_is_one_project_counting_both", two_tools, || {
            let projects = get_projects().unwrap();
            let shared = projects.iter().find(|project| project.project_path == "/work/app").unwrap();
            assert_eq!(shared.session_count, 2);
            let mut tools = shared.tools.clone();
            tools.sort();
            assert_eq!(tools, ["claude", "codex"]);
            assert_eq!(projects.iter().find(|project| project.project_path == "/work/other").unwrap().session_count, 1);
            assert_eq!(projects.len(), 2);
        });
    }

    #[test]
    fn a_session_copied_to_another_folder_is_listed_once() {
        let lay_out = |home: &TestFolder| {
            let line = claude_transcript("/work/app", "same session");
            home.write(".claude/projects/-work-app/dddddddd-0000-0000-0000-000000000001.jsonl", &[&line]);
            home.write(".claude/projects/-work-copy/dddddddd-0000-0000-0000-000000000001.jsonl", &[&line]);
        };
        in_fake_home("a_session_copied_to_another_folder_is_listed_once", lay_out, || {
            assert_eq!(scan_projects().unwrap().len(), 1);
        });
    }

    #[test]
    fn helpers_stay_out_of_the_session_list() {
        in_fake_home("helpers_stay_out_of_the_session_list", two_tools, || {
            let sessions = scan_projects().unwrap();
            assert_eq!(sessions.len(), 3);
            assert!(!sessions.iter().any(|session| session.session_id.starts_with("cccccccc")));
        });
    }

    #[test]
    fn the_tool_filter_narrows_search_to_that_tool() {
        in_fake_home("the_tool_filter_narrows_search_to_that_tool", two_tools, || {
            let tools_found = |tool: Option<&str>| {
                let mut found: Vec<String> = search_every_tool("needle", tool, &[]).into_iter().map(|hit| hit.tool).collect();
                found.sort();
                found
            };
            assert_eq!(tools_found(None), ["claude", "claude", "codex"], "and helpers aren't searched");
            assert_eq!(tools_found(Some("claude")), ["claude", "claude"]);
            assert_eq!(tools_found(Some("codex")), ["codex"]);
        });
    }

    #[test]
    fn a_search_gone_stale_stops_in_both_tools() {
        in_fake_home("a_search_gone_stale_stops_in_both_tools", two_tools, || {
            let projects = get_claude_projects_dir().unwrap();
            assert_eq!(search_transcripts_unless("needle", Some(&projects), &[], &|| false).len(), 2);
            assert!(search_transcripts_unless("needle", Some(&projects), &[], &|| true).is_empty());
            assert_eq!(codex::search(&["needle"], &|| false).len(), 1);
            assert!(codex::search(&["needle"], &|| true).is_empty());
        });
    }

    #[test]
    fn a_new_search_stops_the_one_before_it() {
        // Big enough that the older search is still reading when the newer one starts.
        let lay_out = |home: &TestFolder| {
            let line = claude_transcript("/work/app", &format!("needle {}", "padding ".repeat(50)));
            let lines = vec![line.as_str(); 1500];
            for index in 0..60 {
                home.write(&format!(".claude/projects/-work-app/{index}.jsonl"), &lines);
            }
        };
        in_fake_home("a_new_search_stops_the_one_before_it", lay_out, || {
            // ponytail: timing-based, because the staleness check is private to search_every_tool. Retried, so it can only fail if cancellation never fires.
            let cancelled = (0..20).any(|_| {
                let older = std::thread::spawn(|| search_every_tool("needle", Some("claude"), &[]));
                std::thread::sleep(std::time::Duration::from_millis(1));
                let newer = search_every_tool("needle", Some("claude"), &[]);
                assert_eq!(newer.len(), 50, "the newest search always finishes");
                older.join().unwrap().is_empty()
            });
            assert!(cancelled, "the older search kept going to the end");
        });
    }

    #[test]
    fn long_tool_output_is_cut_at_the_cap_and_says_so() {
        let folder = TestFolder::new("output_cap");
        let small = folder.write("small.txt", &["all of it"]);
        assert_eq!(read_capped_text(Path::new(&small)).unwrap(), "all of it");
        let large = folder.0.join("large.txt");
        const CAP: u64 = 5 * 1024 * 1024;
        fs::File::create(&large).unwrap().set_len(CAP + 10).unwrap();
        let text = read_capped_text(&large).unwrap();
        assert!(text.ends_with(&format!("[…truncated; file is {} bytes total]", CAP + 10)));
        assert!((text.len() as u64) < CAP + 100, "the rest isn't read");
    }

    #[test]
    fn the_list_notices_a_file_that_changed() {
        let folder = TestFolder::new("metadata_cache");
        let first = claude_transcript("/work/app", "one");
        let second = claude_transcript("/work/app", "two");
        let path = folder.write("-work-app/s.jsonl", &[&first]);
        let metadata = || extract_quick_metadata(Path::new(&path));
        assert_eq!(metadata().conversation_count, 1);
        folder.write("-work-app/s.jsonl", &[&first, &second]);
        assert_eq!(metadata().conversation_count, 2, "grew");
        // The same size with a new time still counts as a change.
        let size_before = fs::metadata(&path).unwrap().len();
        fs::write(&path, format!("{}\n{}", first.replace("one", "new"), second)).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), size_before);
        let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        fs::File::options().write(true).open(&path).unwrap().set_modified(earlier).unwrap();
        assert_eq!(metadata().first_prompt.as_deref(), Some("new"));
    }

    #[test]
    fn hook_failures_are_counted_from_exit_codes_and_failure_types() {
        let folder = TestFolder::new("hook_failures");
        let hook = |kind: &str, exit_code: i32| format!(r#"{{"type":"attachment","timestamp":"t","attachment":{{"type":"{kind}","hookEvent":"PostToolUse","exitCode":{exit_code}}}}}"#);
        let lines = [hook("hook_success", 0), hook("hook_success", 1), hook("hook_blocking_error", 0), hook("hook_cancelled", 0)];
        let path = folder.write("s.jsonl", &lines.iter().map(String::as_str).collect::<Vec<_>>());
        let stats = get_session_stats(path).unwrap();
        let tally = stats.hooks.iter().find(|hook| hook.event == "PostToolUse").unwrap();
        assert_eq!((tally.runs, tally.failures), (4, 3));
    }

    #[test]
    fn an_older_live_file_does_not_overwrite_a_newer_saved_copy() {
        let folder = TestFolder::new("archive_newer");
        let root = folder.0.join("archive");
        let live = folder.write("live/s.jsonl", &["{\"live\":1}", "{\"longer\":\"than the saved copy\"}"]);
        let saved_copy = folder.write("archive/s/s.jsonl", &["{\"saved\":1}"]);
        let set_age = |path: &str, seconds: u64| {
            let time = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds);
            fs::File::options().write(true).open(path).unwrap().set_modified(time).unwrap();
        };
        set_age(&live, 3600);
        archive_session_to(&root, Path::new(&live), "s", None, &Value::Null).unwrap();
        assert_eq!(fs::read_to_string(&saved_copy).unwrap(), "{\"saved\":1}");
        set_age(&saved_copy, 7200);
        archive_session_to(&root, Path::new(&live), "s", None, &Value::Null).unwrap();
        assert!(fs::read_to_string(&saved_copy).unwrap().contains("longer"), "newer now, so it's copied");
    }
}
