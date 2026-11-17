//! Repositorio SQLite. Las migraciones se aplican en orden según `user_version`.
//! Nunca se guardan claves ni secretos de cifrado.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

use crate::commands::DbStatus;

const MIGRATIONS: &[&str] = &[include_str!("../../migrations/0001_init.sql")];

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
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

    /// Conexión con bloqueo exclusivo. Mantenerla el menor tiempo posible.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_create_full_schema() {
        let db = Database::in_memory().unwrap();
        let s = db.status().unwrap();
        assert_eq!(s.schema_version, 1);
        for t in [
            "alert_rules", "alerts", "boot_media", "diff_entries", "disks", "encryption_layers",
            "findings", "rebuilds", "reports", "sessions", "settings", "smart_attributes",
            "smart_snapshots", "sources", "undo_blocks", "volumes", "write_transactions",
        ] {
            assert!(s.tables.iter().any(|x| x == t), "falta la tabla {t}");
        }
    }
}
