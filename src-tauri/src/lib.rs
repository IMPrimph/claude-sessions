mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_projects,
            commands::scan_projects,
            commands::get_session_messages,
            commands::get_project_tokens,
            commands::get_session_stats,
            commands::export_session_markdown,
            commands::list_subagents,
            commands::get_subagent_messages,
            commands::get_tool_results,
            commands::get_session_questions,
            commands::get_session_artifacts,
            commands::archive_session,
            commands::get_archived_session_path,
            commands::get_archived_session_ids,
            commands::unarchive_session,
            commands::get_archive_info,
            commands::set_archive_location,
            commands::reset_archive_location,
            commands::open_archive_location,
            commands::read_tool_output_file,
            commands::get_session_file_changes,
            commands::global_search,
            commands::get_image_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
