//! blocksmith-helper: aplica una reconstrucción directamente sobre una imagen o un
//! dispositivo, con registro de deshacer, y recupera transacciones interrumpidas.
//!
//! Lo lanza la app con privilegios (pkexec / osascript) cuando el destino no es
//! escribible por el usuario. Repite todas las comprobaciones de seguridad por su
//! cuenta: no confía en la app.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{bail, Context};
use blocksmith_fs_core::{Progress, Severity, WritePhase};
use blocksmith_fs_hfsplus::{scan_volume, Volume};
use blocksmith_helper::{sha256_hex, HelperEvent, Job};
use blocksmith_io::{open_image, Slice};
use blocksmith_txwriter::device::{assess, undo_on_target_device};
use blocksmith_txwriter::{apply, inspect, mark_verified, prepare, roll_forward, rollback, Control, FileTarget, Observer, PlannedWrite, TxError, TxMeta};
use clap::{Parser, Subcommand};

static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Parser)]
#[command(name = "blocksmith-helper", version, about = "Escritura directa transaccional de BlockSmith")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Aplica un trabajo preparado por la app.
    Apply {
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        job: PathBuf,
        /// SHA-256 del archivo de trabajo, calculado por la app.
        #[arg(long)]
        job_sha256: String,
        #[arg(long)]
        undo: PathBuf,
    },
    /// Revierte una transacción con su registro de deshacer.
    Rollback {
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        undo: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Completa una transacción interrumpida.
    RollForward {
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        undo: PathBuf,
    },
    /// Muestra el estado de un registro de deshacer (JSON).
    Inspect {
        #[arg(long)]
        undo: PathBuf,
    },
    /// Evaluación de seguridad de un destino (JSON).
    Check {
        #[arg(long)]
        target: PathBuf,
    },
}

fn emit(e: &HelperEvent) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", serde_json::to_string(e).unwrap_or_default());
    let _ = out.flush();
}

#[cfg(unix)]
fn install_signal_handlers() {
    extern "C" fn on_signal(_: libc::c_int) {
        STOP.store(true, Ordering::SeqCst);
    }
    // SAFETY: el manejador solo escribe en un atómico.
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
}

#[cfg(not(unix))]
fn install_signal_handlers() {}

/// Observador: progreso, cancelación por señal y, solo en compilaciones de
/// desarrollo, un fallo simulado (`BLOCKSMITH_TX_FAIL_AFTER=N`) para probar la recuperación.
struct Obs {
    fail_at: Option<u32>,
}

impl Observer for Obs {
    fn before_write(&mut self, seq: u32, _: WritePhase) -> Control {
        if self.fail_at == Some(seq) {
            return Control::Crash;
        }
        if STOP.load(Ordering::SeqCst) { Control::Stop } else { Control::Continue }
    }
    fn progress(&mut self, done: u32, total: u32, _: &str) {
        emit(&HelperEvent::Progress { done, total });
    }
}

fn fail_injection() -> Option<u32> {
    if cfg!(debug_assertions) {
        std::env::var("BLOCKSMITH_TX_FAIL_AFTER").ok().and_then(|v| v.parse().ok())
    } else {
        None
    }
}

struct Quiet;
impl Progress for Quiet {
    fn phase(&mut self, _: &str) {}
    fn advance(&mut self, _: u64, _: u64) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Comprobaciones que el helper exige siempre.
fn safety(target: &Path, undo: &Path) -> anyhow::Result<()> {
    let a = assess(target);
    if a.kind.is_none() {
        bail!("{} no es una imagen ni un dispositivo de bloque", target.display());
    }
    if a.is_boot {
        bail!("{} contiene el sistema en uso; arranca desde otro disco para repararlo", target.display());
    }
    if !a.mounted_at.is_empty() {
        bail!("{} está montado en {}; desmóntalo antes de reparar", target.display(), a.mounted_at.join(", "));
    }
    let undo_dir = undo.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if undo_on_target_device(undo_dir, target) {
        bail!("el registro de deshacer debe guardarse en otro disco");
    }
    Ok(())
}

/// Analiza el volumen resultante: (sano, códigos de lo que quede).
fn verify_volume(target: &Path, base: u64, len: u64) -> anyhow::Result<(bool, Vec<String>)> {
    let dev = open_image(target)?;
    let slice = Slice::new(dev.as_ref(), base, len)?;
    let vol = Volume::open(&slice)?.context("el destino ya no contiene un volumen HFS+")?;
    let scan = scan_volume(&vol, &mut Quiet)?;
    let codes: Vec<String> = scan.result.findings.iter().filter(|f| f.severity != Severity::Cosmetic).map(|f| f.code.clone()).collect();
    Ok((codes.is_empty(), codes))
}

fn run_apply(target: &Path, job_path: &Path, job_sha: &str, undo: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read(job_path).with_context(|| format!("no se pudo leer el trabajo {}", job_path.display()))?;
    if sha256_hex(&raw) != job_sha.to_ascii_lowercase() {
        bail!("el archivo de trabajo no coincide con el aprobado (hash distinto)");
    }
    let job: Job = serde_json::from_slice(&raw).context("trabajo no válido")?;
    if Path::new(&job.target) != target {
        bail!("el trabajo es para {} y no para {}", job.target, target.display());
    }
    let writes = job.decode().map_err(anyhow::Error::msg)?;
    safety(target, undo)?;

    emit(&HelperEvent::Phase { name: "Guardando la copia de deshacer".into() });
    let mut dest = FileTarget::open(target).with_context(|| format!("no se pudo abrir {} para escribir", target.display()))?;
    let planned = writes
        .into_iter()
        .map(|(offset, phase, label, data)| PlannedWrite { offset: job.base_offset + offset, data, phase, label })
        .collect();
    let meta = TxMeta {
        target: job.target.clone(),
        target_len: job.target_len,
        base_offset: job.base_offset,
        volume_len: job.volume_len,
        created_at: String::new(),
        plan_sha256: job.plan_sha256.clone(),
        label: job.label.clone(),
        first_commit: None,
    };
    let mut tx = prepare(&dest, planned, undo, meta)?;
    emit(&HelperEvent::Phase { name: "Escribiendo el directorio nuevo".into() });
    match apply(&mut tx, &mut dest, &mut Obs { fail_at: fail_injection() }) {
        Ok(()) => {}
        Err(TxError::Cancelled) => {
            emit(&HelperEvent::Phase { name: "Revirtiendo".into() });
            rollback(undo, &mut dest, false)?;
            emit(&HelperEvent::Cancelled { rolled_back: true });
            return Ok(());
        }
        Err(e) => {
            let interrupted = matches!(e, TxError::Interrupted { .. } | TxError::VerifyFailed { .. } | TxError::Io(_));
            emit(&HelperEvent::Error { message: e.to_string(), interrupted });
            std::process::exit(3);
        }
    }
    drop(dest);
    emit(&HelperEvent::Phase { name: "Verificando el volumen".into() });
    let (ok, findings) = verify_volume(target, job.base_offset, job.volume_len)?;
    if ok {
        mark_verified(undo)?;
    }
    emit(&HelperEvent::Done { state: if ok { "verified" } else { "finalized" }.into(), verified: ok, findings });
    Ok(())
}

fn main() {
    install_signal_handlers();
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Apply { target, job, job_sha256, undo } => run_apply(&target, &job, &job_sha256, &undo),
        Cmd::Rollback { target, undo, force } => (|| {
            let a = assess(&target);
            if !a.mounted_at.is_empty() {
                bail!("{} está montado; desmóntalo antes de revertir", target.display());
            }
            let mut dest = FileTarget::open(&target)?;
            rollback(&undo, &mut dest, force)?;
            emit(&HelperEvent::Done { state: "rolled_back".into(), verified: true, findings: vec![] });
            Ok(())
        })(),
        Cmd::RollForward { target, undo } => (|| {
            let a = assess(&target);
            if !a.mounted_at.is_empty() {
                bail!("{} está montado; desmóntalo antes de completar", target.display());
            }
            let info = inspect(&undo)?;
            let mut dest = FileTarget::open(&target)?;
            roll_forward(&undo, &mut dest)?;
            drop(dest);
            let (ok, findings) = verify_volume(&target, info.meta.base_offset, info.meta.volume_len)?;
            if ok {
                mark_verified(&undo)?;
            }
            emit(&HelperEvent::Done { state: if ok { "verified" } else { "finalized" }.into(), verified: ok, findings });
            Ok(())
        })(),
        Cmd::Inspect { undo } => inspect(&undo).map(|i| println!("{}", serde_json::to_string(&i).unwrap_or_default())).map_err(Into::into),
        Cmd::Check { target } => {
            println!("{}", serde_json::to_string(&assess(&target)).unwrap_or_default());
            Ok(())
        }
    };
    if let Err(e) = result {
        emit(&HelperEvent::Error { message: format!("{e:#}"), interrupted: false });
        std::process::exit(2);
    }
}
