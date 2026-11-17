//! Salud de discos sobre la base de datos compartida.
//!
//! - [`rules`]: reglas de alerta configurables, evaluadas sobre dos lecturas sucesivas.
//! - [`repo`]: discos, lecturas, atributos, alertas, reglas y estado del agente.
//! - [`collector`]: una pasada completa (enumerar → leer → guardar → alertar).

pub mod collector;
pub mod repo;
pub mod rules;

pub use collector::{collect, CollectReport};
pub use repo::{AlertRow, DiskRow, StoredSnapshot};
pub use rules::{AlertCandidate, Rule};

pub type Result<T> = std::result::Result<T, rusqlite::Error>;

pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

use blocksmith_store::rusqlite;
