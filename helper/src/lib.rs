//! Formato del trabajo que la app entrega al helper y su hash.
//! Lo comparten el helper (que lo valida) y la app (que lo genera).

use base64::Engine;
use blocksmith_fs_core::{RebuildPlan, WritePhase};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const JOB_VERSION: u32 = 1;

/// Escritura decodificada: (offset relativo al volumen, fase, etiqueta, datos).
pub type DecodedWrite = (u64, WritePhase, String, Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobWrite {
    /// Offset **relativo al volumen**.
    pub offset: u64,
    pub phase: WritePhase,
    pub label: String,
    pub data_b64: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub version: u32,
    pub target: String,
    pub target_len: u64,
    pub base_offset: u64,
    pub volume_len: u64,
    pub label: String,
    pub plan_sha256: String,
    pub writes: Vec<JobWrite>,
}

/// Hash del plan: offsets, fases, etiquetas y datos de cada escritura, en orden.
pub fn plan_sha256<'a>(writes: impl IntoIterator<Item = (u64, WritePhase, &'a str, &'a [u8])>) -> String {
    let mut h = Sha256::new();
    for (offset, phase, label, data) in writes {
        h.update(offset.to_le_bytes());
        h.update([phase as u8]);
        h.update((label.len() as u64).to_le_bytes());
        h.update(label.as_bytes());
        h.update((data.len() as u64).to_le_bytes());
        h.update(data);
    }
    hex(&h.finalize())
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

impl Job {
    pub fn from_plan(plan: &RebuildPlan, target: &str, target_len: u64, base_offset: u64, volume_len: u64, label: &str) -> Self {
        let ordered: Vec<_> = plan.ordered_writes().collect();
        Self {
            version: JOB_VERSION,
            target: target.into(),
            target_len,
            base_offset,
            volume_len,
            label: label.into(),
            plan_sha256: plan_sha256(ordered.iter().map(|w| (w.offset, w.phase, w.label.as_str(), w.data.as_slice()))),
            writes: ordered
                .iter()
                .map(|w| JobWrite {
                    offset: w.offset,
                    phase: w.phase,
                    label: w.label.clone(),
                    data_b64: base64::engine::general_purpose::STANDARD.encode(&w.data),
                })
                .collect(),
        }
    }

    /// Decodifica las escrituras y comprueba que el hash coincide con el declarado.
    pub fn decode(&self) -> Result<Vec<DecodedWrite>, String> {
        if self.version != JOB_VERSION {
            return Err(format!("versión de trabajo {} no compatible", self.version));
        }
        let mut out = Vec::with_capacity(self.writes.len());
        for w in &self.writes {
            let data = base64::engine::general_purpose::STANDARD.decode(&w.data_b64).map_err(|e| format!("datos no válidos en «{}»: {e}", w.label))?;
            out.push((w.offset, w.phase, w.label.clone(), data));
        }
        let computed = plan_sha256(out.iter().map(|(o, p, l, d)| (*o, *p, l.as_str(), d.as_slice())));
        if computed != self.plan_sha256 {
            return Err("el hash del plan no coincide con sus escrituras".into());
        }
        Ok(out)
    }
}

/// Mensajes del helper a la app (una línea JSON por evento en la salida estándar).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HelperEvent {
    Phase { name: String },
    Progress { done: u32, total: u32 },
    Done { state: String, verified: bool, findings: Vec<String> },
    Cancelled { rolled_back: bool },
    Error { message: String, interrupted: bool },
}
