//! Fixtures generadas byte a byte, validadas con un verificador independiente
//! (`fsck.hfsplus`, si está instalado) y con el propio escaneo de DirForge.

use std::process::Command;

use dirforge_fs_core::{Progress, Severity};
use dirforge_fs_hfsplus::fixtures::{apply_damage, build_volume, sample_tree, BuildOptions, Damage};
use dirforge_fs_hfsplus::{scan_volume, HfsScan, Volume};
use dirforge_io::MemoryDevice;

struct Quiet;
impl Progress for Quiet {
    fn phase(&mut self, _: &str) {}
    fn advance(&mut self, _: u64, _: u64) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

fn scan(img: Vec<u8>) -> HfsScan {
    let dev = MemoryDevice::new(img);
    let vol = Volume::open(&dev).unwrap().expect("volumen HFS+");
    scan_volume(&vol, &mut Quiet).unwrap()
}

fn multi_level() -> BuildOptions {
    BuildOptions { max_records_per_leaf: Some(6), ..Default::default() }
}

/// Ejecuta fsck.hfsplus en modo solo lectura. `None` si no está instalado.
fn fsck(img: &[u8]) -> Option<(bool, String)> {
    let which = Command::new("sh").arg("-c").arg("command -v fsck.hfsplus").output().ok()?;
    if !which.status.success() {
        eprintln!("fsck.hfsplus no disponible: se omite la validación independiente");
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vol.img");
    std::fs::write(&path, img).unwrap();
    let out = Command::new("fsck.hfsplus").arg("-n").arg("-f").arg(&path).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    Some((out.status.success() && text.contains("appears to be OK"), text))
}

#[test]
fn clean_volume_passes_both_checkers() {
    for opts in [BuildOptions::default(), multi_level(), BuildOptions { case_sensitive: true, ..multi_level() }] {
        let img = build_volume(&opts, &sample_tree()).unwrap();
        if let Some((ok, out)) = fsck(&img) {
            assert!(ok, "fsck.hfsplus rechaza la fixture ({opts:?}):\n{out}");
        }
        let s = scan(img);
        assert!(s.result.findings.is_empty(), "{opts:?}: {:#?}", s.result.findings);
        assert_eq!(s.result.stats.files, 42);
        assert_eq!(s.result.stats.folders, 8);
        assert_eq!(s.result.info.name.as_deref(), Some("DirForge Test"));
        assert_eq!(
            s.catalog.path_of(*s.catalog.files.iter().find(|(_, (k, _))| k.name.len() == 13 && k.name[0] == b'I' as u16).unwrap().0).unwrap(),
            "/Fotos/2023/Viaje Lisboa/IMG_4401.HEIC"
        );
    }
}

#[test]
fn multi_level_tree_has_index_nodes() {
    let s = scan(build_volume(&multi_level(), &sample_tree()).unwrap());
    // 2 registros por elemento + raíz → 106 registros / 6 por hoja = 18 hojas y 2 niveles de índice.
    assert!(s.result.stats.nodes_total >= 32);
}

#[test]
fn each_damage_is_detected() {
    for damage in Damage::ALL {
        let mut img = build_volume(&multi_level(), &sample_tree()).unwrap();
        apply_damage(&mut img, damage).unwrap();
        if let Some((ok, out)) = fsck(&img) {
            assert!(!ok, "fsck.hfsplus no detecta {damage:?}:\n{out}");
        }
        let s = scan(img);
        let codes: Vec<&str> = s.result.findings.iter().map(|f| f.code.as_str()).collect();
        assert!(codes.contains(&damage.expected_code()), "{damage:?}: se esperaba {} y se obtuvo {codes:?}", damage.expected_code());
    }
}

#[test]
fn broken_link_records_are_recovered_by_sweep() {
    let mut img = build_volume(&multi_level(), &sample_tree()).unwrap();
    apply_damage(&mut img, Damage::BrokenLink).unwrap();
    let s = scan(img);
    assert!(s.result.findings.iter().any(|f| f.code == "HFS_LEAF_NOT_LINKED"));
    // Ningún archivo se pierde: el barrido recupera los registros de las hojas desenlazadas.
    assert_eq!(s.catalog.files.len(), 42);
    assert!(!s.result.findings.iter().any(|f| f.code == "HFS_ORPHAN_RECORD"));
}

#[test]
fn unreadable_node_leaves_orphans() {
    let mut img = build_volume(&multi_level(), &sample_tree()).unwrap();
    apply_damage(&mut img, Damage::NodeUnreadable).unwrap();
    let s = scan(img);
    assert!(s.result.findings.iter().any(|f| f.severity == Severity::Critical));
    assert!(s.catalog.files.len() < 42);
}

/// Volúmenes creados por mkfs.hfsplus: un productor independiente del nuestro.
#[test]
fn reads_mkfs_volumes() {
    let ok = Command::new("sh").arg("-c").arg("command -v mkfs.hfsplus").output().map(|o| o.status.success()).unwrap_or(false);
    if !ok {
        eprintln!("mkfs.hfsplus no disponible: test omitido");
        return;
    }
    for args in [vec![], vec!["-J"], vec!["-s"]] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mkfs.img");
        std::fs::write(&path, vec![0u8; 64 * 1024 * 1024]).unwrap();
        let st = Command::new("mkfs.hfsplus").args(&args).args(["-v", "Externo"]).arg(&path).output().unwrap();
        assert!(st.status.success());
        let s = scan(std::fs::read(&path).unwrap());
        let serious: Vec<_> = s.result.findings.iter().filter(|f| f.severity != Severity::Cosmetic).collect();
        assert!(serious.is_empty(), "mkfs {args:?}: {serious:#?}");
        assert_eq!(s.result.info.name.as_deref(), Some("Externo"));
    }
}
