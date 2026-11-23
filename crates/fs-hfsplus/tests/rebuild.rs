//! Reconstrucción de volúmenes dañados: el resultado debe pasar el escaneo propio y
//! `fsck.hfsplus`, sin perder archivos ni alterar el contenido de los no afectados.

use std::collections::{BTreeMap, HashSet};
use std::process::Command;

use blocksmith_fs_core::{DiffChange, Progress, RebuildPlan, Severity};
use blocksmith_fs_hfsplus::fixtures::{apply_damage, build_volume, sample_tree, BuildOptions, Damage};
use blocksmith_fs_hfsplus::rebuild::{plan_rebuild, RECOVERED_FOLDER};
use blocksmith_fs_hfsplus::{scan_volume, HfsScan, Volume};
use blocksmith_io::MemoryDevice;
use blocksmith_rebuilder::apply_in_memory;

struct Quiet;
impl Progress for Quiet {
    fn phase(&mut self, _: &str) {}
    fn advance(&mut self, _: u64, _: u64) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

fn scan(dev: &MemoryDevice) -> HfsScan {
    let vol = Volume::open(dev).unwrap().expect("HFS+");
    scan_volume(&vol, &mut Quiet).unwrap()
}

/// Contenido de cada archivo por CNID.
fn contents(img: &[u8]) -> BTreeMap<u32, Vec<u8>> {
    let dev = MemoryDevice::new(img.to_vec());
    let vol = Volume::open(&dev).unwrap().unwrap();
    let s = scan_volume(&vol, &mut Quiet).unwrap();
    s.catalog
        .files
        .iter()
        .map(|(id, (_, r))| {
            let reader = vol.fork_reader(&r.data_fork, *id, true, &s.overflow);
            (*id, reader.read_vec(0, r.data_fork.logical_size as usize).unwrap())
        })
        .collect()
}

fn rebuild(img: &[u8]) -> (Vec<u8>, RebuildPlan) {
    let dev = MemoryDevice::new(img.to_vec());
    let vol = Volume::open(&dev).unwrap().unwrap();
    let s = scan_volume(&vol, &mut Quiet).unwrap();
    let plan = plan_rebuild(&vol, &s).unwrap();
    let mut out = img.to_vec();
    apply_in_memory(&mut out, 0, vol.vh.volume_bytes(), &plan).unwrap();
    (out, plan)
}

fn fsck_ok(img: &[u8]) -> Option<(bool, String)> {
    if !Command::new("sh").arg("-c").arg("command -v fsck.hfsplus").output().ok()?.status.success() {
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("v.img");
    std::fs::write(&p, img).unwrap();
    let o = Command::new("fsck.hfsplus").args(["-n", "-f"]).arg(&p).output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    Some((o.status.success() && text.contains("appears to be OK"), text))
}

fn opts() -> BuildOptions {
    BuildOptions { max_records_per_leaf: Some(6), ..Default::default() }
}

fn assert_healthy(img: &[u8], what: &str) -> HfsScan {
    let s = scan(&MemoryDevice::new(img.to_vec()));
    let serious: Vec<_> = s.result.findings.iter().filter(|f| f.severity != Severity::Cosmetic).collect();
    assert!(serious.is_empty(), "{what}: quedan hallazgos tras reconstruir: {serious:#?}");
    if let Some((ok, out)) = fsck_ok(img) {
        assert!(ok, "{what}: fsck.hfsplus rechaza el resultado:\n{out}");
    }
    s
}

#[test]
fn every_damage_is_repaired_without_losing_files() {
    let clean = build_volume(&opts(), &sample_tree()).unwrap();
    let original = contents(&clean);
    for damage in Damage::ALL {
        let mut img = clean.clone();
        apply_damage(&mut img, damage).unwrap();
        let (fixed, plan) = rebuild(&img);
        let what = format!("{damage:?}");
        let s = assert_healthy(&fixed, &what);
        assert_eq!(s.catalog.files.len(), 42, "{what}: archivos perdidos");
        assert_eq!(plan.lost_count, 0, "{what}");

        // Contenido idéntico salvo en los archivos que el plan declara alterados.
        let altered: HashSet<u32> = plan
            .diff
            .iter()
            .filter(|d| {
                matches!(d.change, DiffChange::DataCloned | DiffChange::DataTruncated | DiffChange::UnchangedFlagged)
            })
            .map(|d| d.object_id as u32)
            .collect();
        let after = contents(&fixed);
        for (id, data) in &original {
            if !altered.contains(id) {
                assert_eq!(after.get(id), Some(data), "{what}: el contenido del CNID {id} cambió");
            }
        }
        match damage {
            // Los dos archivos que comparten bloques quedan señalados.
            Damage::ExtentOverlap => assert_eq!(altered.len(), 2, "{what}"),
            Damage::ExtentOutOfRange => assert_eq!(altered.len(), 1, "{what}"),
            _ => assert!(altered.is_empty(), "{what}: {altered:?}"),
        }
    }
}

#[test]
fn lost_folder_is_recreated_from_its_thread() {
    let mut img = build_volume(&opts(), &sample_tree()).unwrap();
    apply_damage(&mut img, Damage::FolderRecordLost).unwrap();
    let (fixed, plan) = rebuild(&img);
    let s = assert_healthy(&fixed, "FolderRecordLost");
    assert!(
        plan.diff.iter().any(|d| d.change == DiffChange::Recovered
            && d.is_folder
            && d.new_path.as_deref() == Some("/Fotos/2023/Viaje Lisboa")),
        "{:#?}",
        plan.diff
    );
    let photo = s.catalog.files.keys().find(|id| s.catalog.path_of(**id).unwrap().ends_with("IMG_4401.HEIC")).unwrap();
    assert_eq!(s.catalog.path_of(*photo).unwrap(), "/Fotos/2023/Viaje Lisboa/IMG_4401.HEIC");
}

#[test]
fn orphans_go_to_recovered_items() {
    let mut img = build_volume(&opts(), &sample_tree()).unwrap();
    apply_damage(&mut img, Damage::FolderAndThreadLost).unwrap();
    let (fixed, plan) = rebuild(&img);
    let s = assert_healthy(&fixed, "FolderAndThreadLost");
    let moved = plan.diff.iter().filter(|d| d.change == DiffChange::MovedToRecovered).count();
    assert_eq!(moved, 24);
    let in_recovered = s
        .catalog
        .files
        .keys()
        .filter(|id| s.catalog.path_of(**id).unwrap().starts_with(&format!("/{RECOVERED_FOLDER}/")))
        .count();
    assert_eq!(in_recovered, 24);
}

#[test]
fn clean_volume_rebuild_is_compact_and_lossless() {
    let clean = build_volume(&opts(), &sample_tree()).unwrap();
    let (fixed, plan) = rebuild(&clean);
    assert_healthy(&fixed, "sano");
    assert!(plan.diff.is_empty(), "{:#?}", plan.diff);
    // Con 6 registros por hoja el original está medio vacío: el nuevo es más compacto.
    assert!(plan.nodes_after < plan.nodes_before, "{} → {}", plan.nodes_before, plan.nodes_after);
    assert_eq!(contents(&clean), contents(&fixed));
    // Reconstruir dos veces no cambia nada más.
    let (_, again) = rebuild(&fixed);
    assert!(again.diff.is_empty());
}

#[test]
fn hfsx_case_sensitive_names_survive() {
    let o = BuildOptions { case_sensitive: true, ..opts() };
    let tree = vec![
        blocksmith_fs_hfsplus::fixtures::Entry::file("Nota.txt", "A"),
        blocksmith_fs_hfsplus::fixtures::Entry::file("nota.txt", "b"),
    ];
    let mut img = build_volume(&o, &tree).unwrap();
    apply_damage(&mut img, Damage::FileCountWrong).unwrap();
    let (fixed, plan) = rebuild(&img);
    let s = assert_healthy(&fixed, "HFSX");
    assert_eq!(s.catalog.files.len(), 2);
    assert!(!plan.diff.iter().any(|d| d.change == DiffChange::Renamed), "en HFSX no hay conflicto de mayúsculas");
}

#[test]
fn mkfs_volumes_rebuild_cleanly() {
    if !Command::new("sh")
        .arg("-c")
        .arg("command -v mkfs.hfsplus")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return;
    }
    for args in [vec![], vec!["-J"], vec!["-s"]] {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("m.img");
        std::fs::write(&p, vec![0u8; 64 * 1024 * 1024]).unwrap();
        assert!(Command::new("mkfs.hfsplus")
            .args(&args)
            .args(["-v", "Externo"])
            .arg(&p)
            .output()
            .unwrap()
            .status
            .success());
        let (fixed, _) = rebuild(&std::fs::read(&p).unwrap());
        assert_healthy(&fixed, &format!("mkfs {args:?}"));
    }
}

#[test]
fn larger_volume_with_full_nodes() {
    use blocksmith_fs_hfsplus::fixtures::Entry;
    let tree: Vec<Entry> = (0..50)
        .map(|d| {
            Entry::folder(
                &format!("Carpeta {d:02}"),
                (0..50).map(|f| Entry::file(&format!("documento-{d:02}-{f:03}.txt"), format!("{d}/{f}"))).collect(),
            )
        })
        .collect();
    let o = BuildOptions { size_bytes: 64 * 1024 * 1024, ..Default::default() };
    let clean = build_volume(&o, &tree).unwrap();
    for damage in [Damage::BrokenLink, Damage::NodeUnreadable, Damage::FolderAndThreadLost, Damage::KeyOrder] {
        let mut img = clean.clone();
        apply_damage(&mut img, damage).unwrap();
        let (fixed, plan) = rebuild(&img);
        let s = assert_healthy(&fixed, &format!("grande {damage:?}"));
        assert_eq!(s.catalog.files.len(), 2500, "{damage:?}");
        assert_eq!(plan.lost_count, 0);
        assert!(s.result.stats.nodes_total > 100);
    }
}

#[test]
fn volume_name_and_trees() {
    let img = build_volume(&opts(), &sample_tree()).unwrap();
    let dev = MemoryDevice::new(img.clone());
    let vol = Volume::open(&dev).unwrap().unwrap();
    assert_eq!(blocksmith_fs_hfsplus::volume::volume_name(&vol).as_deref(), Some("BlockSmith Test"));
    let s = scan_volume(&vol, &mut Quiet).unwrap();
    assert_eq!(s.catalog.entries().len(), 42 + 8 + 1);
    let (_, plan) = rebuild(&img);
    assert_eq!(plan.tree_after.len(), 42 + 8 + 1);
    assert!(plan.tree_after.iter().any(|e| e.name == "Viaje Lisboa" && e.is_folder));
}
