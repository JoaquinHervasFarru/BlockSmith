//! Contratos comunes a todos los sistemas de archivos de BlockSmith.
//!
//! Cada formato (HFS+, APFS, NTFS, ext4) implementa [`FileSystem`] y declara
//! sus [`Capabilities`]; la UI habilita acciones según ellas.

mod finding;
mod plan;

use std::path::Path;

use blocksmith_io::BlockDevice;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use finding::{Finding, ObjectType, Severity};
pub use plan::{BlockWrite, DiffChange, DiffEntry, RebuildPlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FsKind {
    HfsPlus,
    Hfsx,
    Apfs,
    Ntfs,
    Ext4,
}

/// Nivel de soporte de una acción.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    No,
    Yes,
    Experimental,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Capabilities {
    pub scan: Support,
    pub recover: Support,
    pub rebuild: Support,
    pub write_in_place: Support,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsInfo {
    pub kind: FsKind,
    pub name: Option<String>,
    pub block_size: u32,
    pub total_blocks: u64,
    pub free_blocks: u64,
    pub journaled: bool,
    pub case_sensitive: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ScanStats {
    pub nodes_total: u64,
    pub nodes_damaged: u64,
    pub files: u64,
    pub folders: u64,
    pub fragmentation_pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub info: FsInfo,
    pub stats: ScanStats,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RecoverResult {
    pub files_copied: u64,
    pub bytes_copied: u64,
    pub failed: Vec<String>,
}

/// Progreso y cancelación compartidos por todas las operaciones largas.
pub trait Progress: Send {
    fn phase(&mut self, name: &str);
    fn advance(&mut self, done: u64, total: u64);
    fn is_cancelled(&self) -> bool;
}

pub struct ScanContext<'a> {
    pub device: &'a dyn BlockDevice,
    pub progress: &'a mut dyn Progress,
}

#[derive(Debug, Error)]
pub enum FsError {
    #[error(transparent)]
    Io(#[from] blocksmith_io::IoError),
    #[error("estructura no válida: {0}")]
    Invalid(String),
    #[error("operación cancelada")]
    Cancelled,
    #[error("no soportado por este sistema de archivos: {0}")]
    Unsupported(&'static str),
}

pub type FsResult<T> = Result<T, FsError>;

/// Contrato que implementa cada sistema de archivos.
pub trait FileSystem: Send + Sync {
    fn kind(&self) -> FsKind;
    fn probe(&self, dev: &dyn BlockDevice) -> FsResult<Option<FsInfo>>;
    fn capabilities(&self) -> Capabilities;
    fn scan(&self, ctx: &mut ScanContext<'_>) -> FsResult<ScanResult>;
    fn recover(&self, ctx: &mut ScanContext<'_>, dest: &Path) -> FsResult<RecoverResult>;
    /// Calcula los cambios **sin escribir nada**.
    fn plan_rebuild(&self, ctx: &mut ScanContext<'_>) -> FsResult<RebuildPlan>;
}
