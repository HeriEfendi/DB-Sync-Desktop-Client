mod commands;

pub use commands::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .invoke_handler(tauri::generate_handler![
            commands::test_local_connection,
            commands::get_last_local_id,
            commands::get_local_max_updated_at,
            commands::sync_to_local_db,
            commands::get_local_table_preview,
            commands::truncate_local_table
        ])
        .run(tauri::generate_context!())
        .expect("Error while running Tauri application");
}
