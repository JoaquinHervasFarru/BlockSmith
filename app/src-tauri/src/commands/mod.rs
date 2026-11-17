//! Comandos expuestos a la UI. En F0 solo hay comandos de diagnóstico; la UI
//! usa datos simulados (`ui/src/mocks`) mientras se implementa el motor.

use serde::Serialize;
use tauri::State;

use crate::db::Database;

#[derive(Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub platform: &'static str,
    pub phase: &'static str,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        phase: "F0",
    }
}

#[derive(Serialize)]
pub struct DbStatus {
    pub schema_version: i64,
    pub tables: Vec<String>,
}

#[tauri::command]
pub fn db_status(db: State<'_, Database>) -> Result<DbStatus, String> {
    db.status().map_err(|e| e.to_string())
}
