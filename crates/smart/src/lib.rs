//! Lectura de salud de disco: atributos S.M.A.R.T. (ATA) y Health log (NVMe).
//! Si un puente USB no transmite los datos, el estado es `Unknown`, nunca `Passed`.

pub mod backends;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bus {
    Sata,
    Nvme,
    Usb,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Overall {
    Passed,
    Warning,
    Failing,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribute {
    pub id: u8,
    pub name: String,
    pub value: u8,
    pub worst: u8,
    pub threshold: u8,
    pub raw: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub overall: Overall,
    pub temperature_c: Option<i16>,
    pub power_on_hours: Option<u64>,
    pub percentage_used: Option<u8>,
    pub attributes: Vec<Attribute>,
}

/// Evaluación mínima: un atributo con umbral > 0 y valor <= umbral es fallo.
pub fn evaluate(attrs: &[Attribute]) -> Overall {
    if attrs.is_empty() {
        return Overall::Unknown;
    }
    if attrs.iter().any(|a| a.threshold > 0 && a.value <= a.threshold) {
        return Overall::Failing;
    }
    // 5: reasignados, 197: pendientes, 198: incorregibles.
    if attrs.iter().any(|a| matches!(a.id, 5 | 197 | 198) && a.raw > 0) {
        return Overall::Warning;
    }
    Overall::Passed
}

pub trait Backend {
    fn read(&self, device_path: &str) -> Result<Option<HealthSnapshot>, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: u8, value: u8, threshold: u8, raw: u64) -> Attribute {
        Attribute { id, name: String::new(), value, worst: value, threshold, raw }
    }

    #[test]
    fn evaluation_rules() {
        assert_eq!(evaluate(&[]), Overall::Unknown);
        assert_eq!(evaluate(&[a(9, 100, 0, 1234)]), Overall::Passed);
        assert_eq!(evaluate(&[a(5, 100, 10, 8)]), Overall::Warning);
        assert_eq!(evaluate(&[a(5, 9, 10, 900)]), Overall::Failing);
    }
}
