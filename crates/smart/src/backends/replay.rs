//! Backend de reproducción: discos y lecturas definidos en un JSON. Sirve para
//! demostraciones y pruebas de punta a punta sin hardware. Cada disco puede tener
//! una secuencia de lecturas que se recorre en lecturas sucesivas (la última se repite).
//!
//! ```json
//! { "devices": [ { "info": { "path": "/dev/sdb", "bus": "sata", ... },
//!                  "sequence": [ { "kind": "read", "detail": { ...HealthSnapshot } } ] } ] }
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::{evaluate_ata, evaluate_nvme, DeviceInfo, ReadOutcome};

use super::HealthBackend;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayDevice {
    pub info: DeviceInfo,
    pub sequence: Vec<ReadOutcome>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplayFile {
    pub devices: Vec<ReplayDevice>,
}

type Cursor = Arc<Mutex<HashMap<String, usize>>>;

pub struct Replay {
    file: Result<ReplayFile, String>,
    cursor: Cursor,
}

/// Cursores por archivo compartidos en todo el proceso: cada pasada crea un backend
/// nuevo, pero la secuencia debe seguir avanzando.
fn shared_cursor(path: &str) -> Cursor {
    static CURSORS: OnceLock<Mutex<HashMap<String, Cursor>>> = OnceLock::new();
    CURSORS
        .get_or_init(Mutex::default)
        .lock()
        .expect("cursores de reproducción")
        .entry(path.to_string())
        .or_default()
        .clone()
}

impl Replay {
    pub fn new(file: ReplayFile) -> Self {
        Self { file: Ok(file), cursor: Cursor::default() }
    }

    pub fn from_file(path: &str) -> Self {
        let file = std::fs::read(path).map_err(|e| format!("no se pudo leer {path}: {e}")).and_then(|b| {
            serde_json::from_slice(&b).map_err(|e| format!("{path} no es un archivo de reproducción válido: {e}"))
        });
        Self { file, cursor: shared_cursor(path) }
    }
}

impl HealthBackend for Replay {
    fn name(&self) -> &'static str {
        "reproducción"
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, String> {
        Ok(self.file.as_ref().map_err(Clone::clone)?.devices.iter().map(|d| d.info.clone()).collect())
    }

    fn read(&self, dev: &DeviceInfo) -> ReadOutcome {
        let file = match &self.file {
            Ok(f) => f,
            Err(e) => return ReadOutcome::Failed(e.clone()),
        };
        let Some(d) = file.devices.iter().find(|d| d.info.path == dev.path) else {
            return ReadOutcome::Failed(format!("{} no está en el archivo de reproducción", dev.path));
        };
        let mut cursor = self.cursor.lock().expect("cursor de reproducción");
        let i = cursor.entry(dev.path.clone()).or_insert(0);
        let Some(outcome) = d.sequence.get((*i).min(d.sequence.len().saturating_sub(1))).cloned() else {
            return ReadOutcome::Unsupported("sin lecturas definidas".into());
        };
        *i += 1;
        // La evaluación se recalcula siempre: el archivo no puede contradecir las reglas.
        match outcome {
            ReadOutcome::Read(mut s) => {
                let (overall, reasons) = match &s.nvme {
                    Some(n) => evaluate_nvme(n),
                    None => evaluate_ata(&s.attributes),
                };
                s.overall = overall;
                s.reasons = reasons;
                s.source = "reproducción".into();
                ReadOutcome::Read(s)
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_advances_across_instances_of_the_same_file() {
        let dir = std::env::temp_dir().join(format!("bs-replay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("r.json");
        std::fs::write(
            &path,
            r#"{ "devices": [ { "info": { "path": "/dev/x", "model": null, "serial": null, "firmware": null,
            "bus": "other", "capacityBytes": 1, "removable": false, "rotational": null, "isBootDisk": false },
            "sequence": [ { "kind": "unsupported", "detail": "uno" }, { "kind": "failed", "detail": "dos" } ] } ] }"#,
        )
        .unwrap();
        let p = path.to_string_lossy().to_string();
        let a = Replay::from_file(&p);
        let dev = a.devices().unwrap().remove(0);
        assert_eq!(a.read(&dev), ReadOutcome::Unsupported("uno".into()));
        let b = Replay::from_file(&p);
        assert_eq!(b.read(&dev), ReadOutcome::Failed("dos".into()));
        assert_eq!(b.read(&dev), ReadOutcome::Failed("dos".into()), "la última se repite");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
