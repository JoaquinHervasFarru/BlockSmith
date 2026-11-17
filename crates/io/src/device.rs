//! Dispositivos físicos en solo lectura (F1: enumeración; apertura por plataforma).

use serde::{Deserialize, Serialize};

/// Descripción de un dispositivo físico detectado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub path: String,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub size_bytes: u64,
    pub removable: bool,
    pub is_boot_disk: bool,
}
