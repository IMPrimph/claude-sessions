// Batch-exports every Claude Code session across all projects to Markdown,
// reusing the exact same parsing + rendering the GUI uses (scan_projects +
// export_session_markdown from commands.rs) so the output is identical to the
// app's per-session "Export as Markdown" button.
//
// Usage:
//   cargo run --bin export_all -- [output_dir]
// Default output_dir: ~/Documents/claude-sessions-export
//
// Output layout:
//   <output_dir>/<project-name>/<YYYY-MM-DD>_<title-slug>_<sessionid8>.md

// This bin pulls in the whole commands module but only exercises a subset of it,
// so most of the app's structs/commands are legitimately unused here.
#![allow(dead_code)]

#[path = "../commands.rs"]
mod commands;

use commands::SessionInfo;
use std::fs;
use std::path::{Path, PathBuf};

/// Mirror the frontend's display-title precedence (SessionList.svelte):
/// custom_title → summary (without `**`) → ai_title → first_prompt → session id.
fn pick_title(session: &SessionInfo) -> String {
    if let Some(custom_title) = session.custom_title.as_ref().filter(|value| !value.trim().is_empty()) {
        return custom_title.trim().to_string();
    }
    if let Some(summary) = session.summary.as_ref().filter(|value| !value.trim().is_empty()) {
        return summary.replace("**", "").trim().to_string();
    }
    if let Some(ai_title) = session.ai_title.as_ref().filter(|value| !value.trim().is_empty()) {
        return ai_title.trim().to_string();
    }
    if let Some(first_prompt) = session.first_prompt.as_ref().filter(|value| !value.trim().is_empty()) {
        let collapsed: String = first_prompt.split_whitespace().collect::<Vec<_>>().join(" ");
        let truncated: String = collapsed.chars().take(80).collect();
        if !truncated.trim().is_empty() {
            return truncated.trim().to_string();
        }
    }
    session.session_id.clone()
}

/// Lowercase, keep alphanumerics, collapse everything else to single hyphens,
/// trim leading/trailing hyphens, cap length.
fn slugify(input: &str, max_len: usize) -> String {
    let mut slug = String::with_capacity(input.len());
    let mut last_was_hyphen = false;
    for character in input.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            slug.push('-');
            last_was_hyphen = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    let capped: String = trimmed.chars().take(max_len).collect();
    let capped = capped.trim_matches('-').to_string();
    if capped.is_empty() {
        "untitled".to_string()
    } else {
        capped
    }
}

/// First 10 chars of an ISO timestamp = YYYY-MM-DD; falls back to "nodate".
fn date_prefix(session: &SessionInfo) -> String {
    let source = session
        .created
        .as_ref()
        .or(session.modified.as_ref())
        .map(|value| value.as_str())
        .unwrap_or("");
    if source.len() >= 10 {
        source[..10].to_string()
    } else {
        "nodate".to_string()
    }
}

fn short_id(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

fn main() {
    let output_root: PathBuf = match std::env::args().nth(1) {
        Some(path) => PathBuf::from(path),
        None => {
            let home = dirs::home_dir().expect("cannot resolve home directory");
            home.join("Documents").join("claude-sessions-export")
        }
    };

    let sessions = match commands::scan_projects(None) {
        Ok(sessions) => sessions,
        Err(scan_error) => {
            eprintln!("Failed to scan projects: {}", scan_error);
            std::process::exit(1);
        }
    };

    println!(
        "Found {} session(s). Exporting to {}",
        sessions.len(),
        output_root.display()
    );

    let mut exported = 0usize;
    let mut failed = 0usize;
    let mut used_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    for session in &sessions {
        let title = pick_title(session);
        let project_folder = slugify(&session.project_name, 80);
        let project_dir = output_root.join(&project_folder);
        if let Err(create_error) = fs::create_dir_all(&project_dir) {
            eprintln!(
                "  ! cannot create dir {}: {}",
                project_dir.display(),
                create_error
            );
            failed += 1;
            continue;
        }

        let base_name = format!(
            "{}_{}_{}",
            date_prefix(session),
            slugify(&title, 60),
            short_id(&session.session_id)
        );

        // Guard against the rare case of two sessions producing the same name.
        let mut save_path = project_dir.join(format!("{}.md", base_name));
        let mut disambiguator = 1;
        while used_paths.contains(&save_path.to_string_lossy().to_string()) {
            save_path = project_dir.join(format!("{}-{}.md", base_name, disambiguator));
            disambiguator += 1;
        }
        used_paths.insert(save_path.to_string_lossy().to_string());

        match commands::export_session_markdown(
            session.jsonl_path.clone(),
            save_path.to_string_lossy().to_string(),
            Some(title.clone()),
        ) {
            Ok(()) => {
                exported += 1;
                print_progress(&save_path, output_root.as_path());
            }
            Err(export_error) => {
                eprintln!(
                    "  ! failed {}: {}",
                    session.jsonl_path, export_error
                );
                failed += 1;
            }
        }
    }

    println!(
        "\nDone. Exported {} session(s), {} failure(s), into {}",
        exported,
        failed,
        output_root.display()
    );
}

fn print_progress(save_path: &Path, output_root: &Path) {
    let display = save_path
        .strip_prefix(output_root)
        .unwrap_or(save_path)
        .display();
    println!("  ✓ {}", display);
}
