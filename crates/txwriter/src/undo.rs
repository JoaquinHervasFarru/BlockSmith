//! Registro de deshacer: formato en disco, inspección, reversión y finalización.
//!
//! ```text
//! 0      16   magic "BLOCKSMITH-UNDO\0"
//! 16     4    versión (1)
//! 20     4    estado (TxState)
//! 24     4    escrituras completadas
//! 28     4    escrituras totales
//! 32     4    longitud del JSON de metadatos
//! 36     32   SHA-256 del JSON
//! 68     …    JSON (TxMeta), hasta el byte 4096
//! 4096   …    registros: magic "RCOR", seq, offset, len, fase, sha original,
//!             sha nuevo, bytes originales, bytes nuevos
//! ```

use std::fs::{File, OpenOptions};
use std::path::Path;

use blocksmith_fs_core::WritePhase;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::target::{pread, pwrite, Target};
use crate::{TxError, TxResult};

pub(crate) const MAGIC: &[u8; 16] = b"BLOCKSMITH-UNDO\0";
pub(crate) const VERSION: u32 = 1;
pub(crate) const HEADER: u64 = 4096;
pub(crate) const REC_MAGIC: &[u8; 4] = b"RCOR";
pub(crate) const REC_HEADER: u64 = 88;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum TxState {
    /// Registro completo; el destino aún no se ha tocado.
    Prepared = 1,
    /// Escribiendo estructuras nuevas (la versión vieja sigue siendo la vigente).
    Writing = 2,
    /// Cabecera del volumen actualizada: la versión nueva es la vigente.
    Committed = 3,
    /// Todas las escrituras aplicadas.
    Finalized = 4,
    /// El volumen resultante se ha verificado.
    Verified = 5,
    /// Se restauraron los bytes originales.
    RolledBack = 6,
}

impl TxState {
    fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            1 => Self::Prepared,
            2 => Self::Writing,
            3 => Self::Committed,
            4 => Self::Finalized,
            5 => Self::Verified,
            6 => Self::RolledBack,
            _ => return None,
        })
    }

    /// Interrumpida: empezó a escribir y no terminó ni se revirtió.
    pub fn is_interrupted(self) -> bool {
        matches!(self, Self::Prepared | Self::Writing | Self::Committed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxMeta {
    /// Destino tal como se abrió (ruta de la imagen o del dispositivo).
    pub target: String,
    pub target_len: u64,
    /// Offset del volumen dentro del destino.
    pub base_offset: u64,
    pub volume_len: u64,
    pub created_at: String,
    /// Hash del plan aplicado (para comprobar que se aprueba lo que se escribe).
    pub plan_sha256: String,
    pub label: String,
    /// Índice del primer registro de la fase de commit.
    pub first_commit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoRecordInfo {
    pub seq: u32,
    pub offset: u64,
    pub len: u32,
    pub phase: WritePhase,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoInfo {
    pub meta: TxMeta,
    pub state: TxState,
    pub written: u32,
    pub total: u32,
    pub records: Vec<UndoRecordInfo>,
    /// Bytes originales guardados (tamaño de lo que se puede restaurar).
    pub bytes_saved: u64,
}

pub(crate) fn phase_code(p: WritePhase) -> u8 {
    match p {
        WritePhase::Data => 0,
        WritePhase::Commit => 1,
        WritePhase::Finalize => 2,
    }
}

fn phase_from(c: u8) -> Option<WritePhase> {
    Some(match c {
        0 => WritePhase::Data,
        1 => WritePhase::Commit,
        2 => WritePhase::Finalize,
        _ => return None,
    })
}

pub(crate) fn sha(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// Posición de un registro dentro del archivo.
#[derive(Debug, Clone)]
pub(crate) struct RecordPos {
    pub seq: u32,
    pub log_offset: u64,
    pub offset: u64,
    pub len: u32,
    pub phase: WritePhase,
    pub orig_sha: [u8; 32],
    pub new_sha: [u8; 32],
}

impl RecordPos {
    pub fn read_orig(&self, f: &File) -> std::io::Result<Vec<u8>> {
        let mut b = vec![0u8; self.len as usize];
        pread(f, self.log_offset + REC_HEADER, &mut b)?;
        Ok(b)
    }
    pub fn read_new(&self, f: &File) -> std::io::Result<Vec<u8>> {
        let mut b = vec![0u8; self.len as usize];
        pread(f, self.log_offset + REC_HEADER + self.len as u64, &mut b)?;
        Ok(b)
    }
}

pub(crate) fn encode_record(seq: u32, offset: u64, phase: WritePhase, orig: &[u8], new: &[u8]) -> Vec<u8> {
    let mut r = Vec::with_capacity(REC_HEADER as usize + orig.len() * 2);
    r.extend_from_slice(REC_MAGIC);
    r.extend_from_slice(&seq.to_le_bytes());
    r.extend_from_slice(&offset.to_le_bytes());
    r.extend_from_slice(&(orig.len() as u32).to_le_bytes());
    r.extend_from_slice(&[phase_code(phase), 0, 0, 0]);
    r.extend_from_slice(&sha(orig));
    r.extend_from_slice(&sha(new));
    r.extend_from_slice(orig);
    r.extend_from_slice(new);
    r
}

pub(crate) fn encode_header(state: TxState, written: u32, total: u32, meta: &TxMeta) -> TxResult<Vec<u8>> {
    let json = serde_json::to_vec(meta).map_err(|e| TxError::BadUndo(e.to_string()))?;
    if 68 + json.len() > HEADER as usize {
        return Err(TxError::BadUndo("metadatos demasiado largos".into()));
    }
    let mut h = vec![0u8; HEADER as usize];
    h[0..16].copy_from_slice(MAGIC);
    h[16..20].copy_from_slice(&VERSION.to_le_bytes());
    h[20..24].copy_from_slice(&(state as u32).to_le_bytes());
    h[24..28].copy_from_slice(&written.to_le_bytes());
    h[28..32].copy_from_slice(&total.to_le_bytes());
    h[32..36].copy_from_slice(&(json.len() as u32).to_le_bytes());
    h[36..68].copy_from_slice(&sha(&json));
    h[68..68 + json.len()].copy_from_slice(&json);
    Ok(h)
}

/// Registro abierto.
pub(crate) struct UndoFile {
    pub file: File,
    pub meta: TxMeta,
    pub state: TxState,
    pub written: u32,
    pub total: u32,
    pub records: Vec<RecordPos>,
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().expect("4 bytes"))
}

impl UndoFile {
    pub fn open(path: &Path, write: bool) -> TxResult<Self> {
        let file = OpenOptions::new().read(true).write(write).open(path)?;
        let mut h = vec![0u8; HEADER as usize];
        pread(&file, 0, &mut h).map_err(|_| TxError::BadUndo("cabecera truncada".into()))?;
        if &h[0..16] != MAGIC {
            return Err(TxError::BadUndo("no es un registro de deshacer de BlockSmith".into()));
        }
        if u32_at(&h, 16) != VERSION {
            return Err(TxError::BadUndo(format!("versión {} no compatible", u32_at(&h, 16))));
        }
        let state = TxState::from_u32(u32_at(&h, 20))
            .ok_or_else(|| TxError::BadUndo("estado desconocido (preparación incompleta)".into()))?;
        let json_len = u32_at(&h, 32) as usize;
        let json = h.get(68..68 + json_len).ok_or_else(|| TxError::BadUndo("metadatos truncados".into()))?;
        if sha(json) != h[36..68] {
            return Err(TxError::BadUndo("metadatos dañados".into()));
        }
        let meta: TxMeta = serde_json::from_slice(json).map_err(|e| TxError::BadUndo(e.to_string()))?;
        let total = u32_at(&h, 28);
        let mut records = Vec::with_capacity(total as usize);
        let mut pos = HEADER;
        for seq in 0..total {
            let mut rh = [0u8; REC_HEADER as usize];
            pread(&file, pos, &mut rh).map_err(|_| TxError::BadUndo(format!("registro {seq} truncado")))?;
            if &rh[0..4] != REC_MAGIC || u32_at(&rh, 4) != seq {
                return Err(TxError::BadUndo(format!("registro {seq} dañado")));
            }
            let len = u32_at(&rh, 16);
            let phase =
                phase_from(rh[20]).ok_or_else(|| TxError::BadUndo(format!("fase desconocida en el registro {seq}")))?;
            records.push(RecordPos {
                seq,
                log_offset: pos,
                offset: u64::from_le_bytes(rh[8..16].try_into().expect("8")),
                len,
                phase,
                orig_sha: rh[24..56].try_into().expect("32"),
                new_sha: rh[56..88].try_into().expect("32"),
            });
            pos += REC_HEADER + 2 * len as u64;
        }
        Ok(Self { file, meta, state, written: u32_at(&h, 24).min(total), total, records })
    }

    /// Comprueba que los bytes guardados no se han corrompido.
    pub fn check_integrity(&self) -> TxResult<()> {
        for r in &self.records {
            if sha(&r.read_orig(&self.file)?) != r.orig_sha || sha(&r.read_new(&self.file)?) != r.new_sha {
                return Err(TxError::BadUndo(format!("los datos del registro {} están dañados", r.seq)));
            }
        }
        Ok(())
    }

    pub fn set_state(&mut self, state: TxState, written: u32) -> TxResult<()> {
        let mut b = [0u8; 8];
        b[0..4].copy_from_slice(&(state as u32).to_le_bytes());
        b[4..8].copy_from_slice(&written.to_le_bytes());
        pwrite(&self.file, 20, &b)?;
        self.file.sync_data()?;
        self.state = state;
        self.written = written;
        Ok(())
    }

    fn check_target(&self, target: &dyn Target) -> TxResult<()> {
        if target.len() != self.meta.target_len {
            return Err(TxError::WrongTarget(format!(
                "tamaño {} y se esperaba {} ({})",
                target.len(),
                self.meta.target_len,
                self.meta.target
            )));
        }
        Ok(())
    }

    /// Comprueba que cada rango contiene lo que debería tras `written` escrituras:
    /// originales, con las nuevas aplicadas en orden encima. El registro `written`
    /// pudo quedar a medias, así que su rango admite cualquier contenido.
    pub fn check_expected(&self, target: &dyn Target, written: u32) -> TxResult<()> {
        for r in &self.records {
            let mut expected = r.read_orig(&self.file)?;
            let mut unknown = vec![false; r.len as usize];
            for k in &self.records[..written.min(self.total) as usize + usize::from(written < self.total)] {
                let (a, b) = (r.offset.max(k.offset), (r.offset + r.len as u64).min(k.offset + k.len as u64));
                if a >= b {
                    continue;
                }
                let (ri, ki, n) = ((a - r.offset) as usize, (a - k.offset) as usize, (b - a) as usize);
                if k.seq < written {
                    let new = k.read_new(&self.file)?;
                    expected[ri..ri + n].copy_from_slice(&new[ki..ki + n]);
                    unknown[ri..ri + n].fill(false);
                } else {
                    unknown[ri..ri + n].fill(true);
                }
            }
            let mut actual = vec![0u8; r.len as usize];
            target.read_at(r.offset, &mut actual)?;
            if actual.iter().zip(&expected).zip(&unknown).any(|((a, e), u)| !u && a != e) {
                return Err(TxError::ForeignChange { seq: r.seq });
            }
        }
        Ok(())
    }
}

/// Lee un registro de deshacer y comprueba su integridad.
pub fn inspect(path: &Path) -> TxResult<UndoInfo> {
    let u = UndoFile::open(path, false)?;
    u.check_integrity()?;
    Ok(UndoInfo {
        bytes_saved: u.records.iter().map(|r| r.len as u64).sum(),
        records: u
            .records
            .iter()
            .map(|r| UndoRecordInfo { seq: r.seq, offset: r.offset, len: r.len, phase: r.phase })
            .collect(),
        meta: u.meta,
        state: u.state,
        written: u.written,
        total: u.total,
    })
}

/// Restaura los bytes originales (en orden inverso) y verifica el resultado.
/// Sin `force`, se niega si algún rango contiene algo distinto de lo esperado.
pub fn rollback(path: &Path, target: &mut dyn Target, force: bool) -> TxResult<()> {
    let mut u = UndoFile::open(path, true)?;
    u.check_integrity()?;
    u.check_target(target)?;
    if u.state == TxState::RolledBack {
        return Ok(());
    }
    if !force {
        u.check_expected(target, u.written)?;
    }
    for r in u.records.iter().rev() {
        target.write_at(r.offset, &r.read_orig(&u.file)?)?;
        target.sync()?;
    }
    for r in &u.records {
        let mut actual = vec![0u8; r.len as usize];
        target.read_at(r.offset, &mut actual)?;
        if sha(&actual) != r.orig_sha {
            return Err(TxError::VerifyFailed { seq: r.seq, offset: r.offset });
        }
    }
    u.set_state(TxState::RolledBack, 0)
}

/// Completa una transacción interrumpida aplicando de nuevo todos los bytes nuevos.
pub fn roll_forward(path: &Path, target: &mut dyn Target) -> TxResult<()> {
    let mut u = UndoFile::open(path, true)?;
    u.check_integrity()?;
    u.check_target(target)?;
    match u.state {
        TxState::RolledBack => return Err(TxError::BadState(u.state)),
        TxState::Finalized | TxState::Verified => return Ok(()),
        _ => {}
    }
    u.check_expected(target, u.written)?;
    u.set_state(TxState::Writing, u.written)?;
    for r in &u.records {
        target.write_at(r.offset, &r.read_new(&u.file)?)?;
        target.sync()?;
    }
    u.check_expected(target, u.total)?;
    u.set_state(TxState::Finalized, u.total)
}

/// Marca la transacción como verificada (el llamador comprobó el volumen resultante).
pub fn mark_verified(path: &Path) -> TxResult<()> {
    let mut u = UndoFile::open(path, true)?;
    if u.state != TxState::Finalized {
        return Err(TxError::BadState(u.state));
    }
    let total = u.total;
    u.set_state(TxState::Verified, total)
}
