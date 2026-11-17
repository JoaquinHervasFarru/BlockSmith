//! Base de datos SQLite de BlockSmith, compartida por la app de escritorio y el
//! agente de salud. Las migraciones se aplican en orden según `user_version` y son
//! solo de añadido: nunca se edita una migración ya publicada.
//!
//! Nunca se guardan claves ni secretos de cifrado.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

pub use rusqlite;
use rusqlite::Connection;
use serde::Serialize;

pub const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_health.sql"),
];

/// Identificador de la app (igual que en tauri.conf.json): define la carpeta de datos.
pub const APP_IDENTIFIER: &str = "dev.blocksmith.desktop";
pub const DB_FILE: &str = "blocksmith.sqlite3";

/// Ruta por defecto de la base de datos (la misma que usa la app de escritorio).
pub fn default_db_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_IDENTIFIER).join(DB_FILE))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbStatus {
    pub schema_version: i64,
    pub tables: Vec<String>,
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        // App y agente pueden escribir a la vez: se espera en lugar de fallar.
        conn.busy_timeout(Duration::from_secs(10))?;
        Self::init(conn)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Conexión con bloqueo exclusivo. Mantenerla el menor tiempo posible y no
    /// llamar a otra función que la pida mientras se tiene (el Mutex no es reentrante).
    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("db mutex envenenado")
    }

    pub fn status(&self) -> rusqlite::Result<DbStatus> {
        let conn = self.conn();
        let schema_version = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let tables = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(DbStatus { schema_version, tables })
    }

    /// Valor de ajustes (JSON) por clave.
    pub fn setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        use rusqlite::OptionalExtension;
        self.conn().query_row("SELECT value_json FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()
    }

    pub fn set_setting(&self, key: &str, value_json: &str) -> rusqlite::Result<()> {
        self.conn().execute("INSERT OR REPLACE INTO settings (key, value_json) VALUES (?1, ?2)", [key, value_json])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_create_full_schema() {
        let db = Database::in_memory().unwrap();
        let s = db.status().unwrap();
        assert_eq!(s.schema_version, MIGRATIONS.len() as i64);
        for t in [
            "alert_rules", "alerts", "boot_media", "diff_entries", "disks", "encryption_layers",
            "findings", "rebuilds", "reports", "sessions", "settings", "smart_attributes",
            "smart_snapshots", "sources", "undo_blocks", "volumes", "write_transactions",
        ] {
            assert!(s.tables.iter().any(|x| x == t), "falta la tabla {t}");
        }
        let rules: i64 = db.conn().query_row("SELECT COUNT(*) FROM alert_rules", [], |r| r.get(0)).unwrap();
        assert_eq!(rules, 6);
    }

    #[test]
    fn upgrades_from_v1_on_disk() {
        let dir = std::env::temp_dir().join(format!("bs-store-{}", std::process::id()));
        let path = dir.join("db.sqlite3");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        {
            let mut c = Connection::open(&path).unwrap();
            let tx = c.transaction().unwrap();
            tx.execute_batch(MIGRATIONS[0]).unwrap();
            tx.pragma_update(None, "user_version", 1).unwrap();
            tx.commit().unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.status().unwrap().schema_version, 2);
        db.set_setting("x", "1").unwrap();
        assert_eq!(db.setting("x").unwrap().as_deref(), Some("1"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
