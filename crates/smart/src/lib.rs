//! Salud de discos: atributos S.M.A.R.T. (ATA) y registro de salud NVMe.
//!
//! - Los intérpretes de datos (`ata`, `nvme`, `backends::smartctl`) son puros y se
//!   prueban con datos construidos según las especificaciones.
//! - Los backends hablan con el sistema: Linux nativo (sysfs + `SG_IO` + ioctl NVMe),
//!   `smartctl` como binario externo (macOS y respaldo) y reproducción desde JSON
//!   (demostraciones y pruebas).
//!
//! Regla de honestidad: si no se puede leer, el estado es `Unknown` con un motivo;
//! nunca `Passed`.

pub mod ata;
pub mod backends;
pub mod nvme;

use serde::{Deserialize, Serialize};

pub use backends::{auto_backend, HealthBackend};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bus {
    Sata,
    Nvme,
    Usb,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Overall {
    Passed,
    Unknown,
    Warning,
    Failing,
}

impl Overall {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Warning => "warning",
            Self::Failing => "failing",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Ata,
    Nvme,
}

/// Disco físico detectado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Ruta del dispositivo (`/dev/sda`, `/dev/nvme0n1`, `\\.\PhysicalDrive0`…).
    pub path: String,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub firmware: Option<String>,
    pub bus: Bus,
    pub capacity_bytes: u64,
    pub removable: bool,
    pub rotational: Option<bool>,
    pub is_boot_disk: bool,
}

impl DeviceInfo {
    /// Identificador estable para la base de datos: número de serie o, si no hay, la ruta.
    pub fn stable_key(&self) -> String {
        match &self.serial {
            Some(s) if !s.trim().is_empty() => format!("serial:{}", s.trim()),
            _ => format!("path:{}", self.path),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    pub id: u8,
    pub name: String,
    pub flags: u16,
    pub value: u8,
    pub worst: u8,
    pub threshold: u8,
    pub raw: u64,
}

impl Attribute {
    /// El valor normalizado ha caído al umbral del fabricante (fallo previsto).
    pub fn failing(&self) -> bool {
        self.threshold > 0 && self.value > 0 && self.value <= self.threshold
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NvmeHealth {
    pub critical_warning: u8,
    pub temperature_k: u16,
    pub available_spare: u8,
    pub spare_threshold: u8,
    pub percentage_used: u8,
    pub data_units_read: u64,
    pub data_units_written: u64,
    pub power_cycles: u64,
    pub power_on_hours: u64,
    pub unsafe_shutdowns: u64,
    pub media_errors: u64,
    pub error_log_entries: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSnapshot {
    pub protocol: Protocol,
    pub overall: Overall,
    pub temperature_c: Option<i16>,
    pub power_on_hours: Option<u64>,
    pub percentage_used: Option<u8>,
    pub attributes: Vec<Attribute>,
    pub nvme: Option<NvmeHealth>,
    /// Origen de la lectura: "nativo", "smartctl" o "reproducción".
    pub source: String,
    /// Motivos de la evaluación, legibles («5 sectores reasignados»…).
    pub reasons: Vec<String>,
}

/// Resultado de intentar leer un disco.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ReadOutcome {
    Read(HealthSnapshot),
    /// El dispositivo o el puente (p. ej. una caja USB) no transmite S.M.A.R.T.
    Unsupported(String),
    /// Hacen falta permisos de administrador.
    PermissionDenied(String),
    Failed(String),
}

/// Atributos ATA críticos: con raw > 0 indican deterioro aunque no crucen el umbral.
pub const CRITICAL_ATA: &[(u8, &str)] = &[
    (5, "sectores reasignados"),
    (187, "errores no corregibles notificados"),
    (197, "sectores pendientes de reasignación"),
    (198, "sectores no corregibles"),
];

/// Evaluación de un disco ATA a partir de sus atributos.
pub fn evaluate_ata(attrs: &[Attribute]) -> (Overall, Vec<String>) {
    if attrs.is_empty() {
        return (Overall::Unknown, vec!["el disco no devolvió atributos".into()]);
    }
    let mut reasons = Vec::new();
    for a in attrs.iter().filter(|a| a.failing()) {
        reasons.push(format!("{} por debajo del umbral ({} ≤ {})", a.name, a.value, a.threshold));
    }
    if !reasons.is_empty() {
        return (Overall::Failing, reasons);
    }
    for (id, what) in CRITICAL_ATA {
        if let Some(a) = attrs.iter().find(|a| a.id == *id && ata::counter(a) > 0) {
            reasons.push(format!("{} {what}", ata::counter(a)));
        }
    }
    if reasons.is_empty() { (Overall::Passed, reasons) } else { (Overall::Warning, reasons) }
}

/// Evaluación de un disco NVMe a partir de su registro de salud.
pub fn evaluate_nvme(n: &NvmeHealth) -> (Overall, Vec<String>) {
    let mut reasons: Vec<String> = nvme::warning_reasons(n.critical_warning);
    if n.available_spare < n.spare_threshold && n.spare_threshold > 0 {
        reasons.push(format!("bloques de reserva por debajo del umbral ({} % < {} %)", n.available_spare, n.spare_threshold));
    }
    if !reasons.is_empty() {
        return (Overall::Failing, reasons);
    }
    if n.percentage_used >= 100 {
        reasons.push(format!("vida útil estimada consumida ({} %)", n.percentage_used));
    }
    if n.media_errors > 0 {
        reasons.push(format!("{} errores de integridad de datos", n.media_errors));
    }
    if reasons.is_empty() { (Overall::Passed, reasons) } else { (Overall::Warning, reasons) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: u8, value: u8, threshold: u8, raw: u64) -> Attribute {
        Attribute { id, name: ata::attribute_name(id).into(), flags: 0, value, worst: value, threshold, raw }
    }

    #[test]
    fn ata_evaluation_rules() {
        assert_eq!(evaluate_ata(&[]).0, Overall::Unknown);
        assert_eq!(evaluate_ata(&[a(9, 100, 0, 1234)]).0, Overall::Passed);
        let (o, r) = evaluate_ata(&[a(5, 100, 10, 8)]);
        assert_eq!(o, Overall::Warning);
        assert_eq!(r, vec!["8 sectores reasignados"]);
        assert_eq!(evaluate_ata(&[a(5, 9, 10, 900)]).0, Overall::Failing);
        // Un valor 0 con umbral significa «no disponible», no fallo.
        assert_eq!(evaluate_ata(&[a(1, 0, 6, 0)]).0, Overall::Passed);
    }

    #[test]
    fn nvme_evaluation_rules() {
        let ok = NvmeHealth { available_spare: 100, spare_threshold: 10, ..Default::default() };
        assert_eq!(evaluate_nvme(&ok).0, Overall::Passed);
        assert_eq!(evaluate_nvme(&NvmeHealth { media_errors: 3, ..ok.clone() }).0, Overall::Warning);
        assert_eq!(evaluate_nvme(&NvmeHealth { percentage_used: 104, ..ok.clone() }).0, Overall::Warning);
        let (o, r) = evaluate_nvme(&NvmeHealth { critical_warning: 0b0000_0100, ..ok.clone() });
        assert_eq!(o, Overall::Failing);
        assert!(r[0].contains("fiabilidad"));
        assert_eq!(evaluate_nvme(&NvmeHealth { available_spare: 5, ..ok }).0, Overall::Failing);
    }

    #[test]
    fn stable_key_prefers_serial() {
        let mut d = DeviceInfo {
            path: "/dev/sda".into(), model: None, serial: Some(" WX42 ".into()), firmware: None, bus: Bus::Sata,
            capacity_bytes: 0, removable: false, rotational: None, is_boot_disk: false,
        };
        assert_eq!(d.stable_key(), "serial:WX42");
        d.serial = None;
        assert_eq!(d.stable_key(), "path:/dev/sda");
    }
}
