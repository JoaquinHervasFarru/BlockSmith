//! El helper como proceso: aplicar, rechazar trabajos manipulados y recuperar
//! transacciones interrumpidas.

use std::path::{Path, PathBuf};
use std::process::Command;

use blocksmith_fs_core::Progress;
use blocksmith_fs_hfsplus::fixtures::{apply_damage, build_volume, sample_tree, BuildOptions, Damage};
use blocksmith_fs_hfsplus::rebuild::plan_rebuild;
use blocksmith_fs_hfsplus::{scan_volume, Volume};
use blocksmith_helper::{sha256_hex, HelperEvent, Job};
use blocksmith_io::RawImage;

struct Quiet;
impl Progress for Quiet {
    fn phase(&mut self, _: &str) {}
    fn advance(&mut self, _: u64, _: u64) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

const BIN: &str = env!("CARGO_BIN_EXE_blocksmith-helper");

struct Setup {
    dir: tempfile::TempDir,
    image: PathBuf,
    job: PathBuf,
    job_sha: String,
    original: Vec<u8>,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let mut img =
        build_volume(&BuildOptions { max_records_per_leaf: Some(6), ..Default::default() }, &sample_tree()).unwrap();
    apply_damage(&mut img, Damage::FolderAndThreadLost).unwrap();
    let image = dir.path().join("dañada.img");
    std::fs::write(&image, &img).unwrap();
    let dev = RawImage::open(&image).unwrap();
    let vol = Volume::open(&dev).unwrap().unwrap();
    let plan = plan_rebuild(&vol, &scan_volume(&vol, &mut Quiet).unwrap()).unwrap();
    let job = Job::from_plan(&plan, &image.to_string_lossy(), img.len() as u64, 0, img.len() as u64, "BlockSmith Test");
    let raw = serde_json::to_vec(&job).unwrap();
    let job_path = dir.path().join("trabajo.json");
    std::fs::write(&job_path, &raw).unwrap();
    Setup { job_sha: sha256_hex(&raw), job: job_path, image, original: img, dir }
}

fn run(args: &[&str], env: &[(&str, &str)]) -> (i32, Vec<HelperEvent>) {
    let mut c = Command::new(BIN);
    c.args(args);
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().unwrap();
    let events = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    (out.status.code().unwrap_or(-1), events)
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn healthy(image: &Path) -> bool {
    let dev = RawImage::open(image).unwrap();
    let vol = Volume::open(&dev).unwrap().unwrap();
    scan_volume(&vol, &mut Quiet)
        .unwrap()
        .result
        .findings
        .iter()
        .all(|f| f.severity == blocksmith_fs_core::Severity::Cosmetic)
}

#[test]
fn applies_and_verifies() {
    let st = setup();
    let undo = st.dir.path().join("tx.undo");
    let (code, ev) = run(
        &["apply", "--target", s(&st.image), "--job", s(&st.job), "--job-sha256", &st.job_sha, "--undo", s(&undo)],
        &[],
    );
    assert_eq!(code, 0, "{ev:?}");
    assert!(ev.iter().any(|e| matches!(e, HelperEvent::Progress { .. })));
    assert!(matches!(ev.last(), Some(HelperEvent::Done { verified: true, .. })), "{ev:?}");
    assert!(healthy(&st.image));
    let (_, _) = run(&["inspect", "--undo", s(&undo)], &[]);
    let info = Command::new(BIN).args(["inspect", "--undo", s(&undo)]).output().unwrap();
    assert!(String::from_utf8_lossy(&info.stdout).contains("\"state\":\"verified\""));
    // Deshacer la reparación completa devuelve la imagen original.
    let (code, _) = run(&["rollback", "--target", s(&st.image), "--undo", s(&undo)], &[]);
    assert_eq!(code, 0);
    assert!(std::fs::read(&st.image).unwrap() == st.original);
}

#[test]
fn rejects_tampered_or_mismatched_jobs() {
    let st = setup();
    let undo = st.dir.path().join("tx.undo");
    // Alguien modifica el trabajo después de que la app calculara su hash.
    let mut raw = std::fs::read(&st.job).unwrap();
    let i = raw.windows(8).position(|w| w == b"\"offset\"").unwrap() + 9;
    raw[i] = if raw[i] == b'1' { b'2' } else { b'1' };
    std::fs::write(&st.job, &raw).unwrap();
    let (code, ev) = run(
        &["apply", "--target", s(&st.image), "--job", s(&st.job), "--job-sha256", &st.job_sha, "--undo", s(&undo)],
        &[],
    );
    assert_ne!(code, 0);
    assert!(matches!(&ev[0], HelperEvent::Error { message, .. } if message.contains("hash")), "{ev:?}");
    assert!(std::fs::read(&st.image).unwrap() == st.original && !undo.exists());

    // Trabajo válido pero para otro destino.
    let st = setup();
    let other = st.dir.path().join("otra.img");
    std::fs::write(&other, &st.original).unwrap();
    let (code, ev) = run(
        &["apply", "--target", s(&other), "--job", s(&st.job), "--job-sha256", &st.job_sha, "--undo", s(&undo)],
        &[],
    );
    assert_ne!(code, 0);
    assert!(matches!(&ev[0], HelperEvent::Error { message, .. } if message.contains("es para")), "{ev:?}");
}

#[test]
fn interrupted_then_rolled_back_or_completed() {
    for forward in [false, true] {
        let st = setup();
        let undo = st.dir.path().join("tx.undo");
        let (code, ev) = run(
            &["apply", "--target", s(&st.image), "--job", s(&st.job), "--job-sha256", &st.job_sha, "--undo", s(&undo)],
            &[("BLOCKSMITH_TX_FAIL_AFTER", "2")],
        );
        assert_eq!(code, 3, "{ev:?}");
        assert!(matches!(ev.last(), Some(HelperEvent::Error { interrupted: true, .. })), "{ev:?}");
        let info =
            String::from_utf8_lossy(&Command::new(BIN).args(["inspect", "--undo", s(&undo)]).output().unwrap().stdout)
                .to_string();
        assert!(info.contains("\"state\":\"writing\"") && info.contains("\"written\":2"), "{info}");
        if forward {
            let (code, ev) = run(&["roll-forward", "--target", s(&st.image), "--undo", s(&undo)], &[]);
            assert_eq!(code, 0, "{ev:?}");
            assert!(matches!(ev.last(), Some(HelperEvent::Done { verified: true, .. })));
            assert!(healthy(&st.image));
        } else {
            let (code, ev) = run(&["rollback", "--target", s(&st.image), "--undo", s(&undo)], &[]);
            assert_eq!(code, 0, "{ev:?}");
            assert!(std::fs::read(&st.image).unwrap() == st.original);
        }
    }
}

#[test]
fn check_reports_assessment() {
    let st = setup();
    let out = Command::new(BIN).args(["check", "--target", s(&st.image)]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((v["kind"].as_str(), v["isBoot"].as_bool()), (Some("image"), Some(false)));
}
