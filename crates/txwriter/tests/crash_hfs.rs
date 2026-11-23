//! Seguridad ante cortes con volúmenes HFS+ reales: la transacción se interrumpe en
//! cada escritura posible (corte limpio y a medias) y se comprueba que
//! - antes del commit sigue vigente la versión vieja (sin daños nuevos),
//! - después del commit la nueva (sana, como mucho con bloques de más en el bitmap),
//! - revertir devuelve exactamente el original y completar, el resultado esperado.

use std::collections::BTreeSet;
use std::process::Command;

use blocksmith_fs_core::{Progress, Severity, WritePhase};
use blocksmith_fs_hfsplus::fixtures::{apply_damage, build_volume, sample_tree, BuildOptions, Damage};
use blocksmith_fs_hfsplus::rebuild::plan_rebuild;
use blocksmith_fs_hfsplus::{scan_volume, Volume};
use blocksmith_io::MemoryDevice;
use blocksmith_txwriter::{
    apply, prepare, roll_forward, rollback, Control, MemTarget, Observer, PlannedWrite, TxError, TxMeta,
};

struct Quiet;
impl Progress for Quiet {
    fn phase(&mut self, _: &str) {}
    fn advance(&mut self, _: u64, _: u64) {}
    fn is_cancelled(&self) -> bool {
        false
    }
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

/// (códigos no cosméticos, archivos en el catálogo)
fn health(img: &[u8]) -> (BTreeSet<String>, usize) {
    let dev = MemoryDevice::new(img.to_vec());
    let vol = Volume::open(&dev).unwrap().expect("sigue siendo HFS+");
    let s = scan_volume(&vol, &mut Quiet).unwrap();
    let codes = s.result.findings.iter().filter(|f| f.severity != Severity::Cosmetic).map(|f| f.code.clone()).collect();
    (codes, s.catalog.files.len())
}

fn fsck_ok(img: &[u8]) -> Option<bool> {
    if !Command::new("sh").arg("-c").arg("command -v fsck.hfsplus").output().ok()?.status.success() {
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("v.img");
    std::fs::write(&p, img).unwrap();
    let o = Command::new("fsck.hfsplus").args(["-n", "-f"]).arg(&p).output().unwrap();
    Some(o.status.success() && String::from_utf8_lossy(&o.stdout).contains("appears to be OK"))
}

/// Bloques marcados de más en el bitmap: inofensivo (el espacio no se reutiliza).
const BENIGN: &[&str] = &["HFS_BITMAP_LEAK", "HFS_FREE_COUNT"];

#[test]
fn interruption_at_every_write_is_safe() {
    let base =
        build_volume(&BuildOptions { max_records_per_leaf: Some(6), ..Default::default() }, &sample_tree()).unwrap();
    let cases: [(&str, Option<Damage>); 3] = [
        ("sano", None),
        ("huérfanos", Some(Damage::FolderAndThreadLost)),
        ("bloques compartidos", Some(Damage::ExtentOverlap)),
    ];
    let mut checked = 0;
    for (name, damage) in cases {
        let mut orig = base.clone();
        if let Some(d) = damage {
            apply_damage(&mut orig, d).unwrap();
        }
        let (orig_codes, orig_files) = health(&orig);
        let dev = MemoryDevice::new(orig.clone());
        let vol = Volume::open(&dev).unwrap().unwrap();
        let plan = plan_rebuild(&vol, &scan_volume(&vol, &mut Quiet).unwrap()).unwrap();
        let writes: Vec<PlannedWrite> = plan
            .ordered_writes()
            .map(|w| PlannedWrite { offset: w.offset, data: w.data.clone(), phase: w.phase, label: w.label.clone() })
            .collect();
        // La cabecera principal es la primera escritura de commit: hasta escribirla, manda la versión vieja.
        let first_commit = writes.iter().position(|w| w.phase == WritePhase::Commit).unwrap() as u32;
        let meta = TxMeta {
            target: name.into(),
            target_len: orig.len() as u64,
            base_offset: 0,
            volume_len: orig.len() as u64,
            created_at: String::new(),
            plan_sha256: String::new(),
            label: name.into(),
            first_commit: None,
        };
        let dir = tempfile::tempdir().unwrap();

        let mut full = MemTarget(orig.clone());
        let mut tx = prepare(&full, writes.clone(), &dir.path().join("full"), meta.clone()).unwrap();
        apply(&mut tx, &mut full, &mut ()).unwrap();
        if let Some(ok) = fsck_ok(&full.0) {
            assert!(ok, "{name}: fsck.hfsplus rechaza el resultado completo");
        }

        for seq in 0..writes.len() as u32 {
            for mode in [Control::Crash, Control::Torn] {
                let undo = dir.path().join(format!("{seq}-{mode:?}"));
                let mut t = MemTarget(orig.clone());
                let mut tx = prepare(&t, writes.clone(), &undo, meta.clone()).unwrap();
                assert!(matches!(apply(&mut tx, &mut t, &mut CrashAt(seq, mode)), Err(TxError::Interrupted { .. })));
                let (codes, files) = health(&t.0);
                let what = format!(
                    "{name}: corte {mode:?} antes de completar la escritura {seq} ({})",
                    writes[seq as usize].label
                );
                if seq <= first_commit {
                    // La cabecera nueva no está completa: versión vieja, sin daños nuevos.
                    let new: Vec<_> = codes.difference(&orig_codes).filter(|c| !BENIGN.contains(&c.as_str())).collect();
                    assert!(new.is_empty(), "{what}: daños nuevos {new:?}");
                    assert_eq!(files, orig_files, "{what}");
                } else {
                    // Commit hecho: versión nueva y sana.
                    let bad: Vec<_> = codes.iter().filter(|c| !BENIGN.contains(&c.as_str())).collect();
                    assert!(bad.is_empty(), "{what}: {bad:?}");
                    assert_eq!(files, 42, "{what}");
                }
                let mut back = MemTarget(t.0.clone());
                rollback(&undo, &mut back, false).unwrap();
                assert!(back.0 == orig, "{what}: revertir no devolvió el original exacto");
                let fwd_undo = dir.path().join(format!("{seq}-{mode:?}-fwd"));
                let mut fwd = MemTarget(orig.clone());
                let mut tx = prepare(&fwd, writes.clone(), &fwd_undo, meta.clone()).unwrap();
                let _ = apply(&mut tx, &mut fwd, &mut CrashAt(seq, mode));
                roll_forward(&fwd_undo, &mut fwd).unwrap();
                assert!(fwd.0 == full.0, "{what}: completar no dio el resultado esperado");
                checked += 1;
            }
        }
    }
    assert!(checked >= 30, "solo {checked} casos");
    eprintln!("{checked} interrupciones comprobadas");
}
