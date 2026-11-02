//! Reads OpenAI Codex sessions into the same structures as Claude Code sessions, so
//! everything above this file (the UI, search, export) handles both.
//!
//! Sessions are `~/.codex/{sessions,archived_sessions}/**/rollout-*.jsonl[.zst]`. Each line
//! is `{timestamp, type, payload}`. The format changed over time, and every session mixes
//! two records: `item_completed` events and raw `response_item` calls. Messages always have
//! items. Tools may have only a raw call (most sessions before about August 2026), an item
//! with the same id, or items nested inside an `exec` script wrapper. `parse_session`
//! turns all of that into one list of events, and everything else reads from that. Thread
//! names come from `session_index.jsonl`. Unknown `item_completed` items still show up
//! rather than vanishing; unknown raw `response_item`s (bookkeeping such as `additional_tools`)
//! don't.
//!
//! Guardian threads (`source.subagent` without a `thread_spawn`) have no parent and are
//! helpers, not conversations, so they appear neither in the list nor under Agents.

use super::*;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::time::SystemTime;

/// Tags Codex wraps around context it feeds the model, not things the person typed.
const INJECTED_TAGS: &[&str] = &[
    "environment_context",
    "recommended_plugins",
    "turn_aborted",
    "external_codex_apps_open_page",
];

/// Largest pasted file shown inside a prompt.
const MAX_ATTACHMENT_BYTES: u64 = 2 * 1024 * 1024;

pub(super) fn is_codex_path(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("rollout-"))
}

/// `rollout-<time>-<uuid>.jsonl[.zst]` → the uuid, which is the thread id.
pub(super) fn session_id_from_path(path: &Path) -> String {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let stem = name.trim_end_matches(".zst").trim_end_matches(".jsonl");
    let start = stem.char_indices().rev().nth(35).map_or(0, |(index, _)| index);
    stem[start..].to_string()
}

fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
}

/// A transcript's lines, decompressing as they're read so a scan can stop early. A bad byte
/// costs only its own character. `ended_early` is set if the file couldn't be read to its end.
struct TranscriptLines {
    reader: BufReader<Box<dyn Read>>,
    ended_early: bool,
}

impl Iterator for TranscriptLines {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        let mut bytes = Vec::new();
        match self.reader.read_until(b'\n', &mut bytes) {
            Ok(0) => None,
            Ok(_) => {
                let text = String::from_utf8_lossy(&bytes);
                Some(text.trim_end_matches(['\n', '\r']).to_string())
            }
            Err(_) => {
                self.ended_early = true;
                None
            }
        }
    }
}

fn read_lines(path: &Path) -> Result<TranscriptLines, String> {
    let file = fs::File::open(path).map_err(|open_error| format!("Cannot open file: {}", open_error))?;
    let reader: Box<dyn Read> = if path.extension().is_some_and(|extension| extension == "zst") {
        Box::new(zstd::stream::read::Decoder::new(file).map_err(|decode_error| format!("Cannot decompress: {}", decode_error))?)
    } else {
        Box::new(file)
    };
    Ok(TranscriptLines { reader: BufReader::new(reader), ended_early: false })
}

fn text_field<'value>(value: &'value Value, key: &str) -> &'value str {
    value.get(key).and_then(|field| field.as_str()).unwrap_or("")
}

/// The clean text of a UserMessage or AgentMessage item's content blocks. Goes by the field,
/// not the type name: the two items spell it differently ("text" and "Text").
fn content_text(item: &Value) -> String {
    item.get("content")
        .and_then(|content| content.as_array())
        .into_iter()
        .flatten()
        .filter_map(|block| block.get("text").and_then(|text| text.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn clean_user_text(text: &str) -> String {
    let without_context = strip_paired_tags(&strip_browser_context(text), INJECTED_TAGS, false);
    request_from_wrapper(&strip_system_tags(&without_context))
}

/// `<in-app-browser-context source="...">` carries an attribute, which `strip_paired_tags`
/// doesn't match.
fn strip_browser_context(text: &str) -> String {
    const OPEN: &str = "<in-app-browser-context";
    const CLOSE: &str = "</in-app-browser-context>";
    let mut output = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start) = remaining.find(OPEN) {
        let Some(length) = remaining[start..].find(CLOSE) else { break };
        output.push_str(&remaining[..start]);
        remaining = &remaining[start + length + CLOSE.len()..];
    }
    output.push_str(remaining);
    output
}

/// Codex's desktop app puts attachments ahead of what the person asked:
/// `# Files pasted by the user:`, a `## name: /path` per file, then `## My request:`. The
/// request leads here and the files follow. A pasted file can *be* the request, leaving the
/// request section empty, so its text is what's shown.
fn request_from_wrapper(text: &str) -> String {
    const WRAPPERS: &[&str] = &["# Files mentioned by the user:", "# Files pasted by the user:", "# Applications mentioned by the user:", "## Referenced ChatGPT conversation:", "## My request"];
    if !WRAPPERS.iter().any(|wrapper| text.starts_with(wrapper)) {
        return text.to_string();
    }
    let marker = ["## My request for Codex:", "## My request:"]
        .iter()
        .filter_map(|marker| text.find(marker).map(|start| (start, marker.len())))
        .min();
    let Some((marker_start, marker_length)) = marker else { return text.to_string() };
    let mut parts: Vec<String> = Vec::new();
    let request = text[marker_start + marker_length..].trim();
    if !request.is_empty() {
        parts.push(request.to_string());
    }
    let root = codex_home().map(|home| home.join("attachments")).unwrap_or_default();
    let mut in_files = false;
    let mut head_lines = text[..marker_start].lines().peekable();
    while let Some(line) = head_lines.next() {
        if line.starts_with("# ") {
            in_files = line.starts_with("# Files");
        } else if let Some(app) = line.strip_prefix("<appshot app=\"").and_then(|rest| rest.split('"').next()) {
            parts.push(format!("Mentioned app: {}", app));
        } else if let Some((label, file_path)) = line.strip_prefix("## ").filter(|_| in_files).and_then(|entry| entry.rfind(": /").map(|at| (&entry[..at], &entry[at + 2..]))) {
            // Images are in the message's own image blocks.
            if head_lines.peek() == Some(&"Image attachment: true") {
                continue;
            }
            match read_attachment(&root, Path::new(file_path)) {
                Some(content) if parts.is_empty() => parts.push(content),
                Some(content) => parts.push(format!("Pasted file {}:\n{}", label, content)),
                None => parts.push(format!("Attached file: {}", file_path)),
            }
        }
    }
    parts.join("\n\n")
}

/// Only from Codex's own attachments folder, whatever a transcript names: it decides what
/// gets read off the disk.
fn read_attachment(root: &Path, file_path: &Path) -> Option<String> {
    let real_path = file_path.canonicalize().ok()?;
    if !real_path.starts_with(root.canonicalize().ok()?) || fs::metadata(&real_path).ok()?.len() > MAX_ATTACHMENT_BYTES {
        return None;
    }
    Some(String::from_utf8_lossy(&fs::read(real_path).ok()?).trim().to_string())
}

/// A timestamp that means something, in UTC so they sort as text. Anything else is treated
/// as no timestamp, so it can't reach the screen as "Invalid Date".
fn clean_timestamp(raw: &str) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
    if raw.ends_with('Z') {
        return Some(raw.to_string());
    }
    Some(parsed.with_timezone(&chrono::Utc).to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// The inline picture a message block holds, if any.
fn inline_image_url(block: &Value) -> Option<&str> {
    block.get("url").or_else(|| block.get("image_url")).and_then(|url| url.as_str()).filter(|url| url.starts_with("data:"))
}

fn is_user_image(block: &Value) -> bool {
    match text_field(block, "type") {
        "image" => inline_image_url(block).is_some(),
        "local_image" => true,
        _ => false,
    }
}

/// The `item` of an `item_completed` event, or None for any other line.
fn completed_item(entry: &Value) -> Option<&Value> {
    let payload = entry.get("payload")?;
    (text_field(entry, "type") == "event_msg" && text_field(payload, "type") == "item_completed")
        .then(|| payload.get("item"))
        .flatten()
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct CodexMeta {
    pub id: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub first_prompt: Option<String>,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,
    /// When the last real message was sent; `last_timestamp` also counts bookkeeping lines.
    pub last_message_timestamp: Option<String>,
    /// Prompts typed in this session. A fork's inherited ones are in `inherited_prompts`.
    pub prompts: u64,
    pub inherited_prompts: u64,
    pub parent_id: Option<String>,
    pub forked_from: Option<String>,
    pub agent_role: Option<String>,
    pub agent_name: Option<String>,
    /// A spawned or background helper (guardian, worker), not a conversation of its own.
    pub is_subagent: bool,
    /// The file couldn't be read to its end.
    pub ended_early: bool,
    /// The file has the line every session starts with.
    pub has_session_meta: bool,
}

fn read_meta(path: &Path) -> Result<CodexMeta, String> {
    let mut meta = CodexMeta { id: session_id_from_path(path), ..Default::default() };
    let mut lines = read_lines(path)?;
    for line in lines.by_ref() {
        let is_interesting = line.contains("\"session_meta\"")
            || (line.contains("\"item_completed\"") && line.contains("\"UserMessage\""));
        let mut line_timestamp = None;
        if let Some(timestamp_start) = line.find("\"timestamp\":\"") {
            let value_start = timestamp_start + "\"timestamp\":\"".len();
            if let Some(value_end) = line[value_start..].find('"') {
                line_timestamp = clean_timestamp(&line[value_start..value_start + value_end]);
                if let Some(timestamp) = &line_timestamp {
                    meta.first_timestamp.get_or_insert_with(|| timestamp.clone());
                    meta.last_timestamp = Some(timestamp.clone());
                }
            }
        }
        if line.contains("\"item_completed\"") && line.contains("\"AgentMessage\"") && line_timestamp.is_some() {
            meta.last_message_timestamp = line_timestamp.clone();
        }
        if !is_interesting {
            continue;
        }
        let entry: Value = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if text_field(&entry, "type") == "session_meta" {
            let payload = &entry["payload"];
            meta.has_session_meta = true;
            if meta.cwd.is_empty() {
                meta.cwd = text_field(payload, "cwd").to_string();
            }
            if let Some(branch) = payload.get("git").and_then(|git| git.get("branch")).and_then(|branch| branch.as_str()) {
                meta.branch = Some(branch.to_string());
            }
            meta.forked_from = payload.get("forked_from_id").and_then(|id| id.as_str()).map(String::from);
            if let Some(subagent) = payload.get("source").and_then(|source| source.get("subagent")) {
                meta.is_subagent = true;
                if let Some(spawn) = subagent.get("thread_spawn") {
                    meta.parent_id = spawn.get("parent_thread_id").and_then(|id| id.as_str()).map(String::from);
                    meta.agent_role = spawn.get("agent_role").and_then(|role| role.as_str()).map(String::from);
                    meta.agent_name = spawn.get("agent_nickname").and_then(|name| name.as_str()).map(String::from);
                }
            }
        } else if let Some(item) = completed_item(&entry).filter(|item| text_field(item, "type") == "UserMessage") {
            let text = clean_user_text(&content_text(item));
            let has_image = item["content"].as_array().is_some_and(|blocks| blocks.iter().any(is_user_image));
            if !text.is_empty() || has_image {
                meta.prompts += 1;
                if line_timestamp.is_some() {
                    meta.last_message_timestamp = line_timestamp;
                }
                if meta.first_prompt.is_none() && !text.is_empty() {
                    meta.first_prompt = Some(text.chars().take(200).collect());
                }
            }
        }
    }
    meta.ended_early = lines.ended_early;
    if meta.forked_from.is_some() {
        // A fork's copied history isn't its own work, and finding where it ends takes the
        // whole parse. Only forks pay for that.
        let session = parse_session(path)?;
        meta.inherited_prompts = user_prompts(session.inherited()) as u64;
        meta.prompts = user_prompts(session.own()) as u64;
    }
    Ok(meta)
}

/// What the list knows about every rollout, kept across launches: otherwise each launch
/// decompresses hundreds of files.
static CODEX_INDEX: FileIndex<CodexMeta> = FileIndex::new("codex-index");

fn cached_meta(path: &Path) -> Option<CodexMeta> {
    CODEX_INDEX.get_or_read(path, |file| read_meta(file).ok())
}

pub(super) fn save_index() {
    CODEX_INDEX.save();
}

/// Links aren't followed: one pointing back into the tree would list every session under it
/// twice. `locked` collects folders that exist but can't be opened.
fn collect_rollouts(directory: &Path, found: &mut Vec<PathBuf>, locked: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(read_error) => {
            if read_error.kind() != std::io::ErrorKind::NotFound {
                locked.push(directory.to_path_buf());
            }
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else { continue };
        if file_type.is_dir() {
            collect_rollouts(&path, found, locked);
        } else if file_type.is_file() && is_codex_path(&path) {
            found.push(path);
        }
    }
}

/// Every Codex transcript, plus the folders that couldn't be opened. A Mac without Codex
/// simply has neither.
fn scan_rollouts() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut found, mut locked) = (Vec::new(), Vec::new());
    if let Some(home) = codex_home() {
        collect_rollouts(&home.join("sessions"), &mut found, &mut locked);
        collect_rollouts(&home.join("archived_sessions"), &mut found, &mut locked);
    }
    (found, locked)
}

fn all_rollouts() -> Vec<PathBuf> {
    scan_rollouts().0
}

/// What couldn't be read, in plain words, for `get_read_problems`.
pub(super) fn read_problems() -> Vec<String> {
    let (rollouts, locked) = scan_rollouts();
    let mut problems: Vec<String> = locked.iter().map(|folder| format!("Couldn't open the Codex folder {}.", folder.display())).collect();
    let metas = in_parallel(&rollouts, |path| Some(cached_meta(path)));
    let unreadable = metas.iter().filter(|meta| !meta.as_ref().is_some_and(|meta| meta.has_session_meta)).count();
    let partial = metas.iter().filter(|meta| meta.as_ref().is_some_and(|meta| meta.has_session_meta && meta.ended_early)).count();
    let sessions_word = |count: usize| if count == 1 { "1 Codex session".to_string() } else { format!("{} Codex sessions", count) };
    if unreadable > 0 {
        problems.push(format!("{} couldn't be read: the files are locked or damaged.", sessions_word(unreadable)));
    }
    if partial > 0 {
        problems.push(format!("{} ended early: the rest of the file is damaged.", sessions_word(partial)));
    }
    problems
}

fn thread_names() -> HashMap<String, String> {
    let index = codex_home().and_then(|home| fs::read_to_string(home.join("session_index.jsonl")).ok());
    let mut names = HashMap::new();
    for line in index.unwrap_or_default().lines() {
        if let Ok(entry) = serde_json::from_str::<Value>(line) {
            let name = text_field(&entry, "thread_name");
            if !name.is_empty() {
                names.insert(text_field(&entry, "id").to_string(), name.to_string());
            }
        }
    }
    names
}

fn session_info(path: &Path, meta: &CodexMeta, names: &HashMap<String, String>) -> SessionInfo {
    SessionInfo {
        session_id: meta.id.clone(),
        tool: "codex".to_string(),
        archived_in_tool: path.components().any(|part| part.as_os_str() == "archived_sessions"),
        summary: None,
        custom_title: None,
        ai_title: names.get(&meta.id).cloned(),
        first_prompt: meta.first_prompt.clone(),
        project_path: meta.cwd.clone(),
        project_name: project_name_of(&meta.cwd),
        created: meta.first_timestamp.clone(),
        modified: meta.last_message_timestamp.clone().or(meta.last_timestamp.clone()).or_else(|| get_file_mtime_iso(path)),
        last_written: get_file_mtime_iso(path),
        conversation_count: meta.prompts,
        git_branch: meta.branch.clone(),
        forked_from_session_id: meta.forked_from.clone(),
        jsonl_path: path.to_string_lossy().to_string(),
    }
}

/// Runs `work` across a thread per core. Files are independent, and decompressing hundreds
/// of them one at a time is what makes a search or a cold start feel slow.
pub(super) fn in_parallel<Item: Sync, Output: Send>(items: &[Item], work: impl Fn(&Item) -> Option<Output> + Sync) -> Vec<Output> {
    let threads = std::thread::available_parallelism().map_or(4, |count| count.get());
    let chunk_size = items.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = items
            .chunks(chunk_size)
            .map(|chunk| scope.spawn(|| chunk.iter().filter_map(&work).collect::<Vec<_>>()))
            .collect();
        workers.into_iter().flat_map(|worker| worker.join().unwrap_or_default()).collect()
    })
}

/// Conversations only: helpers belong to their parent, and files that never reached a
/// prompt (a window opened and closed) aren't sessions anyone wants to see.
pub(super) fn sessions() -> Vec<SessionInfo> {
    let names = thread_names();
    in_parallel(&all_rollouts(), |path| {
        let meta = cached_meta(path)?;
        (!meta.is_subagent && meta.prompts + meta.inherited_prompts > 0 && !meta.cwd.is_empty()).then(|| session_info(path, &meta, &names))
    })
}

/// One thing that happened in a session, whichever era of the format recorded it.
enum Event {
    User { text: String, images: Vec<MessageImage>, timestamp: String },
    Block { block: MessageBlock, timestamp: String },
    Tool { call: ToolCall, timestamp: String },
    Compaction { text: String, timestamp: String },
    Aborted { timestamp: String },
    Failure { text: String, timestamp: String },
    Notification { notification: AgentNotification, timestamp: String },
    EndedEarly { timestamp: String },
}

struct ToolCall {
    id: String,
    name: String,
    summary: String,
    input: String,
    output: String,
    failed: bool,
    images: Vec<String>,
    /// A picture still to be read off the disk if it's asked for. Decoding every viewed image
    /// up front would hold them all in memory.
    image_file: Option<String>,
    agent_id: Option<String>,
    edits: Vec<PatchEdit>,
    reads: Vec<String>,
}

struct PatchEdit {
    path: String,
    action: &'static str,
    old: Option<String>,
    new: Option<String>,
}

#[derive(Default)]
struct CodexSession {
    events: Vec<Event>,
    models: Vec<String>,
    usage: (u64, u64, u64, u64),
    turns: u64,
    active_ms: u64,
    /// In a fork, how many of `events` were copied from the parent. Zero otherwise.
    inherited_events: usize,
    fork_timestamp: String,
    /// Token totals had already reached this when the fork began.
    inherited_usage: (u64, u64, u64, u64),
}

impl CodexSession {
    fn inherited(&self) -> &[Event] {
        &self.events[..self.inherited_events]
    }

    fn own(&self) -> &[Event] {
        &self.events[self.inherited_events..]
    }

    /// Everything counted so far was the parent's work.
    fn begin_own_work(&mut self, models: &mut HashSet<String>, timestamp: String) {
        self.inherited_events = self.events.len();
        self.fork_timestamp = timestamp;
        self.inherited_usage = self.usage;
        self.turns = 0;
        self.active_ms = 0;
        models.clear();
    }
}

fn user_prompts(events: &[Event]) -> usize {
    events.iter().filter(|event| matches!(event, Event::User { .. })).count()
}

const COMPACTION_PLACEHOLDER: &str = "Codex compacted the conversation here. It didn't record the summary as readable text.";

/// Scripts in newer sessions run commands through a wrapper; the commands then appear as
/// their own items, and the wrapper itself is only worth showing when it ran none.
const SCRIPT_WRAPPERS: &[&str] = &["exec", "wait"];

const COMPLETED_TOOL_ITEMS: &[&str] =
    &["CommandExecution", "FileChange", "McpToolCall", "WebSearch", "ImageView", "Extension", "DynamicToolCall"];

const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

const ENDED_EARLY_NOTICE: &str = "This transcript ended early: the rest of the file couldn't be read.";

/// Tool calls that become their own blocks however they were recorded.
const STRUCTURED_CALLS: &[&str] = &["update_plan", "request_user_input", "request_user_input_async"];

/// A fork's copied lines arrive together, this far apart at most. The first real gap after
/// them is the fork's own work. Used only when the parent's file is gone.
const FORK_BURST_GAP_MS: i64 = 2000;

/// The session being looked at, parsed once. Opening one asks for its messages, results,
/// stats and changes separately, and each would otherwise decompress the whole file. The
/// lock is held while parsing so those requests wait for the first instead of repeating it.
// ponytail: one entry, because only one session is open at a time.
fn load_session(path: &Path) -> Result<std::sync::Arc<CodexSession>, String> {
    type Entry = (PathBuf, SystemTime, u64, std::sync::Arc<CodexSession>);
    static CURRENT: std::sync::Mutex<Option<Entry>> = std::sync::Mutex::new(None);
    let info = fs::metadata(path).map_err(|read_error| format!("Cannot read session: {}", read_error))?;
    let modified = info.modified().map_err(|read_error| format!("Cannot read session: {}", read_error))?;
    let mut guard = CURRENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((cached_path, cached_modified, cached_size, session)) = guard.as_ref() {
        if cached_path == path && *cached_modified == modified && *cached_size == info.len() {
            return Ok(session.clone());
        }
    }
    let session = std::sync::Arc::new(parse_session(path)?);
    *guard = Some((path.to_path_buf(), modified, info.len(), session.clone()));
    Ok(session)
}

/// Identifiers that are unique across rollouts. `item-N` and `rollout-N` restart in each file.
fn stable_ids(entry: &Value) -> Vec<&str> {
    if text_field(entry, "type") == "session_meta" {
        return Vec::new();
    }
    let payload = &entry["payload"];
    let item_id = text_field(&payload["item"], "id");
    [text_field(payload, "call_id"), text_field(payload, "id"), text_field(payload, "turn_id"), item_id]
        .into_iter()
        .filter(|id| !id.is_empty() && !id.starts_with("item-") && !id.starts_with("rollout-"))
        .collect()
}

fn payload_fingerprint(entry: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    entry["payload"].to_string().hash(&mut hasher);
    hasher.finish()
}

/// Where a fork's own work starts: the line after the last one copied from its parent. Codex
/// rewrites a copied line's time to the moment of the fork, so ids are what tell them apart.
/// The last id shared with the parent anchors it. Lines with no id of their own (token counts,
/// a bare message) are then followed for as long as the parent has them word for word, which
/// stops at the fork's first new turn. Items aren't compared, since Codex rewrites those too.
fn fork_boundary(entries: &[Value], parent: Option<&Path>) -> usize {
    let copied_up_to = parent.and_then(|parent_path| {
        let (mut ids, mut fingerprints) = (HashSet::new(), HashSet::new());
        for line in read_lines(parent_path).ok()? {
            if let Ok(entry) = serde_json::from_str::<Value>(&line) {
                ids.extend(stable_ids(&entry).into_iter().map(String::from));
                if completed_item(&entry).is_none() && text_field(&entry, "type") != "session_meta" {
                    fingerprints.insert(payload_fingerprint(&entry));
                }
            }
        }
        let last_shared = entries.iter().rposition(|entry| stable_ids(entry).iter().any(|id| ids.contains(*id)));
        let mut boundary = last_shared.map_or(0, |index| index + 1);
        for (index, entry) in entries.iter().enumerate().skip(boundary) {
            if text_field(entry, "type") == "session_meta" || completed_item(entry).is_some() {
                continue;
            }
            if !fingerprints.contains(&payload_fingerprint(entry)) {
                break;
            }
            boundary = index + 1;
        }
        (boundary > 0).then_some(boundary)
    });
    copied_up_to.unwrap_or_else(|| initial_burst_end(entries))
}

/// The first line after the burst of copied lines a fork starts with.
// ponytail: a fork that starts its own turn within the gap would count it as inherited; only
// reached when the parent's rollout is gone.
fn initial_burst_end(entries: &[Value]) -> usize {
    let times: Vec<Option<i64>> = entries.iter().map(|entry| parse_millis(text_field(entry, "timestamp"))).collect();
    times
        .windows(2)
        .position(|pair| match pair {
            [Some(earlier), Some(later)] => later - earlier > FORK_BURST_GAP_MS,
            _ => true,
        })
        .map_or(entries.len(), |index| index + 1)
}

/// A pasted image as the model received it: `<image name=[Image #1] path="...">` when Codex
/// recorded where it came from.
struct SentImage {
    path: Option<String>,
    data_url: String,
}

fn sent_images(payload: &Value) -> Vec<SentImage> {
    let mut images = Vec::new();
    let mut tagged_path = None;
    for block in payload["content"].as_array().into_iter().flatten() {
        match text_field(block, "type") {
            "input_text" => {
                let text = text_field(block, "text");
                tagged_path = text.rfind("<image").and_then(|start| {
                    let tag = &text[start..];
                    let value = &tag[tag.find("path=\"")? + "path=\"".len()..];
                    Some(value[..value.find('"')?].to_string())
                });
            }
            "input_image" => {
                let url = text_field(block, "image_url");
                if url.starts_with("data:") {
                    images.push(SentImage { path: tagged_path.take(), data_url: url.to_string() });
                }
            }
            _ => {}
        }
    }
    images
}

/// `<subagent_notification>{"agent_path":"<id>","status":{"completed":"report"}}`
fn parse_agent_notification(payload: &Value) -> Option<AgentNotification> {
    if text_field(payload, "role") != "user" {
        return None;
    }
    let blocks = payload["content"].as_array()?;
    let text = blocks.iter().map(|block| text_field(block, "text")).collect::<String>();
    let body = text.trim().strip_prefix("<subagent_notification>")?.strip_suffix("</subagent_notification>")?;
    let parsed: Value = serde_json::from_str(body.trim()).ok()?;
    let agent_id = text_field(&parsed, "agent_path");
    let (status, result) = match &parsed["status"] {
        Value::Object(fields) => fields.iter().next().map(|(state, detail)| (state.clone(), detail.as_str().unwrap_or("").to_string()))?,
        other => (other.as_str()?.to_string(), String::new()),
    };
    let name = all_rollouts()
        .iter()
        .find(|rollout| session_id_from_path(rollout) == agent_id)
        .and_then(|child| cached_meta(child)?.agent_name)
        .unwrap_or_else(|| agent_id.to_string());
    let status = if status == "errored" { "failed".to_string() } else { status };
    Some(AgentNotification { summary: format!("Agent \"{}\" {}", name, if status == "completed" { "finished" } else { &status }), status, kind: "agent".to_string(), result, ..Default::default() })
}

/// The string fields of a call inside a script, in order, read from its JavaScript source.
/// `update_plan({plan:[{step:"a",status:"completed"}]})` → (step, a), (status, completed).
fn script_call_strings(source: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut depth = 0;
    let mut key = String::new();
    let mut after_colon = false;
    let mut characters = source.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            ':' => after_colon = true,
            '"' | '\'' | '`' => {
                let mut literal = String::new();
                while let Some(inner) = characters.next() {
                    match inner {
                        '\\' => match characters.next() {
                            Some('n') => literal.push('\n'),
                            Some(escaped) => literal.push(escaped),
                            None => {}
                        },
                        quote if quote == character => break,
                        other => literal.push(other),
                    }
                }
                if after_colon {
                    fields.push((key.clone(), literal));
                    after_colon = false;
                } else {
                    key = literal;
                }
            }
            letter if letter.is_alphabetic() || letter == '_' => {
                key = letter.to_string();
                while let Some(next) = characters.peek().copied().filter(|next| next.is_alphanumeric() || *next == '_') {
                    key.push(next);
                    characters.next();
                }
                after_colon = false;
            }
            ',' => after_colon = false,
            _ => {}
        }
    }
    fields
}

/// Plans a script set with `tools.update_plan(...)`. A plan built from variables has no
/// strings to read, and stays visible only as the script's code.
fn plans_in_script(script: &str) -> Vec<Vec<PlanItem>> {
    script
        .match_indices("update_plan(")
        .map(|(start, marker)| {
            let mut items: Vec<PlanItem> = Vec::new();
            for (key, value) in script_call_strings(&script[start + marker.len() - 1..]) {
                match key.as_str() {
                    "step" => items.push(PlanItem { content: value, status: "pending".to_string() }),
                    "status" => {
                        if let Some(last) = items.last_mut() {
                            last.status = value;
                        }
                    }
                    _ => {}
                }
            }
            items
        })
        .filter(|items| !items.is_empty())
        .collect()
}

fn raw_reasoning_text(payload: &Value) -> String {
    payload["summary"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn is_web_item(item: &Value) -> bool {
    let kind = text_field(item, "type");
    kind == "WebSearch" || (kind == "Extension" && text_field(item, "kind") == "web.search")
}

/// A viewed picture comes from the tool's output if it has one, or else the file on disk. With
/// neither, say so instead of opening to an empty panel.
fn resolve_viewed_image(call: &mut ToolCall) {
    let Some(file_path) = call.image_file.take() else { return };
    if !call.images.is_empty() {
        return;
    }
    match image_media_type(Path::new(&file_path)) {
        Ok(_) => call.image_file = Some(file_path),
        Err(problem) => {
            if call.output.is_empty() {
                call.output = format!("{}: {}", problem, file_path);
            }
        }
    }
}

fn parse_session(path: &Path) -> Result<CodexSession, String> {
    let mut lines = read_lines(path)?;
    let entries: Vec<Value> = lines.by_ref().filter_map(|line| serde_json::from_str(&line).ok()).collect();
    let session_meta = entries.iter().find(|entry| text_field(entry, "type") == "session_meta").map(|entry| &entry["payload"]);
    let session_cwd = session_meta.map_or("", |payload| text_field(payload, "cwd"));
    let fork_start = session_meta.and_then(|payload| payload.get("forked_from_id")).and_then(|id| id.as_str()).map_or(0, |parent_id| {
        let parent = all_rollouts().into_iter().find(|rollout| session_id_from_path(rollout) == parent_id);
        fork_boundary(&entries, parent.as_deref())
    });

    // First pass: what the raw calls need to know about the rest of the file.
    let mut tool_items = HashMap::new();
    let mut raw_tool_ids = HashSet::new();
    let mut raw_thinking = HashSet::new();
    let mut searched_queries = HashSet::new();
    let mut outputs = HashMap::new();
    let mut has_compaction_items = false;
    for entry in &entries {
        let payload = &entry["payload"];
        if let Some(item) = completed_item(entry) {
            let kind = text_field(item, "type");
            if !matches!(kind, "UserMessage" | "AgentMessage" | "Reasoning" | "Plan") && !text_field(item, "id").is_empty() {
                tool_items.insert(text_field(item, "id"), item);
            }
            has_compaction_items |= kind == "ContextCompaction";
            if kind == "WebSearch" || kind == "Extension" {
                searched_queries.insert(text_field(item, "query"));
            }
        } else if text_field(entry, "type") == "response_item" {
            match text_field(payload, "type") {
                "function_call" | "custom_tool_call" if !STRUCTURED_CALLS.contains(&text_field(payload, "name")) => {
                    raw_tool_ids.insert(text_field(payload, "call_id"));
                }
                "function_call_output" | "custom_tool_call_output" => {
                    outputs.insert(text_field(payload, "call_id"), payload.get("output"));
                }
                "tool_search_output" => {
                    outputs.insert(text_field(payload, "call_id"), payload.get("tools"));
                }
                "reasoning" => {
                    raw_thinking.insert(raw_reasoning_text(payload));
                }
                _ => {}
            }
        }
    }

    // Which script each item without a raw call of its own ran inside. Items can land just
    // after their script's output, so the last closed script still claims them.
    let mut open_scripts: Vec<&str> = Vec::new();
    let mut last_closed_script = None;
    let mut scripts_with_items = HashSet::new();
    let mut scripts_past_the_web = HashSet::new();
    let mut viewed_by_script: HashMap<&str, usize> = HashMap::new();
    let mut image_view_script = HashMap::new();
    for entry in &entries {
        let payload = &entry["payload"];
        if let Some(item) = completed_item(entry) {
            let (kind, item_id) = (text_field(item, "type"), text_field(item, "id"));
            if !COMPLETED_TOOL_ITEMS.contains(&kind) || raw_tool_ids.contains(item_id) {
                continue;
            }
            if let Some(script) = open_scripts.last().copied().or(last_closed_script) {
                scripts_with_items.insert(script);
                if !is_web_item(item) {
                    scripts_past_the_web.insert(script);
                }
                if kind == "ImageView" {
                    *viewed_by_script.entry(script).or_default() += 1;
                    image_view_script.insert(item_id, script);
                }
            }
        } else if text_field(entry, "type") == "response_item" {
            let call_id = text_field(payload, "call_id");
            match text_field(payload, "type") {
                "function_call" | "custom_tool_call" if SCRIPT_WRAPPERS.contains(&text_field(payload, "name")) => open_scripts.push(call_id),
                "function_call_output" | "custom_tool_call_output" if open_scripts.contains(&call_id) => {
                    open_scripts.retain(|open| *open != call_id);
                    last_closed_script = Some(call_id);
                }
                _ => {}
            }
        }
    }

    let mut session = CodexSession::default();
    let mut turn_started: Option<i64> = None;
    let mut models = HashSet::new();
    let mut sent_user_images: Vec<SentImage> = Vec::new();
    let mut images_given_out: HashMap<&str, usize> = HashMap::new();
    for (line_index, entry) in entries.iter().enumerate() {
        let timestamp = clean_timestamp(text_field(entry, "timestamp")).unwrap_or_default();
        if line_index == fork_start && fork_start > 0 {
            session.begin_own_work(&mut models, timestamp.clone());
        }
        let payload = &entry["payload"];
        match (text_field(entry, "type"), text_field(payload, "type")) {
            ("turn_context", _) => {
                if let Some(model) = payload.get("model").and_then(|model| model.as_str()) {
                    models.insert(model.to_string());
                }
            }
            ("compacted", _) => {
                let summary = text_field(payload, "message");
                let last_is_placeholder = matches!(session.events.last(), Some(Event::Compaction { text, .. }) if text == COMPACTION_PLACEHOLDER);
                if !summary.is_empty() {
                    if last_is_placeholder {
                        session.events.pop();
                    }
                    session.events.push(Event::Compaction { text: summary.to_string(), timestamp });
                } else if !has_compaction_items {
                    session.events.push(Event::Compaction { text: COMPACTION_PLACEHOLDER.to_string(), timestamp });
                }
            }
            ("event_msg", "item_completed") => {
                let item = &payload["item"];
                let item_id = text_field(item, "id");
                // Already placed where its raw call was made.
                if raw_tool_ids.contains(item_id) {
                    continue;
                }
                let is_user_message = text_field(item, "type") == "UserMessage";
                if let Some(mut event) = event_from_item(item, &raw_thinking, &outputs, &sent_user_images, timestamp) {
                    if let (Event::Tool { call, .. }, Some(script)) = (&mut event, image_view_script.get(item_id).copied()) {
                        let given_out = images_given_out.entry(script).or_default();
                        if let Some(image) = output_images(outputs.get(script).copied().flatten()).nth(*given_out) {
                            call.images = vec![image.to_string()];
                        }
                        *given_out += 1;
                    }
                    if let Event::Tool { call, .. } = &mut event {
                        resolve_viewed_image(call);
                    }
                    session.events.push(event);
                }
                if is_user_message {
                    sent_user_images.clear();
                }
            }
            ("event_msg", "turn_aborted") => {
                session.active_ms += payload.get("duration_ms").and_then(|duration| duration.as_u64()).unwrap_or(0);
                session.events.push(Event::Aborted { timestamp });
            }
            ("event_msg", "task_started") => {
                session.turns += 1;
                turn_started = parse_millis(&timestamp);
            }
            ("event_msg", "task_complete") => {
                session.active_ms += payload.get("duration_ms").and_then(|duration| duration.as_u64()).unwrap_or_else(|| {
                    // Older files record no duration, only when the turn began and ended.
                    let ended = parse_millis(&timestamp);
                    turn_started.zip(ended).map_or(0, |(started, ended)| (ended - started).max(0) as u64)
                });
                if let Some(error) = payload.get("error").filter(|error| !error.is_null()) {
                    let text = error.get("message").and_then(|message| message.as_str()).map(String::from).unwrap_or_else(|| error.to_string());
                    session.events.push(Event::Failure { text, timestamp });
                }
            }
            ("event_msg", "token_count") => {
                if let Some(total) = payload["info"].get("total_token_usage") {
                    let number = |key: &str| total.get(key).and_then(|value| value.as_u64()).unwrap_or(0);
                    let cached = number("cached_input_tokens");
                    session.usage = (number("input_tokens").saturating_sub(cached), number("output_tokens"), cached, number("cache_write_input_tokens"));
                }
            }
            ("response_item", "message") => {
                if let Some(notification) = parse_agent_notification(payload) {
                    session.events.push(Event::Notification { notification, timestamp });
                } else if text_field(payload, "role") == "user" {
                    let images = sent_images(payload);
                    if !images.is_empty() {
                        sent_user_images = images;
                    }
                }
            }
            ("response_item", "function_call" | "custom_tool_call") => {
                let call_id = text_field(payload, "call_id");
                let name = text_field(payload, "name");
                let arguments: Value = serde_json::from_str(text_field(payload, "arguments")).unwrap_or(Value::Null);
                if name == "exec" {
                    for items in plans_in_script(text_field(payload, "input")) {
                        session.events.push(Event::Block { block: MessageBlock::Plan { items }, timestamp: timestamp.clone() });
                    }
                }
                match name {
                    "update_plan" => {
                        if let Some(steps) = arguments["plan"].as_array() {
                            let items = steps
                                .iter()
                                .map(|step| PlanItem { content: text_field(step, "step").to_string(), status: text_field(step, "status").to_string() })
                                .collect();
                            session.events.push(Event::Block { block: MessageBlock::Plan { items }, timestamp });
                        }
                    }
                    "request_user_input" | "request_user_input_async" => {
                        let first = &arguments["questions"][0];
                        let question = first.get("question").or_else(|| first.get("title")).and_then(|text| text.as_str()).unwrap_or("");
                        // A reply that isn't JSON means the question never reached anyone.
                        let refusal = outputs.get(call_id).copied().flatten().and_then(|output| output.as_str()).filter(|reply| !reply.is_empty() && !reply.starts_with('{'));
                        if let Some(reason) = refusal {
                            let mut call = plain_call("AskUserQuestion", call_id, question, String::new());
                            call.output = reason.to_string();
                            call.failed = true;
                            session.events.push(Event::Tool { call, timestamp });
                        } else {
                            session.events.push(Event::Block { block: tool("AskUserQuestion", question, String::new(), call_id), timestamp });
                        }
                    }
                    _ => {
                        if let Some(item) = tool_items.get(call_id) {
                            if let Some(mut event) = event_from_item(item, &raw_thinking, &outputs, &sent_user_images, timestamp) {
                                if let Event::Tool { call, .. } = &mut event {
                                    resolve_viewed_image(call);
                                }
                                session.events.push(event);
                            }
                            continue;
                        }
                        let output = outputs.get(call_id).copied().flatten();
                        let mut call = tool_from_raw_call(name, payload, &arguments, output, session_cwd);
                        if scripts_with_items.contains(call_id) {
                            // The items it ran are the record. Only keep the script itself for
                            // what they don't show: why it failed, pictures no item took, or
                            // page text behind what's otherwise only a search.
                            let viewed = viewed_by_script.get(call_id).copied().unwrap_or(0);
                            call.images.drain(..viewed.min(call.images.len()));
                            let script_failed = output_parts(output).0.starts_with("Script failed");
                            if !script_failed && call.images.is_empty() && scripts_past_the_web.contains(call_id) {
                                continue;
                            }
                        }
                        resolve_viewed_image(&mut call);
                        session.events.push(Event::Tool { call, timestamp });
                    }
                }
            }
            ("response_item", "tool_search_call") => {
                let call_id = text_field(payload, "call_id");
                let mut call = plain_call("ToolSearch", call_id, text_field(&payload["arguments"], "query"), String::new());
                let found = outputs.get(call_id).copied().flatten().and_then(|tools| tools.as_array());
                call.output = found.into_iter().flatten().map(|found_tool| text_field(found_tool, "name")).collect::<Vec<_>>().join("\n");
                session.events.push(Event::Tool { call, timestamp });
            }
            ("response_item", "web_search_call") => {
                let action = &payload["action"];
                let query = action.get("query").and_then(|query| query.as_str())
                    .or_else(|| action["queries"][0].as_str())
                    .unwrap_or("");
                let (url, pattern) = (text_field(action, "url"), text_field(action, "pattern"));
                let summary = match (query, pattern, url) {
                    ("", "", "") => String::new(),
                    ("", "", url) => format!("Opened {}", url),
                    ("", pattern, url) => format!("Looked for \"{}\" in {}", pattern, url),
                    (query, _, _) => query.to_string(),
                };
                let id = text_field(payload, "id");
                let already_shown = tool_items.contains_key(id) || (!query.is_empty() && searched_queries.contains(query));
                if !summary.is_empty() && !already_shown {
                    let call = plain_call("WebSearch", id, &summary, String::new());
                    session.events.push(Event::Tool { call, timestamp });
                }
            }
            ("response_item", "reasoning") => {
                let summary = raw_reasoning_text(payload);
                if !summary.trim().is_empty() {
                    session.events.push(Event::Block { block: MessageBlock::Thinking { text: summary }, timestamp });
                }
            }
            _ => {}
        }
    }
    if fork_start > 0 && fork_start >= entries.len() {
        session.begin_own_work(&mut models, entries.last().map_or(String::new(), |entry| clean_timestamp(text_field(entry, "timestamp")).unwrap_or_default()));
    }
    let (input, output, cached, written) = session.usage;
    let (old_input, old_output, old_cached, old_written) = session.inherited_usage;
    session.usage = (input.saturating_sub(old_input), output.saturating_sub(old_output), cached.saturating_sub(old_cached), written.saturating_sub(old_written));
    if lines.ended_early {
        let timestamp = entries.last().map_or(String::new(), |entry| clean_timestamp(text_field(entry, "timestamp")).unwrap_or_default());
        session.events.push(Event::EndedEarly { timestamp });
    }
    session.models = models.into_iter().collect();
    session.models.sort();
    Ok(session)
}

fn parse_millis(timestamp: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp).ok().map(|parsed| parsed.timestamp_millis())
}

fn plain_call(name: &str, id: &str, summary: &str, input: String) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        summary: summary.to_string(),
        input,
        output: String::new(),
        failed: false,
        images: Vec::new(),
        image_file: None,
        agent_id: None,
        edits: Vec::new(),
        reads: Vec::new(),
    }
}

/// `file:///tmp/x.png` → `/tmp/x.png`.
// ponytail: no percent-decoding; no real path has needed it, add it when one does.
fn file_url_path(path: &str) -> &str {
    path.strip_prefix("file://").unwrap_or(path)
}

fn event_from_item(item: &Value, raw_thinking: &HashSet<String>, outputs: &HashMap<&str, Option<&Value>>, sent_user_images: &[SentImage], timestamp: String) -> Option<Event> {
    let item_id = text_field(item, "id");
    let raw_output = outputs.get(item_id).copied().flatten();
    // The raw call can hold more than the item does: pictures, or output the item left out.
    let tool_event = |mut call: ToolCall, timestamp: String| {
        let raw_images: Vec<String> = output_images(raw_output).map(String::from).collect();
        if raw_images.len() > call.images.len() {
            call.images = raw_images;
        }
        if call.output.is_empty() {
            let (raw_text, _) = output_parts(raw_output);
            call.output = strip_ansi(&split_output(&raw_text).0).trim_end().to_string();
        }
        Some(Event::Tool { call, timestamp })
    };
    match text_field(item, "type") {
        "UserMessage" => {
            let (images, notes) = user_images(item, sent_user_images);
            let mut text = clean_user_text(&content_text(item));
            for note in notes {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&note);
            }
            (!text.is_empty() || !images.is_empty()).then_some(Event::User { text, images, timestamp })
        }
        "AgentMessage" => {
            let text = content_text(item);
            (!text.trim().is_empty()).then_some(Event::Block { block: MessageBlock::Text { text }, timestamp })
        }
        "Plan" => Some(Event::Block {
            block: MessageBlock::Note { label: "Proposed plan".to_string(), text: text_field(item, "text").to_string() },
            timestamp,
        }),
        "Reasoning" => {
            // The raw event carries the same summary and is what older files have. Its id can
            // be missing and item ids repeat, so the text is what matches them.
            let summary = item["summary_text"].as_array().into_iter().flatten().filter_map(|part| part.as_str()).collect::<Vec<_>>().join("\n\n");
            if raw_thinking.contains(&summary) {
                return None;
            }
            (!summary.trim().is_empty()).then_some(Event::Block { block: MessageBlock::Thinking { text: summary }, timestamp })
        }
        "ContextCompaction" => Some(Event::Compaction { text: COMPACTION_PLACEHOLDER.to_string(), timestamp }),
        "CommandExecution" => {
            let command = command_text(item);
            let mut call = plain_call("Shell", item_id, &command, command.clone());
            let (raw_text, _) = output_parts(raw_output);
            let (raw_body, raw_exit_code, _) = split_output(&raw_text);
            let recorded = item.get("aggregated_output").and_then(|text| text.as_str()).unwrap_or_else(|| text_field(item, "stdout"));
            let output = if recorded.is_empty() { raw_body.as_str() } else { recorded };
            let exit_code = item.get("exit_code").and_then(|code| code.as_i64()).or(raw_exit_code).unwrap_or(0);
            call.failed = exit_code != 0 || text_field(item, "status") == "failed";
            let note = if exit_code != 0 { format!("\n[exit code {}]", exit_code) } else { String::new() };
            call.output = format!("{}{}", strip_ansi(output).trim_end(), note);
            let cwd = file_url_path(text_field(item, "cwd"));
            call.reads = item["parsed_cmd"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|parsed| text_field(parsed, "type") == "read" && !text_field(parsed, "path").is_empty())
                .map(|parsed| absolute_path(text_field(parsed, "path"), cwd))
                .collect();
            tool_event(call, timestamp)
        }
        "FileChange" => {
            let changes = item["changes"].as_object();
            let paths: Vec<&str> = changes.into_iter().flatten().map(|(path, _)| path.as_str()).collect();
            let mut call = plain_call("Edit", item_id, &edit_summary(&paths), paths.join("\n"));
            call.failed = text_field(item, "status") == "failed";
            call.output = changes
                .into_iter()
                .flatten()
                .map(|(file, change)| format!("{}  {}", text_field(change, "type"), file))
                .collect::<Vec<_>>()
                .join("\n");
            call.edits = changes
                .into_iter()
                .flatten()
                .flat_map(|(file, change)| edit_from_change(file, change))
                .collect();
            tool_event(call, timestamp)
        }
        "McpToolCall" => {
            let name = format!("mcp__{}__{}", text_field(item, "server"), text_field(item, "tool"));
            let arguments = &item["arguments"];
            let input = serde_json::to_string_pretty(arguments).unwrap_or_default();
            let mut call = plain_call(&name, item_id, &tool_summary(&name, arguments), input);
            let result = &item["result"];
            call.output = tool_result_text(result.get("content")).0;
            call.failed = result.get("isError").and_then(|flag| flag.as_bool()).unwrap_or(false) || text_field(item, "status") == "failed";
            call.images = result["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|block| text_field(block, "type") == "image")
                .filter_map(|block| {
                    let data = block.get("data").and_then(|data| data.as_str())?;
                    let media_type = block.get("mimeType").and_then(|kind| kind.as_str()).unwrap_or("image/png");
                    Some(format!("data:{};base64,{}", media_type, data))
                })
                .collect();
            tool_event(call, timestamp)
        }
        "WebSearch" => {
            let mut call = plain_call("WebSearch", item_id, &search_summary(item), String::new());
            call.output = search_hits(item);
            tool_event(call, timestamp)
        }
        "ImageView" => {
            let viewed_path = file_url_path(text_field(item, "path"));
            let mut call = plain_call("ViewImage", item_id, viewed_path, String::new());
            call.image_file = Some(viewed_path.to_string());
            tool_event(call, timestamp)
        }
        "Extension" if text_field(item, "kind") == "image_gen.generation" => {
            let revised_prompt = text_field(item, "revisedPrompt");
            let saved_path = text_field(item, "savedPath");
            let mut call = plain_call("ImageGeneration", item_id, first_line_of(revised_prompt), revised_prompt.to_string());
            let encoded = text_field(item, "result");
            if encoded.is_empty() {
                call.image_file = Some(saved_path.to_string()).filter(|saved| !saved.is_empty());
            } else {
                call.images = vec![format!("data:image/png;base64,{}", encoded)];
            }
            if !saved_path.is_empty() {
                call.output = format!("Saved to {}", saved_path);
            }
            call.failed = text_field(item, "status") == "failed";
            tool_event(call, timestamp)
        }
        "Extension" if text_field(item, "kind") == "web.search" => {
            let mut call = plain_call("WebSearch", item_id, &search_summary(item), String::new());
            call.output = search_hits(item);
            tool_event(call, timestamp)
        }
        "DynamicToolCall" => {
            let name = text_field(item, "tool");
            let mut call = plain_call(name, item_id, &tool_summary(name, &item["arguments"]), String::new());
            call.output = item["content_items"].as_array().into_iter().flatten().map(|part| text_field(part, "text")).collect::<Vec<_>>().join("\n");
            call.failed = item.get("success").and_then(|success| success.as_bool()) == Some(false);
            tool_event(call, timestamp)
        }
        // Whatever Codex adds next still shows up.
        unknown => {
            let name = if unknown == "Extension" { text_field(item, "kind") } else { unknown };
            let input = serde_json::to_string_pretty(item).unwrap_or_default();
            tool_event(plain_call(name, item_id, &tool_summary(name, item), input), timestamp)
        }
    }
}

/// What was searched for, or if Codex didn't record that, what came back.
fn search_summary(item: &Value) -> String {
    let query = text_field(item, "query");
    if !query.is_empty() {
        return query.to_string();
    }
    let first_hit = &item["results"][0];
    let described = ["title", "url", "domain"].iter().map(|key| text_field(first_hit, key)).find(|text| !text.is_empty());
    described.unwrap_or("Web search").to_string()
}

fn search_hits(item: &Value) -> String {
    item["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|hit| format!("{}\n{}", text_field(hit, "title"), text_field(hit, "url")))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn edit_summary(paths: &[&str]) -> String {
    match paths {
        [only] => (*only).to_string(),
        many => format!("{} files", many.len()),
    }
}

fn edit_from_change(file: &str, change: &Value) -> Vec<PatchEdit> {
    let content = change.get("content").and_then(|text| text.as_str()).map(String::from);
    match text_field(change, "type") {
        "add" => vec![PatchEdit { path: file.to_string(), action: "write", old: None, new: content }],
        "delete" => vec![PatchEdit { path: file.to_string(), action: "delete", old: content, new: None }],
        _ => {
            let (old, new) = split_unified_diff(text_field(change, "unified_diff"));
            match change.get("move_path").and_then(|target| target.as_str()) {
                // A rename is the old path going away and the new one being written to.
                Some(target) => vec![
                    PatchEdit { path: file.to_string(), action: "delete", old: None, new: None },
                    PatchEdit { path: target.to_string(), action: "edit", old, new },
                ],
                None => vec![PatchEdit { path: file.to_string(), action: "edit", old, new }],
            }
        }
    }
}

fn absolute_path(path: &str, cwd: &str) -> String {
    if path.starts_with('/') || cwd.is_empty() {
        path.to_string()
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), path)
    }
}

fn output_images(output: Option<&Value>) -> impl Iterator<Item = &str> {
    output
        .and_then(|output| output.as_array())
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("image_url").and_then(|url| url.as_str()))
}

/// A raw tool call's output as text plus any images: a plain string, or a list of parts.
fn output_parts(output: Option<&Value>) -> (String, Vec<String>) {
    match output {
        Some(Value::String(text)) => (text.clone(), Vec::new()),
        Some(Value::Array(parts)) => {
            let text = parts.iter().filter_map(|part| part.get("text").and_then(|text| text.as_str())).collect::<Vec<_>>().join("");
            (text, output_images(output).map(String::from).collect())
        }
        Some(other) => (other.to_string(), Vec::new()),
        None => (String::new(), Vec::new()),
    }
}

/// Splits the bookkeeping Codex puts around a command's output. Returns the output and the
/// exit code, if one was recorded.
fn split_output(raw: &str) -> (String, Option<i64>, bool) {
    if raw.trim_start().starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
            if let Some(output) = parsed.get("output").and_then(|output| output.as_str()) {
                return (output.to_string(), parsed["metadata"]["exit_code"].as_i64(), true);
            }
        }
    }
    const HEADERS: &[&str] = &["Command:", "Chunk ID:", "Wall time", "Exit code:", "Script ", "Process ", "Original token count:"];
    if !HEADERS.iter().any(|header| raw.starts_with(header)) {
        return (raw.to_string(), None, false);
    }
    let exit_in = |head: &str| {
        ["Process exited with code ", "Exit code: "].iter().find_map(|marker| {
            let found = head.rfind(marker)? + marker.len();
            let digits: String = head[found..].chars().take_while(|character| character.is_ascii_digit() || *character == '-').collect();
            digits.parse::<i64>().ok()
        })
    };
    if let Some(at) = raw.find("\nOutput:") {
        let rest = &raw[at + "\nOutput:".len()..];
        let body = rest.strip_prefix('\n').unwrap_or(rest);
        // A script that printed a command's result carries that command's exit code.
        let printed_exit = || {
            let parsed: Value = serde_json::from_str(body).ok()?;
            parsed.get("exit_code")?.as_i64()
        };
        let script_exit = if raw.starts_with("Script ") { printed_exit() } else { None };
        return (body.to_string(), exit_in(&raw[..at]).or(script_exit), true);
    }
    // No output section: anything that isn't bookkeeping is the message, like "aborted by user".
    let message: Vec<&str> = raw.lines().filter(|line| !HEADERS.iter().any(|header| line.starts_with(header))).collect();
    (message.join("\n"), exit_in(raw), true)
}

/// Without any bookkeeping, a tool's text is its own message, and that's how refusals and
/// crashes are reported.
fn reads_as_failure(text: &str) -> bool {
    // A structured reply is data, whatever words are in it.
    if text.trim_start().starts_with(['{', '[']) {
        return false;
    }
    let first_line = text.lines().next().unwrap_or("").to_lowercase();
    ["failed", "unable to", "rejected", "aborted", "error:"].iter().any(|word| first_line.contains(word))
}

/// `*** Add File: x` / `*** Update File: x` / `*** Delete File: x` patches, as written to
/// apply_patch. Older sessions have no other record of what changed.
fn parse_patch(patch: &str, cwd: &str) -> Vec<PatchEdit> {
    let mut edits: Vec<PatchEdit> = Vec::new();
    let mut removed: Vec<&str> = Vec::new();
    let mut added: Vec<&str> = Vec::new();
    let finish = |edits: &mut Vec<PatchEdit>, removed: &mut Vec<&str>, added: &mut Vec<&str>| {
        if let Some(edit) = edits.last_mut() {
            edit.old = (!removed.is_empty()).then(|| removed.join("\n"));
            edit.new = (!added.is_empty()).then(|| added.join("\n"));
        }
        removed.clear();
        added.clear();
    };
    for line in patch.lines() {
        let header = [("*** Add File: ", "write"), ("*** Update File: ", "edit"), ("*** Delete File: ", "delete")]
            .iter()
            .find_map(|(prefix, action)| line.strip_prefix(prefix).map(|path| (path, *action)));
        if let Some((path, action)) = header {
            finish(&mut edits, &mut removed, &mut added);
            edits.push(PatchEdit { path: absolute_path(path.trim(), cwd), action, old: None, new: None });
        } else if let Some(target) = line.strip_prefix("*** Move to: ") {
            // A rename is the old path going away and the new one being written to.
            if let Some(moved) = edits.last_mut() {
                moved.action = "delete";
            }
            edits.push(PatchEdit { path: absolute_path(target.trim(), cwd), action: "edit", old: None, new: None });
        } else if line.starts_with("*** ") {
            continue;
        } else if let Some(content) = line.strip_prefix('+') {
            added.push(content);
        } else if let Some(content) = line.strip_prefix('-') {
            removed.push(content);
        }
    }
    finish(&mut edits, &mut removed, &mut added);
    edits
}

fn first_line_of(text: &str) -> &str {
    text.lines().find(|line| !line.trim().is_empty()).unwrap_or("").trim()
}

fn tool_from_raw_call(name: &str, payload: &Value, arguments: &Value, output: Option<&Value>, session_cwd: &str) -> ToolCall {
    let call_id = text_field(payload, "call_id");
    let raw_input = text_field(payload, "input");
    let workdir = arguments.get("workdir").and_then(|dir| dir.as_str()).unwrap_or(session_cwd);
    let (raw_output, images) = output_parts(output);
    let (body, exit_code, has_header) = split_output(&raw_output);
    let failed = match exit_code {
        Some(code) => code != 0,
        None if raw_output.starts_with("Script failed") => true,
        None => reads_as_failure(if has_header { &body } else { &raw_output }) && !(has_header && raw_output.contains("\nOutput:")),
    };
    let shell_text = match name {
        "exec_command" => text_field(arguments, "cmd").to_string(),
        "shell" | "shell_command" => command_text(arguments),
        _ => String::new(),
    };
    let patch_text = match name {
        "apply_patch" if !raw_input.is_empty() => raw_input,
        "apply_patch" => text_field(arguments, "input"),
        _ if shell_text.contains("*** Begin Patch") => shell_text.as_str(),
        _ => "",
    };
    let mut call = if !patch_text.is_empty() {
        let named = parse_patch(patch_text, "");
        let paths: Vec<&str> = named.iter().map(|edit| edit.path.as_str()).collect();
        let mut edit_call = plain_call("Edit", call_id, &edit_summary(&paths), patch_text.to_string());
        edit_call.edits = parse_patch(patch_text, workdir);
        edit_call
    } else {
        match name {
            "exec_command" | "shell" | "shell_command" => plain_call("Shell", call_id, &shell_text, shell_text.clone()),
            "write_stdin" => {
                let chars = text_field(arguments, "chars");
                let target = arguments.get("session_id").map(|id| id.to_string()).unwrap_or_default();
                let summary = if chars.is_empty() { format!("Waited for session {}", target) } else { format!("Sent input to session {}: {}", target, chars.trim()) };
                plain_call("ShellInput", call_id, &summary, String::new())
            }
            "view_image" => {
                let viewed_path = file_url_path(text_field(arguments, "path"));
                let mut view_call = plain_call("ViewImage", call_id, viewed_path, String::new());
                view_call.image_file = Some(viewed_path.to_string());
                view_call
            }
            "js" => {
                let code = text_field(arguments, "code");
                let title = text_field(arguments, "title");
                let summary = if title.is_empty() { first_line_of(code) } else { title };
                plain_call("Script", call_id, summary, code.to_string())
            }
            "exec" => plain_call("Script", call_id, first_line_of(raw_input), raw_input.to_string()),
            "wait" => plain_call("Script", call_id, "Waited for a running script", String::new()),
            "spawn_agent" => {
                let message = text_field(arguments, "message");
                let mut agent_call = plain_call("Agent", call_id, first_line_of(message), message.to_string());
                let reply: Value = serde_json::from_str(&raw_output).unwrap_or(Value::Null);
                agent_call.agent_id = reply.get("agent_id").and_then(|id| id.as_str()).map(String::from);
                agent_call
            }
            "send_input" => {
                let message = text_field(arguments, "message");
                plain_call("AgentInput", call_id, first_line_of(message), message.to_string())
            }
            other => {
                let input = if raw_input.is_empty() { serde_json::to_string_pretty(arguments).unwrap_or_default() } else { raw_input.to_string() };
                let input = if input == "null" || input == "{}" { String::new() } else { input };
                plain_call(other, call_id, &tool_summary(other, arguments), input)
            }
        }
    };
    let note = match exit_code {
        Some(code) if code != 0 => format!("\n[exit code {}]", code),
        _ => String::new(),
    };
    call.output = format!("{}{}", strip_ansi(&body).trim_end(), note);
    // Asking for a helper gets back its id, or the reason it wasn't started.
    call.failed = failed || (name == "spawn_agent" && !raw_output.is_empty() && call.agent_id.is_none());
    call.images = images;
    call
}

/// A prompt's images, numbered by where they sit so `[Image #2]` stays #2 even if #1 is gone.
/// The bytes the model was sent are the truth; the file on disk is usually a deleted temp file.
/// Matched by the path Codex tagged them with, else by order if the counts agree. Returns the
/// images and a note for each that can't be shown.
fn user_images(item: &Value, sent: &[SentImage]) -> (Vec<MessageImage>, Vec<String>) {
    let blocks: Vec<&Value> = item["content"].as_array().into_iter().flatten().filter(|block| is_user_image(block)).collect();
    let mut images = Vec::new();
    let mut notes = Vec::new();
    for (position, block) in blocks.iter().enumerate() {
        let number = position as u32 + 1;
        if let Some(url) = inline_image_url(block) {
            images.push(MessageImage { number, data_url: url.to_string() });
            continue;
        }
        let image_path = text_field(block, "path");
        let from_transcript = sent
            .iter()
            .find(|image| image.path.as_deref() == Some(image_path))
            .or_else(|| if sent.len() == blocks.len() { sent.get(position) } else { None });
        match from_transcript.map_or_else(|| read_image_file(Path::new(image_path)), |image| Ok(image.data_url.clone())) {
            Ok(data_url) => images.push(MessageImage { number, data_url }),
            Err(problem) => notes.push(format!("[{}: {}]", problem, image_path)),
        }
    }
    (images, notes)
}

/// Only image files of a sane size, whatever a transcript names: it decides what gets read off
/// the disk. The error says why in words a person can read.
fn image_media_type(path: &Path) -> Result<&'static str, &'static str> {
    const GONE: &str = "Attached image no longer on disk";
    let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or("").to_lowercase();
    let media_type = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return Err("Attached file isn't an image that can be shown"),
    };
    if fs::metadata(path).map_err(|_| GONE)?.len() > MAX_IMAGE_BYTES {
        return Err("Attached image is too large to show");
    }
    Ok(media_type)
}

fn read_image_file(path: &Path) -> Result<String, &'static str> {
    use base64::Engine;
    let media_type = image_media_type(path)?;
    let bytes = fs::read(path).map_err(|_| "Attached image no longer on disk")?;
    Ok(format!("data:{};base64,{}", media_type, base64::engine::general_purpose::STANDARD.encode(bytes)))
}

pub(super) fn local_image_url(path: &Path) -> Option<String> {
    read_image_file(path).ok()
}

pub(super) fn parse_messages(path: &Path) -> Result<Vec<ConversationMessage>, String> {
    let session = load_session(path)?;
    let tools = HashMap::new();
    let mut parser = TranscriptParser::with_agents(&tools);
    let is_fork = session.inherited_events > 0;
    for (index, event) in session.events.iter().enumerate() {
        if is_fork && index == session.inherited_events {
            mark_fork(&mut parser, &session.fork_timestamp);
        }
        match event {
            Event::User { text, images, timestamp } => {
                parser.push_message(ConversationMessage {
                    role: "user".to_string(),
                    text: text.clone(),
                    timestamp: timestamp.clone(),
                    images: images.clone(),
                    ..Default::default()
                });
            }
            Event::Block { block, timestamp } => add_block(&mut parser, block.clone(), timestamp.clone()),
            Event::Tool { call, timestamp } => {
                let input: String = call.input.chars().take(TOOL_INPUT_LIMIT).collect();
                let block = MessageBlock::Tool {
                    name: call.name.clone(),
                    summary: call.summary.chars().take(TOOL_SUMMARY_LIMIT).collect(),
                    input_truncated: input.len() < call.input.len(),
                    input,
                    tool_use_id: Some(call.id.clone()).filter(|id| !id.is_empty()),
                    agent_id: call.agent_id.clone(),
                };
                add_block(&mut parser, block, timestamp.clone());
            }
            Event::Compaction { text, timestamp } => {
                parser.flush_turn();
                parser.messages.push(ConversationMessage {
                    role: "compaction".to_string(),
                    text: text.clone(),
                    timestamp: timestamp.clone(),
                    ..Default::default()
                });
            }
            Event::Aborted { timestamp } => {
                if parser.flush_turn() {
                    if let Some(last) = parser.messages.last_mut() {
                        last.interrupted = true;
                    }
                } else {
                    // Stopped before any reply, so there's no answer to carry the tag.
                    parser.push_event("notice", false, "Interrupted".to_string(), String::new(), timestamp.clone());
                }
            }
            Event::Failure { text, timestamp } => parser.push_event("api-error", true, text.clone(), String::new(), timestamp.clone()),
            Event::Notification { notification, timestamp } => parser.push_notification(notification.clone(), timestamp.clone()),
            Event::EndedEarly { timestamp } => parser.push_event("notice", true, ENDED_EARLY_NOTICE.to_string(), String::new(), timestamp.clone()),
        }
    }
    if is_fork && session.inherited_events == session.events.len() {
        mark_fork(&mut parser, &session.fork_timestamp);
    }
    Ok(parser.finish())
}

/// Everything so far came from the parent, and the fork's own work starts here.
fn mark_fork(parser: &mut TranscriptParser, timestamp: &str) {
    parser.flush_turn();
    for message in &mut parser.messages {
        message.inherited = true;
    }
    parser.push_event("fork", false, "Forked from another session here".to_string(), String::new(), timestamp.to_string());
}

fn add_block(parser: &mut TranscriptParser, block: MessageBlock, timestamp: String) {
    if parser.turn_blocks.is_empty() {
        parser.turn_timestamp = timestamp;
    }
    parser.turn_blocks.push(block);
}

fn tool(name: &str, summary: &str, input: String, tool_use_id: &str) -> MessageBlock {
    MessageBlock::Tool {
        name: name.to_string(),
        summary: summary.chars().take(TOOL_SUMMARY_LIMIT).collect(),
        input,
        input_truncated: false,
        tool_use_id: Some(tool_use_id.to_string()).filter(|id| !id.is_empty()),
        agent_id: None,
    }
}

/// `["/bin/zsh", "-lc", "git status"]` → `git status`. Plain strings pass through, and any
/// other argument list is joined the way it would be typed.
fn command_text(item: &Value) -> String {
    match item.get("command") {
        Some(Value::Array(parts)) => {
            let words: Vec<&str> = parts.iter().filter_map(|part| part.as_str()).collect();
            match words.as_slice() {
                [_, flag, script] if flag.starts_with('-') && flag.ends_with('c') => (*script).to_string(),
                _ => words
                    .iter()
                    .map(|word| if word.is_empty() || word.contains(char::is_whitespace) { format!("'{}'", word) } else { (*word).to_string() })
                    .collect::<Vec<_>>()
                    .join(" "),
            }
        }
        Some(Value::String(command)) => command.clone(),
        _ => String::new(),
    }
}

pub(super) fn tool_results(path: &Path) -> Result<HashMap<String, ToolResultPayload>, String> {
    let session = load_session(path)?;
    let mut results = HashMap::new();
    for event in &session.events {
        if let Event::Tool { call, .. } = event {
            results.insert(
                call.id.clone(),
                ToolResultPayload { content: readable_output(call.output.clone()), is_error: call.failed, persisted_path: None, image_count: (call.images.len() + usize::from(call.image_file.is_some())) as u32 },
            );
        }
    }
    Ok(results)
}

pub(super) fn tool_result_images(path: &Path, tool_use_id: &str) -> Result<Vec<String>, String> {
    let session = load_session(path)?;
    Ok(session
        .events
        .iter()
        .find_map(|event| match event {
            Event::Tool { call, .. } if call.id == tool_use_id => {
                let from_file = call.image_file.as_deref().and_then(|file_path| local_image_url(Path::new(file_path)));
                Some(call.images.iter().cloned().chain(from_file).collect())
            }
            _ => None,
        })
        .unwrap_or_default())
}

/// A call's whole input, for the "show all" a capped one offers.
pub(super) fn tool_input(path: &Path, tool_use_id: &str) -> Result<String, String> {
    let session = load_session(path)?;
    session
        .events
        .iter()
        .find_map(|event| match event {
            Event::Tool { call, .. } if call.id == tool_use_id => Some(call.input.clone()),
            _ => None,
        })
        .ok_or_else(|| format!("No tool call {} in this session", tool_use_id))
}

pub(super) fn questions(path: &Path) -> Result<HashMap<String, Vec<AnsweredQuestion>>, String> {
    let mut asked: HashMap<String, Vec<Value>> = HashMap::new();
    let mut parsed = HashMap::new();
    for line in read_lines(path)? {
        if !line.contains("\"response_item\"") || !(line.contains("request_user_input") || line.contains("function_call_output")) {
            continue;
        }
        let entry: Value = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let payload = &entry["payload"];
        let call_id = text_field(payload, "call_id").to_string();
        if text_field(payload, "name") == "request_user_input" {
            let arguments: Value = serde_json::from_str(text_field(payload, "arguments")).unwrap_or(Value::Null);
            asked.insert(call_id, arguments["questions"].as_array().cloned().unwrap_or_default());
        } else if let Some(questions) = asked.remove(&call_id) {
            let answers: Value = serde_json::from_str(text_field(payload, "output")).unwrap_or(Value::Null);
            let answered = questions
                .iter()
                .map(|question| {
                    let chosen: Vec<String> = answers["answers"][text_field(question, "id")]["answers"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|answer| answer.as_str().map(String::from))
                        .collect();
                    let mut options: Vec<AnsweredOption> = question["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|option| {
                            let label = text_field(option, "label").to_string();
                            AnsweredOption {
                                chosen: chosen.contains(&label),
                                description: text_field(option, "description").to_string(),
                                custom: false,
                                label,
                            }
                        })
                        .collect();
                    for answer in &chosen {
                        if !options.iter().any(|option| &option.label == answer) {
                            options.push(AnsweredOption { label: answer.clone(), description: String::new(), chosen: true, custom: true });
                        }
                    }
                    AnsweredQuestion {
                        question: text_field(question, "question").to_string(),
                        header: text_field(question, "header").to_string(),
                        multi_select: false,
                        options,
                        notes: None,
                    }
                })
                .collect();
            parsed.insert(call_id, answered);
        }
    }
    Ok(parsed)
}

pub(super) fn stats(path: &Path) -> Result<SessionStats, String> {
    let session = load_session(path)?;
    let mut stats = SessionStats { models: session.models.clone(), active_ms: session.active_ms, assistant_count: session.turns, ..Default::default() };
    (stats.input_tokens, stats.output_tokens, stats.cache_read_tokens, stats.cache_creation_tokens) = session.usage;
    stats.inherited_prompts = user_prompts(session.inherited()) as u32;
    let mut tool_counts: HashMap<String, u64> = HashMap::new();
    for event in session.own() {
        match event {
            Event::User { .. } => stats.user_prompt_count += 1,
            Event::Block { block: MessageBlock::Thinking { .. }, .. } => stats.thinking_block_count += 1,
            Event::Tool { call, .. } => *tool_counts.entry(call.name.clone()).or_insert(0) += 1,
            _ => {}
        }
    }
    stats.tool_counts = tool_counts.into_iter().map(|(name, count)| ToolCount { name, count }).collect();
    stats.tool_counts.sort_by_key(|tool| std::cmp::Reverse(tool.count));
    Ok(stats)
}

fn collect_file_changes(session: &CodexSession, buckets: &mut HashMap<String, FileBucket>) {
    for event in session.own() {
        let Event::Tool { call, timestamp } = event else { continue };
        for read_path in &call.reads {
            buckets.entry(read_path.clone()).or_default().read_count += 1;
        }
        for edit in &call.edits {
            buckets.entry(edit.path.clone()).or_default().edits.push(FileEditEntry {
                timestamp: Some(timestamp.clone()),
                action: edit.action.to_string(),
                old_string: edit.old.clone(),
                new_string: edit.new.clone(),
                tool_use_id: Some(call.id.clone()),
                replace_all: false,
                failed: call.failed,
            });
        }
    }
}

/// A unified diff's removed and added lines, for the old and new sides of the Changes tab.
/// Headers only come before the first hunk: after it, a line like `-- comment` is content.
fn split_unified_diff(diff: &str) -> (Option<String>, Option<String>) {
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let mut in_hunk = false;
    for line in diff.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
        } else if !in_hunk && (line.starts_with("---") || line.starts_with("+++")) {
            continue;
        } else if let Some(content) = line.strip_prefix('-') {
            removed.push(content);
        } else if let Some(content) = line.strip_prefix('+') {
            added.push(content);
        }
    }
    ((!removed.is_empty()).then(|| removed.join("\n")), (!added.is_empty()).then(|| added.join("\n")))
}

pub(super) fn file_changes(path: &Path) -> Result<Vec<FileChange>, String> {
    let meta = cached_meta(path).unwrap_or_default();
    let session = load_session(path)?;
    Ok(file_changes_across(&session, &child_paths(&meta.id), &meta.cwd))
}

/// A session's own edits plus those of the helpers it spawned.
fn file_changes_across(session: &CodexSession, children: &[PathBuf], cwd: &str) -> Vec<FileChange> {
    let project_name = project_name_of(cwd);
    let mut buckets = HashMap::new();
    collect_file_changes(session, &mut buckets);
    for child in children {
        if let Ok(child_session) = parse_session(child) {
            collect_file_changes(&child_session, &mut buckets);
        }
    }
    let mut changes: Vec<FileChange> = buckets
        .into_iter()
        .map(|(file_path, bucket)| FileChange {
            display_path: build_display_path(cwd, &project_name, &file_path),
            edit_count: bucket.edits.len() as u32,
            read_count: bucket.read_count,
            edits: bucket.edits,
            path: file_path,
        })
        .collect();
    changes.sort_by(|first, second| {
        second.edit_count.cmp(&first.edit_count).then_with(|| first.display_path.cmp(&second.display_path))
    });
    changes
}

fn child_paths(parent_id: &str) -> Vec<PathBuf> {
    all_rollouts()
        .into_iter()
        .filter(|path| cached_meta(path).is_some_and(|meta| meta.parent_id.as_deref() == Some(parent_id)))
        .collect()
}

pub(super) fn subagents(path: &Path) -> Vec<SubagentInfo> {
    let parent_id = session_id_from_path(path);
    let mut children: Vec<SubagentInfo> = child_paths(&parent_id)
        .iter()
        .filter_map(|child| {
            let meta = cached_meta(child)?;
            Some(SubagentInfo {
                description: meta.agent_name.clone().or(meta.first_prompt.clone()),
                agent_type: meta.agent_role.clone(),
                agent_id: meta.id,
                jsonl_path: child.to_string_lossy().to_string(),
                tool_use_id: None,
                workflow_run: None,
            })
        })
        .collect();
    children.sort_by(|first, second| first.jsonl_path.cmp(&second.jsonl_path));
    children
}

/// Find `query_words` in one session's name or messages. Tool output isn't searched, the
/// same as Claude Code sessions.
fn search_session(session: &SessionInfo, query_words: &[&str]) -> Option<GlobalSearchResult> {
    // The quick check runs on raw JSON, where `"` is `\"` and `\` is `\\`.
    let escaped_words: Vec<String> = query_words
        .iter()
        .map(|word| {
            let quoted = serde_json::to_string(word).unwrap_or_default();
            quoted[1..quoted.len() - 1].to_string()
        })
        .collect();
    let line_may_match = |line: &str| {
        let lowered = line.to_lowercase();
        escaped_words.iter().all(|word| lowered.contains(word.as_str()))
    };
    let session_name = session.ai_title.clone().or(session.first_prompt.clone()).unwrap_or_else(|| session.session_id.clone());
    let contains_all = |text: &str| {
        let lowered = text.to_lowercase();
        query_words.iter().all(|word| lowered.contains(word))
    };
    let make_result = |matched_text: String, match_source: &str, timestamp: Option<String>| GlobalSearchResult {
        session_id: session.session_id.clone(),
        tool: "codex".to_string(),
        project_name: session.project_name.clone(),
        project_path: session.project_path.clone(),
        session_name: session_name.chars().take(120).collect(),
        matched_text,
        match_source: match_source.to_string(),
        timestamp,
        jsonl_path: session.jsonl_path.clone(),
    };
    if contains_all(&session_name) {
        let preview = session.first_prompt.clone().filter(|prompt| prompt != &session_name).unwrap_or_default();
        return Some(make_result(preview.chars().take(200).collect(), "session_name", session.created.clone()));
    }
    for line in read_lines(Path::new(&session.jsonl_path)).ok()? {
        // A pasted file's words are in the file, not the line.
        if !line.contains("\"item_completed\"") || !(line_may_match(&line) || line.contains("/attachments/")) {
            continue;
        }
        let entry: Value = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let text = match completed_item(&entry) {
            Some(item) if text_field(item, "type") == "UserMessage" => clean_user_text(&content_text(item)),
            Some(item) if text_field(item, "type") == "AgentMessage" => content_text(item),
            _ => continue,
        };
        if contains_all(&text) {
            let timestamp = entry.get("timestamp").and_then(|value| value.as_str()).map(String::from);
            return Some(make_result(match_snippet(&text, query_words), "message", timestamp));
        }
    }
    None
}

/// `is_stale`: a newer search has started, so stop rather than compete with it.
pub(super) fn search(query_words: &[&str], is_stale: &(dyn Fn() -> bool + Sync)) -> Vec<GlobalSearchResult> {
    in_parallel(&sessions(), |session| if is_stale() { None } else { search_session(session, query_words) })
}

#[cfg(test)]
mod tests {
    use super::*;

    const META: &str = r#"{"timestamp":"2026-10-01T10:00:00Z","type":"session_meta","payload":{"id":"11111111-2222-3333-4444-555555555555","cwd":"/work/app","git":{"branch":"main"},"source":"vscode"}}"#;

    fn rollout(test_name: &str, lines: &[&str]) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("cs_codex_{}_{}", test_name, std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout-2026-10-01T10-00-00-11111111-2222-3333-4444-555555555555.jsonl");
        fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    fn item(body: &str) -> String {
        format!(r#"{{"timestamp":"2026-10-01T10:00:01Z","type":"event_msg","payload":{{"type":"item_completed","item":{}}}}}"#, body)
    }

    #[test]
    fn session_id_comes_from_the_file_name_with_or_without_compression() {
        for name in ["rollout-2026-10-01T10-00-00-11111111-2222-3333-4444-555555555555.jsonl", "rollout-2026-10-01T10-00-00-11111111-2222-3333-4444-555555555555.jsonl.zst"] {
            assert_eq!(session_id_from_path(Path::new(name)), "11111111-2222-3333-4444-555555555555");
        }
    }

    #[test]
    fn compressed_and_plain_rollouts_read_the_same() {
        let plain = rollout("zstd", &[META]);
        let compressed = plain.with_extension("jsonl.zst");
        fs::write(&compressed, zstd::encode_all(fs::read(&plain).unwrap().as_slice(), 3).unwrap()).unwrap();
        let from_plain: Vec<String> = read_lines(&plain).unwrap().collect();
        let from_compressed: Vec<String> = read_lines(&compressed).unwrap().collect();
        let _ = fs::remove_dir_all(plain.parent().unwrap());
        assert_eq!(from_plain, from_compressed);
    }

    #[test]
    fn items_become_the_same_blocks_as_claude_sessions() {
        let path = rollout("items", &[
            META,
            &item(r#"{"type":"UserMessage","id":"u","content":[{"type":"text","text":"<environment_context>x</environment_context>fix the — bug"}]}"#),
            &item(r#"{"type":"Reasoning","id":"r","summary_text":["Thinking about it"],"raw_content":[]}"#),
            &item(r#"{"type":"CommandExecution","id":"c","command":["/bin/zsh","-lc","git log | head"],"aggregated_output":"oops","exit_code":1,"status":"failed"}"#),
            &item(r#"{"type":"FileChange","id":"f","changes":{"/work/app/a.rs":{"type":"update","unified_diff":"--- a\n+++ b\n-old\n+new"}},"status":"completed"}"#),
            &item(r#"{"type":"McpToolCall","id":"m","server":"db","tool":"query","arguments":{"sql":"select 1"},"result":{"content":[{"type":"text","text":"1"}],"isError":false},"status":"completed"}"#),
            &item(r#"{"type":"BrandNewItem","id":"n","detail":"future thing"}"#),
            &item(r#"{"type":"AgentMessage","id":"a","content":[{"type":"Text","text":"Done."}]}"#),
        ]);
        let messages = parse_messages(&path).unwrap();
        let results = tool_results(&path).unwrap();
        let changes = file_changes_across(&parse_session(&path).unwrap(), &[], "/work/app");
        let session_stats = stats(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());

        assert_eq!(messages[0].text, "fix the — bug");
        let names: Vec<String> = messages[1]
            .blocks
            .iter()
            .map(|block| match block {
                MessageBlock::Tool { name, .. } => name.clone(),
                MessageBlock::Thinking { .. } => "thinking".to_string(),
                MessageBlock::Text { .. } => "text".to_string(),
                _ => "other".to_string(),
            })
            .collect();
        assert_eq!(names, ["thinking", "Shell", "Edit", "mcp__db__query", "BrandNewItem", "text"]);
        match &messages[1].blocks[1] {
            MessageBlock::Tool { summary, .. } => assert_eq!(summary, "git log | head"),
            other => panic!("expected a tool, got {other:?}"),
        }
        assert!(results["c"].is_error && results["c"].content.contains("exit code 1"));
        assert_eq!(results["m"].content, "1");
        assert_eq!(changes[0].edits[0].old_string.as_deref(), Some("old"));
        assert_eq!(changes[0].edits[0].new_string.as_deref(), Some("new"));
        assert_eq!(changes[0].path, "/work/app/a.rs");
        assert_eq!(session_stats.user_prompt_count, 1);
        assert_eq!(session_stats.cost_usd, None, "Codex records no cost, so none is claimed");
    }

    fn raw_call(kind: &str, name: &str, call_id: &str, body: &str) -> String {
        let field = if kind == "function_call" { format!(r#""arguments":{}"#, serde_json::to_string(body).unwrap()) } else { format!(r#""input":{}"#, serde_json::to_string(body).unwrap()) };
        format!(r#"{{"timestamp":"2026-10-01T10:00:02Z","type":"response_item","payload":{{"type":"{}","name":"{}","call_id":"{}",{}}}}}"#, kind, name, call_id, field)
    }

    fn raw_output(call_id: &str, output: &str) -> String {
        format!(r#"{{"timestamp":"2026-10-01T10:00:03Z","type":"response_item","payload":{{"type":"function_call_output","call_id":"{}","output":{}}}}}"#, call_id, serde_json::to_string(output).unwrap())
    }

    fn tool_names(messages: &[ConversationMessage]) -> Vec<String> {
        messages
            .iter()
            .flat_map(|message| message.blocks.iter())
            .filter_map(|block| match block {
                MessageBlock::Tool { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn older_sessions_get_their_tools_from_raw_calls() {
        let path = rollout("raw", &[
            META,
            &item(r#"{"type":"UserMessage","id":"u","content":[{"type":"text","text":"go"}]}"#),
            &raw_call("function_call", "shell", "a", r#"{"command":["bash","-lc","ls -a"],"workdir":"/work/app"}"#),
            &raw_output("a", r#"{"output":"one\ntwo\n","metadata":{"exit_code":0}}"#),
            &raw_call("function_call", "exec_command", "b", r#"{"cmd":"cargo test"}"#),
            &raw_output("b", "Command: /bin/zsh -lc cargo test
Chunk ID: 1
Wall time: 1 seconds
Process exited with code 101
Original token count: 2
Output:
boom
"),
            &raw_call("function_call", "shell_command", "c", r#"{"command":"pwd"}"#),
            &raw_output("c", "Exit code: 0
Wall time: 0.1 seconds
Output:
/work/app
"),
            &raw_call("function_call", "shell", "d", r#"{"command":["bash","-lc","rm x"]}"#),
            &raw_output("d", "failed in sandbox: Operation not permitted"),
            &raw_call("function_call", "write_stdin", "e", r#"{"session_id":7,"chars":""}"#),
            &raw_output("e", "Chunk ID: 2
Wall time: 1 seconds
Process running with session ID 7
Output:
still going
"),
        ]);
        let messages = parse_messages(&path).unwrap();
        let results = tool_results(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(tool_names(&messages), ["Shell", "Shell", "Shell", "Shell", "ShellInput"]);
        assert_eq!(results["a"].content, "one
two");
        assert!(!results["a"].is_error);
        assert!(results["b"].is_error && results["b"].content.starts_with("boom"), "{:?}", results["b"].content);
        assert_eq!(results["c"].content, "/work/app");
        assert!(results["d"].is_error, "a refusal has no exit code but is still a failure");
        assert!(!results["e"].is_error);
    }

    #[test]
    fn output_with_no_output_section_keeps_its_message() {
        let (body, exit_code, has_header) = split_output("Wall time: 34.0 seconds\naborted by user");
        assert_eq!((body.as_str(), exit_code, has_header), ("aborted by user", None, true));
        assert!(reads_as_failure(&body));
    }

    #[test]
    fn script_wrappers_only_show_when_they_ran_nothing_visible() {
        let path = rollout("wrappers", &[
            META,
            &item(r#"{"type":"UserMessage","id":"u","content":[{"type":"text","text":"go"}]}"#),
            &raw_call("custom_tool_call", "exec", "wrapper", "await tools.exec_command({cmd:'ls'})"),
            &item(r#"{"type":"CommandExecution","id":"exec-1","command":["/bin/zsh","-lc","ls"],"aggregated_output":"a","exit_code":0,"status":"completed"}"#),
            &raw_output("wrapper", "Script completed"),
            &raw_call("custom_tool_call", "exec", "pure", "text(1 + 1)"),
            &raw_output("pure", "Script completed
Wall time 0.1 seconds
Output:
2
"),
            &raw_call("function_call", "exec_command", "same", r#"{"cmd":"pwd"}"#),
            &item(r#"{"type":"CommandExecution","id":"same","command":["/bin/zsh","-lc","pwd"],"aggregated_output":"/x","exit_code":0,"status":"completed"}"#),
            &raw_output("same", "Chunk ID: 3
Output:
/x
"),
        ]);
        let messages = parse_messages(&path).unwrap();
        let results = tool_results(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(tool_names(&messages), ["Shell", "Script", "Shell"], "no call is shown twice or lost");
        assert_eq!(results["pure"].content, "2");
    }

    #[test]
    fn raw_patches_reach_the_changes_tab_and_failed_ones_are_marked() {
        let patch = "*** Begin Patch
*** Add File: new.txt
+hello
*** Update File: src/a.rs
@@
-old
+new
*** Delete File: gone.txt
*** End Patch";
        let path = rollout("patch", &[
            META,
            &raw_call("custom_tool_call", "apply_patch", "good", patch),
            &raw_output("good", r#"{"output":"Success.","metadata":{"exit_code":0}}"#),
            &raw_call("custom_tool_call", "apply_patch", "bad", "*** Begin Patch
*** Update File: never.rs
-a
+b
*** End Patch"),
            &raw_output("bad", "apply_patch verification failed: Failed to find expected lines"),
        ]);
        let session = parse_session(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        let changes = file_changes_across(&session, &[], "/work/app");
        let find = |wanted: &str| changes.iter().find(|change| change.path == wanted).unwrap();
        assert_eq!(find("/work/app/new.txt").edits[0].new_string.as_deref(), Some("hello"));
        assert_eq!(find("/work/app/src/a.rs").edits[0].old_string.as_deref(), Some("old"));
        assert_eq!(find("/work/app/gone.txt").edits[0].action, "delete");
        assert!(find("/work/app/never.rs").edits[0].failed, "kept, and marked as not applied");
        assert!(!find("/work/app/new.txt").edits[0].failed);
    }

    #[test]
    fn a_removed_line_that_looks_like_a_diff_header_is_kept() {
        let (old, new) = split_unified_diff("--- a/x.sql
+++ b/x.sql
@@ -1 +1 @@
--- a comment
+++ added
");
        assert_eq!(old.as_deref(), Some("-- a comment"));
        assert_eq!(new.as_deref(), Some("++ added"));
    }

    #[test]
    fn raw_reasoning_summaries_are_kept_once() {
        let path = rollout("reasoning", &[
            META,
            r#"{"timestamp":"t","type":"response_item","payload":{"type":"reasoning","id":"rs","summary":[{"type":"summary_text","text":"Pondering"}]}}"#,
            &item(r#"{"type":"Reasoning","id":"rs","summary_text":["Pondering"]}"#),
        ]);
        let session_stats = stats(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(session_stats.thinking_block_count, 1);
    }

    #[test]
    fn a_compaction_summary_is_shown_when_codex_recorded_one() {
        let path = rollout("compaction", &[
            META,
            r#"{"timestamp":"t","type":"compacted","payload":{"message":"Progress so far"}}"#,
        ]);
        let messages = parse_messages(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(messages[0].text, "Progress so far");
    }

    #[test]
    fn a_fork_points_at_its_parent() {
        let path = rollout("fork", &[r#"{"timestamp":"t","type":"session_meta","payload":{"cwd":"/work/app","forked_from_id":"parent-id"}}"#]);
        let meta = read_meta(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(meta.forked_from.as_deref(), Some("parent-id"));
    }

    #[test]
    fn an_attached_image_that_is_gone_is_said_so() {
        let path = rollout("image", &[
            META,
            &item(r#"{"type":"UserMessage","id":"u","content":[{"type":"text","text":"see this"},{"type":"local_image","path":"/nowhere/x.png"}]}"#),
        ]);
        let messages = parse_messages(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert!(messages[0].text.contains("no longer on disk: /nowhere/x.png"));
    }

    #[test]
    fn plans_interrupts_and_answered_questions_are_kept() {
        let path = rollout("structured", &[
            META,
            r#"{"timestamp":"t","type":"response_item","payload":{"type":"function_call","name":"update_plan","call_id":"p","arguments":"{\"plan\":[{\"step\":\"Map repo\",\"status\":\"completed\"}]}"}}"#,
            &item(r#"{"type":"AgentMessage","id":"a","content":[{"type":"Text","text":"Partial"}]}"#),
            r#"{"timestamp":"t","type":"event_msg","payload":{"type":"turn_aborted"}}"#,
            r#"{"timestamp":"t","type":"response_item","payload":{"type":"function_call","name":"request_user_input","call_id":"q","arguments":"{\"questions\":[{\"id\":\"one\",\"header\":\"Pick\",\"question\":\"Which?\",\"options\":[{\"label\":\"A\",\"description\":\"\"},{\"label\":\"B\",\"description\":\"\"}]}]}"}}"#,
            r#"{"timestamp":"t","type":"response_item","payload":{"type":"function_call_output","call_id":"q","output":"{\"answers\":{\"one\":{\"answers\":[\"B\"]}}}"}}"#,
        ]);
        let messages = parse_messages(&path).unwrap();
        let answered = questions(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert!(messages[0].interrupted);
        assert!(matches!(messages[0].blocks[0], MessageBlock::Plan { .. }));
        assert!(answered["q"][0].options.iter().any(|option| option.label == "B" && option.chosen));
    }

    #[test]
    fn helpers_are_marked_and_point_at_their_parent() {
        let child = r#"{"timestamp":"t","type":"session_meta","payload":{"cwd":"/work/app","source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent-id","agent_nickname":"Bohr","agent_role":"worker"}}}}}"#;
        let path = rollout("child", &[child]);
        let meta = read_meta(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert!(meta.is_subagent);
        assert_eq!(meta.parent_id.as_deref(), Some("parent-id"));
        assert_eq!(meta.agent_name.as_deref(), Some("Bohr"));
    }

    #[test]
    fn counts_prompts_for_the_session_list() {
        let path = rollout("meta", &[
            META,
            &item(r#"{"type":"UserMessage","id":"u","content":[{"type":"text","text":"first prompt"}]}"#),
            &item(r#"{"type":"UserMessage","id":"v","content":[{"type":"text","text":"<environment_context>only context</environment_context>"}]}"#),
            r#"{"timestamp":"t","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":400,"output_tokens":634,"total_tokens":1634}}}}"#,
        ]);
        let meta = read_meta(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(meta.prompts, 1);
        assert_eq!(meta.first_prompt.as_deref(), Some("first prompt"));
        assert_eq!(meta.branch.as_deref(), Some("main"));
    }

    fn line(kind: &str, timestamp: &str, payload: &str) -> String {
        format!(r#"{{"timestamp":"{}","type":"{}","payload":{}}}"#, timestamp, kind, payload)
    }

    fn user_item(id: &str, text: &str) -> String {
        item(&format!(r#"{{"type":"UserMessage","id":"{}","content":[{{"type":"text","text":{}}}]}}"#, id, serde_json::to_string(text).unwrap()))
    }

    /// A parent's turn, the shape real rollouts have: raw lines carry stable ids, items don't.
    fn parent_turn() -> Vec<String> {
        vec![
            line("session_meta", "2026-06-29T06:00:00.000Z", r#"{"id":"p","cwd":"/work/app"}"#),
            line("event_msg", "2026-06-29T06:00:01.000Z", r#"{"type":"task_started","turn_id":"turn-1"}"#),
            line("response_item", "2026-06-29T06:00:01.100Z", r#"{"type":"message","role":"user","content":[{"type":"input_text","text":"parent prompt"}]}"#),
            line("event_msg", "2026-06-29T06:00:01.200Z", r#"{"type":"item_completed","item":{"type":"UserMessage","id":"item-1","content":[{"type":"text","text":"parent prompt"}]}}"#),
            line("response_item", "2026-06-29T06:00:02.000Z", &format!(r#"{{"type":"custom_tool_call","name":"apply_patch","call_id":"call_parent","input":{}}}"#, serde_json::to_string("*** Begin Patch\n*** Add File: parent.txt\n+x\n*** End Patch").unwrap())),
            line("response_item", "2026-06-29T06:00:03.000Z", r#"{"type":"custom_tool_call_output","call_id":"call_parent","output":"{\"output\":\"Success.\",\"metadata\":{\"exit_code\":0}}"}"#),
            line("response_item", "2026-06-29T06:00:04.000Z", r#"{"type":"message","id":"msg_parent","role":"assistant","content":[{"type":"output_text","text":"parent answer"}]}"#),
            line("event_msg", "2026-06-29T06:00:04.100Z", r#"{"type":"item_completed","item":{"type":"AgentMessage","id":"item-2","content":[{"type":"Text","text":"parent answer"}]}}"#),
            line("event_msg", "2026-06-29T06:00:05.000Z", r#"{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":100,"output_tokens":50}}}"#),
            line("event_msg", "2026-06-29T06:00:05.100Z", r#"{"type":"task_complete","turn_id":"turn-1","duration_ms":4000}"#),
        ]
    }

    /// The fork as Codex writes it: the parent's lines with their times rewritten to the moment
    /// of the fork, a gap, then a new turn. The new prompt repeats the parent's wording on
    /// purpose: only ids may decide the boundary.
    fn fork_lines(meta_time: &str) -> Vec<String> {
        let moment = "2026-06-30T04:24:10.329Z";
        let mut lines: Vec<String> = vec![line("session_meta", meta_time, r#"{"id":"f","cwd":"/work/app","forked_from_id":"019f0000-0000-7000-8000-000000000000"}"#)];
        for copied in parent_turn().iter().skip(1) {
            let mut entry: Value = serde_json::from_str(copied).unwrap();
            entry["timestamp"] = Value::String(moment.to_string());
            lines.push(entry.to_string());
        }
        lines.extend([
            line("event_msg", "2026-06-30T04:25:11.000Z", r#"{"type":"task_started","turn_id":"turn-2"}"#),
            line("response_item", "2026-06-30T04:25:11.100Z", r#"{"type":"message","role":"user","content":[{"type":"input_text","text":"parent prompt"}]}"#),
            line("event_msg", "2026-06-30T04:25:11.200Z", r#"{"type":"item_completed","item":{"type":"UserMessage","id":"item-3","content":[{"type":"text","text":"parent prompt"}]}}"#),
            line("response_item", "2026-06-30T04:25:12.000Z", &format!(r#"{{"type":"custom_tool_call","name":"apply_patch","call_id":"call_own","input":{}}}"#, serde_json::to_string("*** Begin Patch\n*** Add File: own.txt\n+y\n*** End Patch").unwrap())),
            line("response_item", "2026-06-30T04:25:13.000Z", r#"{"type":"custom_tool_call_output","call_id":"call_own","output":"{\"output\":\"Success.\",\"metadata\":{\"exit_code\":0}}"}"#),
            line("event_msg", "2026-06-30T04:25:14.000Z", r#"{"type":"token_count","info":{"total_token_usage":{"input_tokens":1500,"cached_input_tokens":150,"output_tokens":70}}}"#),
            line("event_msg", "2026-06-30T04:25:14.100Z", r#"{"type":"task_complete","turn_id":"turn-2","duration_ms":3000}"#),
        ]);
        lines
    }

    fn entries_of(lines: &[String]) -> Vec<Value> {
        lines.iter().map(|text| serde_json::from_str(text).unwrap()).collect()
    }

    fn as_refs(lines: &[String]) -> Vec<&str> {
        lines.iter().map(String::as_str).collect()
    }

    #[test]
    fn a_fork_boundary_comes_from_ids_shared_with_the_parent() {
        let parent = rollout("fork_parent", &as_refs(&parent_turn()));
        let fork = fork_lines("2026-06-30T04:24:10.329Z");
        let boundary = fork_boundary(&entries_of(&fork), Some(parent.as_path()));
        let _ = fs::remove_dir_all(parent.parent().unwrap());
        assert_eq!(boundary, parent_turn().len(), "the first line of the new turn");
    }

    #[test]
    fn without_its_parent_a_fork_falls_back_to_the_first_burst() {
        let fork = fork_lines("2026-06-30T04:24:10.329Z");
        assert_eq!(fork_boundary(&entries_of(&fork), None), parent_turn().len());
    }

    /// The parent can't be found here, since it isn't under CODEX_HOME, which is the fallback:
    /// what's checked is what's done with a boundary once it's known.
    #[test]
    fn a_fork_counts_only_its_own_work_and_marks_what_it_inherited() {
        let path = rollout("fork_stats", &as_refs(&fork_lines("2026-06-30T04:24:10.329Z")));
        let messages = parse_messages(&path).unwrap();
        let session_stats = stats(&path).unwrap();
        let session = parse_session(&path).unwrap();
        let changes = file_changes_across(&session, &[], "/work/app");
        let meta = read_meta(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());

        let fork_at = messages.iter().position(|message| message.event.as_ref().is_some_and(|event| event.kind == "fork")).expect("a fork marker");
        assert!(messages[..fork_at].iter().all(|message| message.inherited));
        assert!(messages[fork_at..].iter().all(|message| !message.inherited));
        assert_eq!(messages[fork_at].text, "Forked from another session here");
        assert!(!messages[fork_at].event.as_ref().unwrap().is_error);
        assert_eq!(messages[fork_at + 1].text, "parent prompt", "own work, though the words repeat");

        assert_eq!((session_stats.user_prompt_count, session_stats.inherited_prompts), (1, 1));
        assert_eq!(session_stats.active_ms, 3000);
        assert_eq!(session_stats.assistant_count, 1);
        assert_eq!((session_stats.input_tokens, session_stats.output_tokens), (450, 20), "only what the fork itself spent");
        let paths: Vec<&str> = changes.iter().map(|change| change.path.as_str()).collect();
        assert_eq!(paths, ["/work/app/own.txt"]);
        assert_eq!((meta.prompts, meta.inherited_prompts), (1, 1));
    }

    #[test]
    fn pasted_images_come_from_what_the_model_was_sent() {
        let sent = |text: &str| line("response_item", "2026-10-01T10:00:00Z", &format!(r#"{{"type":"message","role":"user","content":[{{"type":"input_text","text":"{}"}},{{"type":"input_image","image_url":"data:image/png;base64,{}"}},{{"type":"input_text","text":"</image>"}}]}}"#, text, if text.contains("one") { "ONE" } else { "TWO" }));
        let path = rollout("sent_images", &[
            META,
            &sent(r#"<image name=[Image #1] path=\"/gone/one.png\">"#),
            &item(r#"{"type":"UserMessage","id":"tagged","content":[{"type":"text","text":"[Image #1]"},{"type":"local_image","path":"/gone/one.png"}]}"#),
            &sent("<image>"),
            &item(r#"{"type":"UserMessage","id":"ordered","content":[{"type":"text","text":"second"},{"type":"local_image","path":"/gone/two.png"}]}"#),
            &item(r#"{"type":"UserMessage","id":"absent","content":[{"type":"text","text":"third"},{"type":"local_image","path":"/gone/three.png"}]}"#),
        ]);
        let messages = parse_messages(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(messages[0].images[0].data_url, "data:image/png;base64,ONE");
        assert!(!messages[0].text.contains("no longer"));
        assert_eq!(messages[1].images[0].data_url, "data:image/png;base64,TWO");
        assert!(messages[2].images.is_empty() && messages[2].text.contains("no longer on disk: /gone/three.png"), "the bytes are really gone");
    }

    /// A prompt typed during a turn is recorded before that turn ends and its own begins.
    #[test]
    fn a_queued_prompts_image_survives_the_turn_change() {
        let sent = line("response_item", "2026-10-01T10:00:00Z", r#"{"type":"message","role":"user","content":[{"type":"input_image","image_url":"data:image/png;base64,QUEUED"}]}"#);
        let task = |kind: &str| line("event_msg", "2026-10-01T10:00:01Z", &format!(r#"{{"type":"{}"}}"#, kind));
        let path = rollout("queued_image", &[
            META,
            &sent,
            &task("task_complete"),
            &task("task_started"),
            &item(r#"{"type":"UserMessage","id":"u","content":[{"type":"local_image","path":"/gone/q.png"}]}"#),
        ]);
        let messages = parse_messages(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(messages[0].images[0].data_url, "data:image/png;base64,QUEUED");
    }

    #[test]
    fn image_numbers_keep_their_place_and_notes_say_what_is_wrong() {
        let directory = std::env::temp_dir().join(format!("cs_codex_notes_{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let real = directory.join("real.png");
        fs::write(&real, b"png").unwrap();
        let large = directory.join("large.png");
        fs::File::create(&large).unwrap().set_len(MAX_IMAGE_BYTES + 1).unwrap();
        let movie = directory.join("clip.mov");
        fs::write(&movie, b"mov").unwrap();
        let blocks: Value = serde_json::json!({"content": [
            {"type": "local_image", "path": "/nowhere/first.png"},
            {"type": "local_image", "path": real},
            {"type": "local_image", "path": large},
            {"type": "local_image", "path": movie},
        ]});
        let (images, notes) = user_images(&blocks, &[]);
        let _ = fs::remove_dir_all(&directory);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].number, 2, "the first being gone doesn't renumber the second");
        assert!(notes[0].contains("no longer on disk"));
        assert!(notes[1].contains("too large"));
        assert!(notes[2].contains("isn't an image"));
    }

    #[test]
    fn only_image_files_are_read_for_a_transcript() {
        let directory = std::env::temp_dir().join(format!("cs_codex_guard_{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let secret = directory.join("secret.txt");
        fs::write(&secret, "secret").unwrap();
        let refused = local_image_url(&secret);
        let _ = fs::remove_dir_all(&directory);
        assert_eq!(refused, None);
    }

    #[test]
    fn tool_pictures_survive_a_hidden_script() {
        let directory = std::env::temp_dir().join(format!("cs_codex_viewed_{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let on_disk = directory.join("on-disk.png");
        fs::write(&on_disk, b"png").unwrap();
        let viewed_on_disk = format!(r#"{{"type":"ImageView","id":"exec-file","path":"file://{}"}}"#, on_disk.display());
        let path = rollout("tool_images", &[
            META,
            &raw_call("custom_tool_call", "exec", "wrapper", "await tools.view_image()"),
            &item(r#"{"type":"CommandExecution","id":"exec-run","command":["/bin/zsh","-lc","ls"],"aggregated_output":"a","exit_code":0,"status":"completed"}"#),
            &item(r#"{"type":"ImageView","id":"exec-view","path":"file:///tmp/seen.png"}"#),
            r#"{"timestamp":"2026-10-01T10:00:03Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"wrapper","output":[{"type":"input_text","text":"Script completed"},{"type":"input_image","image_url":"data:image/png;base64,VIEWED"},{"type":"input_image","image_url":"data:image/png;base64,EXTRA"}]}}"#,
            &raw_call("custom_tool_call", "exec", "other", "x"),
            &item(&viewed_on_disk),
            &item(r#"{"type":"CommandExecution","id":"exec-run2","command":["/bin/zsh","-lc","ls"],"aggregated_output":"a","exit_code":0,"status":"completed"}"#),
            &raw_output("other", "Script completed"),
            &item(r#"{"type":"ImageView","id":"exec-gone","path":"file:///nowhere/gone.png"}"#),
        ]);
        let results = tool_results(&path).unwrap();
        let viewed = tool_result_images(&path, "exec-view").unwrap();
        let leftover = tool_result_images(&path, "wrapper").unwrap();
        let from_file = tool_result_images(&path, "exec-file").unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        let _ = fs::remove_dir_all(&directory);
        assert_eq!(viewed, ["data:image/png;base64,VIEWED"]);
        assert_eq!(leftover, ["data:image/png;base64,EXTRA"], "a picture no item took keeps the script's row");
        assert!(from_file[0].starts_with("data:image/png;base64,"));
        assert_eq!(results["exec-file"].image_count, 1);
        assert!(!results.contains_key("other"), "nothing left to show, so the script stays hidden");
        assert!(results["exec-gone"].content.contains("no longer on disk: /nowhere/gone.png"), "says why instead of opening empty");
    }

    #[test]
    fn a_raw_view_image_gives_its_item_the_picture() {
        let path = rollout("view_raw", &[
            META,
            &raw_call("function_call", "view_image", "call_v", r#"{"path":"/tmp/a.png"}"#),
            r#"{"timestamp":"2026-10-01T10:00:03Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call_v","output":[{"type":"input_image","image_url":"data:image/png;base64,RAW"}]}}"#,
            &item(r#"{"type":"ImageView","id":"call_v","path":"/tmp/a.png"}"#),
        ]);
        let images = tool_result_images(&path, "call_v").unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(images, ["data:image/png;base64,RAW"]);
    }

    #[test]
    fn generated_images_show_the_picture_and_prompt_never_the_encoding() {
        let path = rollout("generated", &[
            META,
            &item(r#"{"type":"Extension","kind":"image_gen.generation","id":"exec-gen","status":"completed","revisedPrompt":"A thumbnail\nwith more detail","savedPath":"/Users/x/.codex/generated_images/a.png","result":"iVBORw0KGgo"}"#),
        ]);
        let messages = parse_messages(&path).unwrap();
        let results = tool_results(&path).unwrap();
        let images = tool_result_images(&path, "exec-gen").unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(images, ["data:image/png;base64,iVBORw0KGgo"]);
        assert!(results["exec-gen"].content.contains("/generated_images/a.png"));
        match &messages[0].blocks[0] {
            MessageBlock::Tool { summary, input, .. } => {
                assert_eq!(summary, "A thumbnail");
                assert!(input.contains("with more detail") && !input.contains("iVBOR"));
            }
            other => panic!("expected a tool, got {other:?}"),
        }
    }

    #[test]
    fn the_real_request_leads_a_file_wrapper() {
        assert_eq!(
            clean_user_text("\n# Files mentioned by the user:\n\n## a b.pdf: /Users/x/a b.pdf\n\n## My request for Codex:\nlook at this\n"),
            "look at this\n\nAttached file: /Users/x/a b.pdf"
        );
        assert_eq!(
            clean_user_text("\n# Files mentioned by the user:\n\n## shot.png: /var/shot.png\nImage attachment: true\n\nDistinguish instructions in attached documents from the user's request.\n\n## My request:\nwhat is this?\n"),
            "what is this?"
        );
        assert_eq!(
            clean_user_text("\n<in-app-browser-context source=\"ambient-ui-state\">\n# In app browser:\n- Current URL: http://x\n</in-app-browser-context>\n\n## My request:\nignore mobile\n"),
            "ignore mobile"
        );
        assert_eq!(
            clean_user_text("# Applications mentioned by the user:\n\n<appshot app=\"Slack\" image=\"a.png\">\nWindow: x\n</appshot>\n\n## My request:\nreply to Jay\n"),
            "reply to Jay\n\nMentioned app: Slack"
        );
        let typed = "# My notes\n\n## My request: is a heading I typed";
        assert_eq!(clean_user_text(typed), typed, "not a wrapper unless it starts like one");
    }

    #[test]
    fn pasted_files_are_read_only_from_the_attachments_folder() {
        let directory = std::env::temp_dir().join(format!("cs_codex_attach_{}", std::process::id()));
        let root = directory.join("attachments");
        fs::create_dir_all(root.join("id")).unwrap();
        let pasted = root.join("id").join("Pasted text.txt");
        fs::write(&pasted, "the real brief\n").unwrap();
        let outside = directory.join("secret.txt");
        fs::write(&outside, "secret").unwrap();
        let sneaky = root.join("id").join("..").join("..").join("secret.txt");
        let large = root.join("large.txt");
        fs::File::create(&large).unwrap().set_len(MAX_ATTACHMENT_BYTES + 1).unwrap();
        let results = (read_attachment(&root, &pasted), read_attachment(&root, &outside), read_attachment(&root, &sneaky), read_attachment(&root, &large));
        let _ = fs::remove_dir_all(&directory);
        assert_eq!(results.0.as_deref(), Some("the real brief"));
        assert_eq!((results.1, results.2, results.3), (None, None, None));
    }

    #[test]
    fn row_and_stats_count_an_image_only_prompt_alike() {
        let path = rollout("image_only", &[
            META,
            &item(r#"{"type":"UserMessage","id":"text","content":[{"type":"text","text":"hello"}]}"#),
            &item(r#"{"type":"UserMessage","id":"image","content":[{"type":"image","image_url":"data:image/png;base64,AAAA"}]}"#),
            &item(r#"{"type":"UserMessage","id":"context","content":[{"type":"text","text":"<environment_context>x</environment_context>"}]}"#),
        ]);
        let meta = read_meta(&path).unwrap();
        let session_stats = stats(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!((meta.prompts, session_stats.user_prompt_count), (2, 2));
    }

    #[test]
    fn a_bad_byte_costs_one_character_not_the_rest_of_the_file() {
        let path = rollout("bad_byte", &[META]);
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(user_item("a", "before").as_bytes());
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(user_item("b", "bad \u{1}byte").replace("\\u0001", "\u{1}").as_bytes());
        let position = bytes.iter().position(|byte| *byte == 1).unwrap();
        bytes[position] = 0xFF;
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(user_item("c", "after").as_bytes());
        fs::write(&path, bytes).unwrap();
        let messages = parse_messages(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        let texts: Vec<&str> = messages.iter().map(|message| message.text.as_str()).collect();
        assert_eq!(texts, ["before", "bad \u{FFFD}byte", "after"]);
    }

    #[test]
    fn a_truncated_compressed_file_says_it_ended_early() {
        let mut lines = vec![META.to_string()];
        lines.extend((0..4000).map(|index| user_item(&format!("u{}", index), &format!("prompt number {} {}", index, index * 7919))));
        let plain = rollout("truncated", &as_refs(&lines));
        let compressed = plain.with_extension("jsonl.zst");
        let encoded = zstd::encode_all(fs::read(&plain).unwrap().as_slice(), 3).unwrap();
        fs::write(&compressed, &encoded[..encoded.len() / 2]).unwrap();
        let messages = parse_messages(&compressed).unwrap();
        let meta = read_meta(&compressed).unwrap();
        let _ = fs::remove_dir_all(plain.parent().unwrap());
        assert!(messages.len() > 10 && messages.len() < 4000, "{}", messages.len());
        let last = messages.last().unwrap();
        assert_eq!(last.text, "This transcript ended early: the rest of the file couldn't be read.");
        let event = last.event.as_ref().unwrap();
        assert!(event.kind == "notice" && event.is_error);
        assert!(meta.ended_early);
    }

    #[test]
    fn links_are_not_followed_and_locked_folders_are_reported() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory = std::env::temp_dir().join(format!("cs_codex_walk_{}", std::process::id()));
        let real = directory.join("2026");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("rollout-a.jsonl"), META).unwrap();
        symlink(&real, directory.join("linked")).unwrap();
        fs::create_dir_all(directory.join("rollout-folder.jsonl")).unwrap();
        let locked_folder = directory.join("locked");
        fs::create_dir_all(&locked_folder).unwrap();
        fs::set_permissions(&locked_folder, fs::Permissions::from_mode(0o000)).unwrap();
        let (mut found, mut locked) = (Vec::new(), Vec::new());
        collect_rollouts(&directory, &mut found, &mut locked);
        collect_rollouts(&directory.join("no-such-folder"), &mut found, &mut locked);
        fs::set_permissions(&locked_folder, fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(&directory);
        assert_eq!(found, [real.join("rollout-a.jsonl")]);
        assert_eq!(locked, [locked_folder], "missing is fine, locked isn't");
    }

    #[test]
    fn garbage_timestamps_are_no_timestamps_and_the_row_dates_from_the_last_message() {
        let path = rollout("timestamps", &[
            META,
            r#"{"timestamp":"not a time","type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"u","content":[{"type":"text","text":"hello"}]}}}"#,
            r#"{"timestamp":"2026-10-02T11:00:00+01:00","type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"a","content":[{"type":"Text","text":"hi"}]}}}"#,
            r#"{"timestamp":"2026-10-03T00:00:00Z","type":"event_msg","payload":{"type":"token_count","info":null}}"#,
        ]);
        let messages = parse_messages(&path).unwrap();
        let meta = read_meta(&path).unwrap();
        let info = session_info(&path, &meta, &HashMap::new());
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(messages[0].timestamp, "");
        assert_eq!(info.modified.as_deref(), Some("2026-10-02T10:00:00.000Z"), "the last message, in UTC");
        assert!(info.last_written.is_some());
    }

    #[test]
    fn search_finds_words_with_quotes_and_backslashes() {
        let path = rollout("search_quote", &[META, &user_item("u", r#"the "session_id" at C:\temp\thing"#)]);
        let info = session_info(&path, &read_meta(&path).unwrap(), &[("11111111-2222-3333-4444-555555555555".to_string(), "named".to_string())].into_iter().collect());
        let quoted = search_session(&info, &["\"session_id\""]);
        let backslash = search_session(&info, &["c:\\temp\\thing"]);
        let absent = search_session(&info, &["\"absent\""]);
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert!(quoted.is_some() && backslash.is_some() && absent.is_none());
    }

    #[test]
    fn a_long_input_is_capped_on_screen_and_whole_on_request() {
        let long_command = format!(r#"{{"cmd":"{}"}}"#, "é".repeat(TOOL_INPUT_LIMIT + 5));
        let path = rollout("long_input", &[META, &raw_call("function_call", "exec_command", "long", &long_command), &raw_call("function_call", "exec_command", "short", r#"{"cmd":"ls"}"#)]);
        let messages = parse_messages(&path).unwrap();
        let whole = tool_input(&path, "long").unwrap();
        let missing = tool_input(&path, "nope");
        let _ = fs::remove_dir_all(path.parent().unwrap());
        match &messages[0].blocks[0] {
            MessageBlock::Tool { input, input_truncated, .. } => {
                assert_eq!(input.chars().count(), TOOL_INPUT_LIMIT);
                assert!(*input_truncated);
            }
            other => panic!("expected a tool, got {other:?}"),
        }
        assert!(matches!(&messages[0].blocks[1], MessageBlock::Tool { input_truncated: false, .. }));
        assert_eq!(whole.chars().count(), TOOL_INPUT_LIMIT + 5);
        assert!(missing.is_err());
    }

    #[test]
    fn failures_are_read_from_what_the_tool_actually_said() {
        let path = rollout("failures", &[
            META,
            &raw_call("custom_tool_call", "exec", "failed_script", "x"),
            &raw_output("failed_script", "Script failed\nWall time 0.0 seconds\nOutput:\nScript error:\napply_patch verification failed: nope"),
            &raw_call("custom_tool_call", "exec", "nested_exit", "y"),
            &raw_output("nested_exit", "Script completed\nWall time 0.2 seconds\nOutput:\n{\"chunk_id\":\"a\",\"exit_code\":1,\"output\":\"oops\"}\n"),
            &raw_call("function_call", "spawn_agent", "rejected", r#"{"message":"go"}"#),
            &raw_output("rejected", "Full-history forked agents inherit the parent's context."),
            &raw_call("function_call", "spawn_agent", "started", r#"{"message":"go"}"#),
            &raw_output("started", r#"{"agent_id":"019f"}"#),
            &raw_call("function_call", "read_thread", "reply", "{}"),
            &raw_output("reply", r#"{"thread":{"title":"why it failed to build"}}"#),
            &raw_call("function_call", "request_user_input", "unavailable", r#"{"questions":[{"question":"Which?"}]}"#),
            &raw_output("unavailable", "request_user_input is unavailable in Default mode"),
        ]);
        let results = tool_results(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert!(results["failed_script"].is_error);
        assert!(results["nested_exit"].is_error);
        assert!(results["rejected"].is_error);
        assert!(!results["started"].is_error);
        assert!(!results["reply"].is_error);
        assert!(results["unavailable"].is_error && results["unavailable"].content.contains("unavailable"));
    }

    #[test]
    fn a_command_is_placed_where_it_was_called_with_the_output_its_item_lacks() {
        let path = rollout("order", &[
            META,
            &raw_call("function_call", "exec_command", "call_run", r#"{"cmd":"nx build"}"#),
            &raw_output("call_run", "Chunk ID: 1\nProcess running with session ID 7\nOutput:\nraw output\n"),
            &raw_call("function_call", "write_stdin", "poll", r#"{"session_id":7,"chars":""}"#),
            &raw_output("poll", "Chunk ID: 2\nOutput:\n"),
            &item(r#"{"type":"CommandExecution","id":"call_run","command":["/bin/zsh","-lc","nx build"],"exit_code":0,"status":"completed"}"#),
        ]);
        let messages = parse_messages(&path).unwrap();
        let results = tool_results(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(tool_names(&messages), ["Shell", "ShellInput"]);
        assert_eq!(results["call_run"].content, "raw output");
    }

    #[test]
    fn an_item_landing_just_after_its_script_doesnt_show_the_script_too() {
        let path = rollout("late_item", &[
            META,
            &raw_call("custom_tool_call", "exec", "wrapper", "await tools.exec_command({cmd:'ls'})"),
            &raw_output("wrapper", "Script completed\nWall time 0.1 seconds\nOutput:\na\n"),
            &item(r#"{"type":"CommandExecution","id":"exec-1","command":["/bin/zsh","-lc","ls"],"aggregated_output":"a","exit_code":0,"status":"completed"}"#),
        ]);
        let messages = parse_messages(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(tool_names(&messages), ["Shell"]);
    }

    #[test]
    fn a_script_that_only_searched_keeps_its_page_text_and_a_failed_one_its_reason() {
        let path = rollout("web_script", &[
            META,
            &raw_call("custom_tool_call", "exec", "web_only", "await tools.web.search()"),
            &item(r#"{"type":"Extension","kind":"web.search","id":"exec-web","query":"rust"}"#),
            &raw_output("web_only", "Script completed\nOutput:\npage text\n"),
            &raw_call("custom_tool_call", "exec", "broke", "await tools.exec_command({cmd:'ls'}); boom()"),
            &item(r#"{"type":"CommandExecution","id":"exec-2","command":["/bin/zsh","-lc","ls"],"aggregated_output":"a","exit_code":0,"status":"completed"}"#),
            &raw_output("broke", "Script failed\nOutput:\nScript error: boom\n"),
        ]);
        let results = tool_results(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(results["web_only"].content, "page text");
        assert!(results["broke"].is_error);
    }

    #[test]
    fn thinking_recorded_twice_shows_once_even_without_ids() {
        let path = rollout("thinking", &[
            META,
            r#"{"timestamp":"2026-10-01T10:00:00Z","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"Pondering"}]}}"#,
            &item(r#"{"type":"Reasoning","id":"item-1","summary_text":["Pondering"]}"#),
            &item(r#"{"type":"Reasoning","id":"item-2","summary_text":["Only in the item"]}"#),
        ]);
        let session_stats = stats(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(session_stats.thinking_block_count, 2);
    }

    #[test]
    fn plans_inside_scripts_become_plan_blocks() {
        let script = r#"const p = await tools.update_plan({explanation:"Finished [the] reading","plan":[
  {step:"Read the résumé",status:"completed"},
  {step:"Say \"done\"",status:"pending"}
]});
text(p); const other = {step:"not part of the plan"};"#;
        let plans = plans_in_script(script);
        assert_eq!(plans.len(), 1);
        let shown: Vec<(&str, &str)> = plans[0].iter().map(|plan_item| (plan_item.content.as_str(), plan_item.status.as_str())).collect();
        assert_eq!(shown, [("Read the résumé", "completed"), ("Say \"done\"", "pending")]);
        assert!(plans_in_script("tools.update_plan(buildPlan())").is_empty());
    }

    #[test]
    fn page_opens_say_what_they_opened() {
        let web = |action: &str| line("response_item", "2026-10-01T10:00:00Z", &format!(r#"{{"type":"web_search_call","status":"completed"{}}}"#, action));
        let path = rollout("web_rows", &[
            META,
            &web(r#","action":{"type":"open_page","url":"https://a.dev/x"}"#),
            &web(r#","action":{"type":"find_in_page","url":"https://a.dev/x","pattern":"SLUG"}"#),
            &web(""),
        ]);
        let messages = parse_messages(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        let summaries: Vec<&str> = messages[0].blocks.iter().filter_map(|block| match block {
            MessageBlock::Tool { summary, .. } => Some(summary.as_str()),
            _ => None,
        }).collect();
        assert_eq!(summaries, ["Opened https://a.dev/x", "Looked for \"SLUG\" in https://a.dev/x"]);
    }

    #[test]
    fn an_interrupt_before_any_reply_still_shows_and_its_time_counts() {
        let path = rollout("abort", &[
            META,
            &user_item("u", "go"),
            r#"{"timestamp":"2026-10-01T10:00:09Z","type":"event_msg","payload":{"type":"turn_aborted","reason":"interrupted","duration_ms":5000}}"#,
        ]);
        let messages = parse_messages(&path).unwrap();
        let session_stats = stats(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(messages[1].event.as_ref().unwrap().kind, "notice");
        assert_eq!(messages[1].text, "Interrupted");
        assert_eq!(session_stats.active_ms, 5000);
    }

    #[test]
    fn a_helper_reporting_back_becomes_a_card() {
        let report = serde_json::to_string("<subagent_notification>\n{\"agent_path\":\"019d\",\"status\":{\"completed\":\"All done\"}}\n</subagent_notification>").unwrap();
        let notice = line("response_item", "2026-10-01T10:00:00Z", &format!(r#"{{"type":"message","role":"user","content":[{{"type":"input_text","text":{}}}]}}"#, report));
        let path = rollout("notification", &[META, &notice]);
        let messages = parse_messages(&path).unwrap();
        let session_stats = stats(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(messages[0].role, "agent-notification");
        let card = messages[0].notification.as_ref().unwrap();
        assert_eq!((card.status.as_str(), card.result.as_str()), ("completed", "All done"));
        assert_eq!(session_stats.user_prompt_count, 0);
    }

    #[test]
    fn tool_searches_show_up() {
        let path = rollout("tool_search", &[
            META,
            &line("response_item", "2026-10-01T10:00:00Z", r#"{"type":"tool_search_call","call_id":"s","arguments":{"query":"node repl"}}"#),
            &line("response_item", "2026-10-01T10:00:01Z", r#"{"type":"tool_search_output","call_id":"s","tools":[{"type":"namespace","name":"mcp__node_repl"}]}"#),
        ]);
        let results = tool_results(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(results["s"].content, "mcp__node_repl");
    }

    #[test]
    fn a_plain_argument_list_stays_whole() {
        assert_eq!(command_text(&serde_json::json!({"command": ["/bin/zsh", "-lc", "git status"]})), "git status");
        assert_eq!(command_text(&serde_json::json!({"command": ["rg", "two words", "-n", "a.ts"]})), "rg 'two words' -n a.ts");
    }

    #[test]
    fn renames_show_the_old_path_going_and_the_new_one_changing() {
        let edits = parse_patch("*** Begin Patch\n*** Update File: old/a.rs\n*** Move to: new/a.rs\n@@\n-x\n+y\n*** End Patch", "/w");
        let shown: Vec<(&str, &str)> = edits.iter().map(|edit| (edit.path.as_str(), edit.action)).collect();
        assert_eq!(shown, [("/w/old/a.rs", "delete"), ("/w/new/a.rs", "edit")]);
        assert_eq!(edits[1].new.as_deref(), Some("y"));
        let moved = edit_from_change("/w/b.rs", &serde_json::json!({"type": "update", "move_path": "/w/c.rs", "unified_diff": "@@\n-x\n+y"}));
        assert_eq!((moved[0].action, moved[1].path.as_str()), ("delete", "/w/c.rs"));
    }

    #[test]
    fn a_changed_file_is_read_again() {
        // The session cache holds one entry, so another test's load could hide a stale read. Alone in a child process.
        if std::env::var_os("EVERTUSK_ALONE").is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "commands::codex::tests::a_changed_file_is_read_again"])
                .env("EVERTUSK_ALONE", "1")
                .output()
                .unwrap();
            let report = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success() && report.contains("1 passed"), "{report}");
            return;
        }
        let prompt = |text: &str| item(&format!(r#"{{"type":"UserMessage","id":"{text}","content":[{{"type":"text","text":"{text}"}}]}}"#));
        let path = rollout("cache", &[META, &prompt("aaaa")]);
        let first_text = |messages: Vec<ConversationMessage>| messages.iter().find(|message| message.role == "user").map(|message| message.text.clone());
        assert_eq!(stats(&path).unwrap().user_prompt_count, 1);
        assert_eq!(cached_meta(&path).unwrap().prompts, 1);

        fs::write(&path, [META, &prompt("aaaa"), &prompt("more")].join("\n")).unwrap();
        let (grown_stats, grown_meta) = (stats(&path).unwrap().user_prompt_count, cached_meta(&path).unwrap().prompts);

        // The same size with a new time still counts as a change.
        let earlier = SystemTime::now() - std::time::Duration::from_secs(60);
        fs::write(&path, [META, &prompt("bbbb"), &prompt("more")].join("\n")).unwrap();
        fs::File::options().write(true).open(&path).unwrap().set_modified(earlier).unwrap();
        let rewritten = first_text(parse_messages(&path).unwrap());
        let rewritten_meta = cached_meta(&path).unwrap().first_prompt;
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert_eq!((grown_stats, grown_meta), (2, 2));
        assert_eq!(rewritten.as_deref(), Some("bbbb"));
        assert_eq!(rewritten_meta.as_deref(), Some("bbbb"));
    }
}
