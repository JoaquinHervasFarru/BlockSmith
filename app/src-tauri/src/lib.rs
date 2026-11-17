//! Núcleo de la app: comandos IPC, orquestador de trabajos y repositorio SQLite.

mod commands;
mod db;
mod jobs;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = db::Database::open(&dir.join("dirforge.sqlite3"))?;
            app.manage(db);
            app.manage(jobs::JobQueue::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::db_status,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar DirForge");
}
