//! Núcleo de la app: comandos IPC, orquestador de trabajos y repositorio SQLite.

pub mod commands;
pub mod core;
pub mod db;
mod jobs;

use std::sync::Arc;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db = db::Database::open(&data_dir.join("blocksmith.sqlite3"))?;
            let output_dir = app
                .path()
                .document_dir()
                .or_else(|_| app.path().home_dir())
                .unwrap_or_else(|_| data_dir.clone())
                .join("BlockSmith");
            app.manage(Arc::new(core::Core::new(db, data_dir, output_dir)));
            app.manage(jobs::JobManager::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::db_status,
            commands::open_image,
            commands::list_sources,
            commands::remove_source,
            commands::get_volume,
            commands::list_sessions,
            commands::get_session,
            commands::get_findings,
            commands::get_rebuild,
            commands::get_diff,
            commands::tree_children,
            commands::start_scan,
            commands::start_plan,
            commands::start_export,
            commands::cancel_job,
            commands::suggest_export_path,
            commands::write_report,
            commands::list_reports,
            commands::get_settings,
            commands::set_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar BlockSmith");
}
