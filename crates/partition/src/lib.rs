//! Detección de esquemas de partición: GPT, Apple Partition Map (APM) y MBR.
//!
//! `detect` nunca falla por datos corruptos: devuelve lo que pudo leer y una lista
//! de avisos (p. ej. CRC de GPT incorrecto con la cabecera de respaldo usada).

pub mod apfs_container;
pub mod apm;
pub mod gpt;
pub mod mbr;

use blocksmith_io::{BlockDevice, IoResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Gpt,
    Apm,
    Mbr,
    /// Sin tabla: el volumen ocupa todo el dispositivo.
    None,
}

/// Contenido probable de la partición según su tipo declarado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hint {
    HfsPlus,
    Apfs,
    Ntfs,
    LinuxFs,
    Luks,
    EfiSystem,
    Bitlocker,
    Reserved,
    Free,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Partition {
    pub index: u32,
    pub start: u64,
    pub len: u64,
    pub type_id: String,
    pub name: String,
    pub hint: Hint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionTable {
    pub scheme: Scheme,
    pub partitions: Vec<Partition>,
    pub warnings: Vec<String>,
}

/// Detecta el esquema de particiones. Orden: GPT (incluye MBR protector) → APM → MBR.
pub fn detect(dev: &dyn BlockDevice) -> IoResult<PartitionTable> {
    if let Some(t) = gpt::read(dev)? {
        return Ok(t);
    }
    if let Some(t) = apm::read(dev)? {
        return Ok(t);
    }
    if let Some(t) = mbr::read(dev)? {
        return Ok(t);
    }
    Ok(PartitionTable { scheme: Scheme::None, partitions: vec![], warnings: vec![] })
}
