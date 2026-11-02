// Writes everything the app shows for each given transcript, plus its Markdown export,
// to an output folder. scripts/audit_transcripts.py compares these against the raw JSONL.
//
// Usage: cargo run --example dump_sessions -- <output_dir> <transcript.jsonl>...

// This bin pulls in the whole commands module but only exercises a subset of it.
#![allow(dead_code)]

#[path = "../src/commands.rs"]
mod commands;

use serde_json::json;
use std::path::Path;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let output_dir = arguments.next().expect("usage: dump_sessions <output_dir> <transcript.jsonl>...");
    let listed = commands::scan_projects().expect("list sessions");
    std::fs::write(format!("{output_dir}/_listed.json"), json!(listed).to_string()).expect("write listing");
    let projects = commands::get_projects().expect("list projects");
    std::fs::write(format!("{output_dir}/_projects.json"), json!(projects).to_string()).expect("write projects");
    for transcript in arguments {
        let session_id = Path::new(&transcript)
            .file_stem()
            .expect("transcript has a file name")
            .to_string_lossy()
            .to_string();
        let subagents = commands::list_subagents(transcript.clone()).expect("list subagents");
        let subagent_transcripts: Vec<_> = subagents
            .iter()
            .map(|agent| {
                json!({
                    "path": agent.jsonl_path,
                    "messages": commands::get_subagent_messages(agent.jsonl_path.clone()).expect("subagent messages"),
                })
            })
            .collect();
        let dump = json!({
            "messages": commands::get_session_messages(transcript.clone()).expect("messages"),
            "results": commands::get_tool_results(transcript.clone()).expect("results"),
            "questions": commands::get_session_questions(transcript.clone()).expect("questions"),
            "artifacts": commands::get_session_artifacts(transcript.clone()).expect("artifacts"),
            "stats": commands::get_session_stats(transcript.clone()).expect("stats"),
            "files": commands::get_session_file_changes(transcript.clone()).expect("files"),
            "subagents": subagents,
            "subagent_transcripts": subagent_transcripts,
        });
        std::fs::write(format!("{output_dir}/{session_id}.json"), dump.to_string()).expect("write dump");
        commands::export_session_markdown(
            transcript.clone(),
            format!("{output_dir}/{session_id}.md"),
            Some(session_id.clone()),
            None,
        )
        .expect("export");
    }
}
