//! Preparación y aplicación de una transacción.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use blocksmith_fs_core::WritePhase;

use crate::target::{pwrite, Target};
use crate::undo::{encode_header, encode_record, sha, TxMeta, TxState, UndoFile, HEADER};
use crate::{TxError, TxResult};

/// Escritura con offset **absoluto** en el destino.
#[derive(Debug, Clone)]
pub struct PlannedWrite {
    pub offset: u64,
    pub data: Vec<u8>,
    pub phase: WritePhase,
    pub label: String,
}

/// Decisión del observador antes de cada escritura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Continue,
    /// Cancelar: solo se respeta antes de la fase de commit.
    Stop,
    /// Pruebas: simula un corte de corriente justo antes de esta escritura.
    Crash,
    /// Pruebas: simula un corte a mitad de esta escritura (en un límite de sector).
    Torn,
}

pub trait Observer {
    fn before_write(&mut self, _seq: u32, _phase: WritePhase) -> Control {
        Control::Continue
    }
    fn progress(&mut self, _done: u32, _total: u32, _label: &str) {}
}

impl Observer for () {}

pub struct Transaction {
    pub path: PathBuf,
    pub(crate) undo: UndoFile,
}

impl Transaction {
    pub fn meta(&self) -> &TxMeta {
        &self.undo.meta
    }
    pub fn state(&self) -> TxState {
        self.undo.state
    }
    pub fn written(&self) -> u32 {
        self.undo.written
    }
    pub fn total(&self) -> u32 {
        self.undo.total
    }

    pub fn open(path: &Path) -> TxResult<Self> {
        Ok(Self { path: path.to_path_buf(), undo: UndoFile::open(path, true)? })
    }
}

fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Lee los originales, escribe el registro de deshacer completo y lo sincroniza.
/// El destino no se modifica. Las escrituras se ordenan por fase (orden estable).
pub fn prepare(
    target: &dyn Target,
    mut writes: Vec<PlannedWrite>,
    undo_path: &Path,
    mut meta: TxMeta,
) -> TxResult<Transaction> {
    if target.len() != meta.target_len {
        return Err(TxError::WrongTarget(format!("tamaño {} y se esperaba {}", target.len(), meta.target_len)));
    }
    writes.sort_by_key(|w| w.phase);
    for w in &writes {
        let end = w.offset + w.data.len() as u64;
        let vol_end = meta.base_offset + meta.volume_len;
        if w.offset < meta.base_offset || end > vol_end || end > target.len() || w.data.is_empty() {
            return Err(TxError::OutOfRange { label: w.label.clone(), offset: w.offset, len: w.data.len() });
        }
    }
    meta.first_commit = writes.iter().position(|w| w.phase == WritePhase::Commit).map(|i| i as u32);
    let total = writes.len() as u32;
    let file = OpenOptions::new().read(true).write(true).create_new(true).open(undo_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            TxError::UndoExists(undo_path.display().to_string())
        } else {
            e.into()
        }
    })?;
    // Cabecera con estado 0: si el proceso muere aquí, el registro se reconoce como incompleto.
    let mut h = encode_header(TxState::Prepared, 0, total, &meta)?;
    h[20..24].fill(0);
    pwrite(&file, 0, &h)?;
    let mut pos = HEADER;
    for (seq, w) in writes.iter().enumerate() {
        let mut orig = vec![0u8; w.data.len()];
        target.read_at(w.offset, &mut orig)?;
        let rec = encode_record(seq as u32, w.offset, w.phase, &orig, &w.data);
        pwrite(&file, pos, &rec)?;
        pos += rec.len() as u64;
    }
    file.sync_all()?;
    pwrite(&file, 0, &encode_header(TxState::Prepared, 0, total, &meta)?)?;
    file.sync_all()?;
    sync_parent(undo_path);
    drop(file);
    Transaction::open(undo_path)
}

/// Aplica (o reanuda) la transacción. Cada escritura se sincroniza y se relee.
pub fn apply(tx: &mut Transaction, target: &mut dyn Target, obs: &mut dyn Observer) -> TxResult<()> {
    let u = &mut tx.undo;
    match u.state {
        TxState::Prepared | TxState::Writing | TxState::Committed => {}
        other => return Err(TxError::BadState(other)),
    }
    if target.len() != u.meta.target_len {
        return Err(TxError::WrongTarget(format!("tamaño {} y se esperaba {}", target.len(), u.meta.target_len)));
    }
    if u.state == TxState::Prepared {
        u.set_state(TxState::Writing, 0)?;
    }
    let first_commit = u.meta.first_commit.unwrap_or(u.total);
    let records = u.records.clone();
    for r in records.iter().skip(u.written as usize) {
        match obs.before_write(r.seq, r.phase) {
            Control::Stop if r.seq < first_commit => return Err(TxError::Cancelled),
            Control::Crash => return Err(TxError::Interrupted { written: u.written, total: u.total }),
            Control::Torn => {
                // Los discos escriben cada sector de 512 bytes de forma atómica: el corte
                // solo puede caer entre sectores (una escritura de un sector no se parte).
                let new = r.read_new(&u.file)?;
                let cut = (new.len() / 2) / 512 * 512;
                target.write_at(r.offset, &new[..cut])?;
                target.sync()?;
                return Err(TxError::Interrupted { written: u.written, total: u.total });
            }
            _ => {}
        }
        let new = r.read_new(&u.file)?;
        target.write_at(r.offset, &new)?;
        target.sync()?;
        let mut back = vec![0u8; new.len()];
        target.read_at(r.offset, &mut back)?;
        if sha(&back) != r.new_sha {
            return Err(TxError::VerifyFailed { seq: r.seq, offset: r.offset });
        }
        let done = r.seq + 1;
        let next_phase = records.get(done as usize).map(|n| n.phase);
        let state = if done == u.total {
            TxState::Finalized
        } else if r.phase == WritePhase::Commit && next_phase != Some(WritePhase::Commit) {
            TxState::Committed
        } else {
            u.state
        };
        u.set_state(state, done)?;
        obs.progress(done, u.total, &format!("{:?}", r.phase));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{inspect, mark_verified, roll_forward, rollback, MemTarget};

    fn meta(len: u64) -> TxMeta {
        TxMeta {
            target: "memoria".into(),
            target_len: len,
            base_offset: 0,
            volume_len: len,
            created_at: "2026-01-01T00:00:00Z".into(),
            plan_sha256: "x".into(),
            label: "prueba".into(),
            first_commit: None,
        }
    }

    fn writes() -> Vec<PlannedWrite> {
        let w = |offset, byte, phase| PlannedWrite { offset, data: vec![byte; 1024], phase, label: format!("{byte}") };
        // Incluye dos escrituras solapadas en el mismo rango (como el bitmap de transición y el final).
        vec![
            w(512, 0xA1, WritePhase::Data),
            w(6144, 0xC1, WritePhase::Commit),
            w(3072, 0xA2, WritePhase::Data),
            w(3072, 0xF1, WritePhase::Finalize),
        ]
    }

    fn original() -> Vec<u8> {
        (0..8192u32).map(|i| (i % 251) as u8).collect()
    }

    struct CrashAt(u32, Control);
    impl Observer for CrashAt {
        fn before_write(&mut self, seq: u32, _: WritePhase) -> Control {
            if seq == self.0 {
                self.1
            } else {
                Control::Continue
            }
        }
    }

    fn full_result() -> Vec<u8> {
        let mut t = MemTarget(original());
        let dir = tempfile::tempdir().unwrap();
        let mut tx = prepare(&t, writes(), &dir.path().join("u"), meta(8192)).unwrap();
        apply(&mut tx, &mut t, &mut ()).unwrap();
        t.0
    }

    #[test]
    fn full_apply_orders_phases_and_finalizes() {
        let dir = tempfile::tempdir().unwrap();
        let undo = dir.path().join("tx.undo");
        let mut t = MemTarget(original());
        let mut tx = prepare(&t, writes(), &undo, meta(8192)).unwrap();
        assert_eq!(tx.state(), TxState::Prepared);
        assert_eq!(t.0, original(), "preparar no toca el destino");
        apply(&mut tx, &mut t, &mut ()).unwrap();
        assert_eq!(tx.state(), TxState::Finalized);
        assert_eq!(&t.0[3072..4096], &[0xF1; 1024], "la finalización gana sobre los datos");
        let info = inspect(&undo).unwrap();
        assert_eq!((info.written, info.total, info.meta.first_commit), (4, 4, Some(2)));
        let phases: Vec<WritePhase> = info.records.iter().map(|r| r.phase).collect();
        assert_eq!(phases, vec![WritePhase::Data, WritePhase::Data, WritePhase::Commit, WritePhase::Finalize]);
        mark_verified(&undo).unwrap();
        // Deshacer una reparación completa y verificada también es posible.
        rollback(&undo, &mut t, false).unwrap();
        assert_eq!(t.0, original());
        assert!(prepare(&t, writes(), &undo, meta(8192)).is_err(), "nunca sobrescribe un registro");
    }

    #[test]
    fn every_interruption_can_be_rolled_back_or_completed() {
        let done = full_result();
        for seq in 0..4 {
            for mode in [Control::Crash, Control::Torn] {
                for forward in [false, true] {
                    let dir = tempfile::tempdir().unwrap();
                    let undo = dir.path().join("tx.undo");
                    let mut t = MemTarget(original());
                    let mut tx = prepare(&t, writes(), &undo, meta(8192)).unwrap();
                    let err = apply(&mut tx, &mut t, &mut CrashAt(seq, mode)).unwrap_err();
                    assert!(matches!(err, TxError::Interrupted { .. }));
                    assert!(inspect(&undo).unwrap().state.is_interrupted());
                    if forward {
                        roll_forward(&undo, &mut t).unwrap();
                        assert_eq!(t.0, done, "completar tras {mode:?} en {seq}");
                    } else {
                        rollback(&undo, &mut t, false).unwrap();
                        assert_eq!(t.0, original(), "revertir tras {mode:?} en {seq}");
                        assert_eq!(inspect(&undo).unwrap().state, TxState::RolledBack);
                    }
                }
            }
        }
    }

    #[test]
    fn foreign_changes_block_rollback_unless_forced() {
        let dir = tempfile::tempdir().unwrap();
        let undo = dir.path().join("tx.undo");
        let mut t = MemTarget(original());
        let mut tx = prepare(&t, writes(), &undo, meta(8192)).unwrap();
        let _ = apply(&mut tx, &mut t, &mut CrashAt(2, Control::Crash));
        t.0[600] ^= 0xFF; // otro programa tocó un bloque ya escrito
        assert!(matches!(rollback(&undo, &mut t, false), Err(TxError::ForeignChange { seq: 0 })));
        assert!(matches!(roll_forward(&undo, &mut t), Err(TxError::ForeignChange { .. })));
        rollback(&undo, &mut t, true).unwrap();
        assert_eq!(t.0, original());
    }

    #[test]
    fn cancel_only_before_commit() {
        struct StopAll;
        impl Observer for StopAll {
            fn before_write(&mut self, _: u32, _: WritePhase) -> Control {
                Control::Stop
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let undo = dir.path().join("tx.undo");
        let mut t = MemTarget(original());
        let mut tx = prepare(&t, writes(), &undo, meta(8192)).unwrap();
        assert!(matches!(apply(&mut tx, &mut t, &mut StopAll), Err(TxError::Cancelled)));
        // Reanudar hasta pasar el commit: a partir de ahí, «parar» se ignora.
        let mut tx = Transaction::open(&undo).unwrap();
        apply(&mut tx, &mut t, &mut CrashAt(2, Control::Crash)).unwrap_err();
        let mut tx = Transaction::open(&undo).unwrap();
        apply(&mut tx, &mut t, &mut StopAll).unwrap();
        assert_eq!(tx.state(), TxState::Finalized);
    }

    #[test]
    fn rejects_bad_inputs_and_corrupt_logs() {
        let dir = tempfile::tempdir().unwrap();
        let t = MemTarget(original());
        let mut bad = writes();
        bad[0].offset = 7500;
        assert!(matches!(prepare(&t, bad, &dir.path().join("a"), meta(8192)), Err(TxError::OutOfRange { .. })));
        let mut m = meta(8192);
        m.base_offset = 1024;
        m.volume_len = 4096;
        assert!(matches!(prepare(&t, writes(), &dir.path().join("b"), m), Err(TxError::OutOfRange { .. })));
        assert!(matches!(prepare(&t, writes(), &dir.path().join("c"), meta(4096)), Err(TxError::WrongTarget(_))));

        let undo = dir.path().join("d");
        prepare(&t, writes(), &undo, meta(8192)).unwrap();
        let mut raw = std::fs::read(&undo).unwrap();
        let n = raw.len();
        raw[n - 10] ^= 1; // daño en los bytes guardados
        std::fs::write(&undo, &raw).unwrap();
        assert!(matches!(inspect(&undo), Err(TxError::BadUndo(_))));
        std::fs::write(&undo, b"basura").unwrap();
        assert!(matches!(inspect(&undo), Err(TxError::BadUndo(_))));
    }
}
