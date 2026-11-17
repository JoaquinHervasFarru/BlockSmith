//! Núcleo de la app: comandos IPC, orquestador de trabajos y repositorio SQLite.

pub mod commands;
pub mod core;
pub mod db;
mod jobs;

use std::sync::Arc;

use tauri::{Emitter, Manager};

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
            let core = Arc::new(core::Core::new(db, data_dir, output_dir));
            // Transacciones que quedaron a medias si la app se cerró escribiendo.
            if let Err(e) = core.detect_interrupted() {
                eprintln!("no se pudieron revisar las transacciones pendientes: {e}");
            }
            app.manage(core.clone());
            app.manage(jobs::JobManager::default());

            // Lectura de salud al abrir, en segundo plano, si la última es antigua.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                if !core.health_due().unwrap_or(false) {
                    return;
                }
                if let Ok(report) = core.health_collect(&mut core::NullProgress) {
                    commands::notify_alerts(&handle, &report.new_alerts);
                    let _ = handle.emit("health-updated", &report);
                }
            });
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
            commands::health_disks,
            commands::health_disk,
            commands::health_status,
            commands::start_health_read,
            commands::list_alerts,
            commands::ack_alert,
            commands::get_alert_rules,
            commands::set_alert_rule,
            commands::preflight,
            commands::start_repair,
            commands::list_transactions,
            commands::start_tx_rollback,
            commands::start_tx_complete,
            commands::list_open_devices,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar BlockSmith");
}
