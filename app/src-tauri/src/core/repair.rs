//! Escritura directa: aplicar una reconstrucción sobre el propio origen (imagen o
//! disco) de forma transaccional, y recuperar transacciones interrumpidas.
//!
//! - Si el usuario puede escribir en el destino, la transacción corre en la app.
//! - Si no, se delega en `blocksmith-helper` con elevación (pkexec / osascript); el
//!   helper repite todas las comprobaciones y verifica el resultado.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use blocksmith_fs_core::{Progress, Severity, WritePhase};
use blocksmith_fs_hfsplus::{scan_volume, Volume};
use blocksmith_helper::{sha256_hex, HelperEvent, Job};
use blocksmith_io::{open_image, Slice};
use blocksmith_txwriter::device::{assess, undo_on_target_device, Assessment};
use blocksmith_txwriter::{apply, inspect, mark_verified, prepare, roll_forward, rollback, Control, FileTarget, Observer, PlannedWrite, TxError, TxMeta, TxState};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::dto::FindingDto;
use super::{finding_dto, now, Core, CoreError, CoreResult};

pub const FAIL_ENV: &str = "BLOCKSMITH_TX_FAIL_AFTER";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightCheck {
    pub id: &'static str,
    pub label: String,
    /// ok | warn | block
    pub status: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightDto {
    pub rebuild_id: i64,
    pub target: String,
    pub volume_name: Option<String>,
    /// in_process | helper | unavailable
    pub mode: &'static str,
    pub checks: Vec<PreflightCheck>,
    pub can_proceed: bool,
    pub undo_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairResultDto {
    pub tx_id: i64,
    pub verified: bool,
    pub remaining_findings: Vec<FindingDto>,
    pub undo_log_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxDto {
    pub id: i64,
    pub rebuild_id: i64,
    pub session_id: i64,
    pub volume_name: Option<String>,
    pub device_path: String,
    pub status: String,
    pub mode: String,
    pub undo_log_path: String,
    pub undo_exists: bool,
    pub blocks_total: i64,
    pub blocks_written: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    pub can_rollback: bool,
    pub can_complete: bool,
}

fn check(id: &'static str, label: &str, status: &'static str, detail: impl Into<String>) -> PreflightCheck {
    PreflightCheck { id, label: label.into(), status, detail: detail.into() }
}

/// Espacio libre (bytes) en la carpeta indicada.
fn free_space(dir: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: `c` es una cadena C válida y `st` se inicializa con ceros.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        Some(st.f_bavail as u64 * st.f_frsize as u64)
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        None
    }
}

#[cfg(unix)]
fn same_filesystem(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    matches!((std::fs::metadata(a), std::fs::metadata(b)), (Ok(x), Ok(y)) if x.dev() == y.dev())
}

#[cfg(not(unix))]
fn same_filesystem(_: &Path, _: &Path) -> bool {
    false
}

/// Cómo se ejecutaría el helper con privilegios en esta plataforma.
fn elevation() -> Option<Vec<String>> {
    match std::env::var("BLOCKSMITH_HELPER_ELEVATION").as_deref() {
        Ok("none") => return Some(vec![]),
        Ok("unavailable") => return None,
        _ => {}
    }
    #[cfg(unix)]
    // SAFETY: geteuid no tiene precondiciones.
    if unsafe { libc::geteuid() } == 0 {
        return Some(vec![]);
    }
    if cfg!(target_os = "linux") {
        let pkexec = ["/usr/bin/pkexec", "/bin/pkexec"].into_iter().find(|p| Path::new(p).exists())?;
        return Some(vec![pkexec.into()]);
    }
    if cfg!(target_os = "macos") {
        return Some(vec!["osascript".into()]);
    }
    None
}

fn helper_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("BLOCKSMITH_HELPER_PATH") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "blocksmith-helper.exe" } else { "blocksmith-helper" };
    let candidates = [exe.parent()?.join(name), PathBuf::from("/usr/local/bin").join(name)];
    candidates.into_iter().find(|p| p.is_file())
}

/// Ejecuta el helper (con elevación si hace falta) y traduce sus eventos al progreso.
fn run_helper(args: &[String], progress: &mut dyn Progress) -> CoreResult<HelperEvent> {
    let helper = helper_path().ok_or_else(|| CoreError::Invalid("no se encontró blocksmith-helper junto a la aplicación".into()))?;
    let elev = elevation().ok_or_else(|| CoreError::Invalid("no hay forma de pedir permisos de administrador en este sistema (instala polkit/pkexec)".into()))?;
    let mut cmd = match elev.first().map(String::as_str) {
        None => Command::new(&helper),
        Some("osascript") => {
            let quoted: Vec<String> = std::iter::once(helper.display().to_string())
                .chain(args.iter().cloned())
                .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
                .collect();
            let script = format!("do shell script \"{}\" with administrator privileges", quoted.join(" ").replace('\\', "\\\\").replace('"', "\\\""));
            let mut c = Command::new("osascript");
            c.args(["-e", &script]);
            c
        }
        Some(launcher) => {
            let mut c = Command::new(launcher);
            c.arg(&helper);
            c
        }
    };
    if elev.first().map(String::as_str) != Some("osascript") {
        cmd.args(args);
    }
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| CoreError::Invalid(format!("no se pudo ejecutar el helper: {e}")))?;
    let stdout = child.stdout.take().expect("stdout capturado");
    let mut last: Option<HelperEvent> = None;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(ev) = serde_json::from_str::<HelperEvent>(&line) else { continue };
        match &ev {
            HelperEvent::Phase { name } => progress.phase(name),
            HelperEvent::Progress { done, total } => progress.advance(*done as u64, *total as u64),
            _ => {}
        }
        if progress.is_cancelled() {
            let _ = child.kill();
        }
        last = Some(ev);
    }
    let status = child.wait()?;
    match last {
        Some(ev) => Ok(ev),
        None if !status.success() => {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut e, &mut err);
            }
            Err(CoreError::Invalid(if err.trim().is_empty() { "el helper terminó sin respuesta (¿se canceló la petición de permisos?)".into() } else { err.trim().to_string() }))
        }
        None => Err(CoreError::Invalid("el helper no informó del resultado".into())),
    }
}

/// Observador de la transacción en proceso: fases, cancelación y fallo simulado (solo en desarrollo).
struct TxObs<'a> {
    progress: &'a mut dyn Progress,
    fail_at: Option<u32>,
    phase: Option<WritePhase>,
}

impl Observer for TxObs<'_> {
    fn before_write(&mut self, seq: u32, phase: WritePhase) -> Control {
        if self.phase != Some(phase) {
            self.phase = Some(phase);
            self.progress.phase(match phase {
                WritePhase::Data => "Escribiendo el directorio nuevo",
                WritePhase::Commit => "Actualizando la cabecera del volumen",
                WritePhase::Finalize => "Liberando los bloques antiguos",
            });
        }
        if self.fail_at == Some(seq) {
            return Control::Crash;
        }
        if self.progress.is_cancelled() { Control::Stop } else { Control::Continue }
    }
    fn progress(&mut self, done: u32, total: u32, _: &str) {
        self.progress.advance(done as u64, total as u64);
    }
}


impl Core {
    pub fn undo_dir(&self) -> CoreResult<PathBuf> {
        let s = self.settings()?;
        Ok(if s.undo_dir.trim().is_empty() { self.data_dir.join("undo") } else { PathBuf::from(s.undo_dir) })
    }

    /// Ruta del volumen físico que contiene la ruta indicada (para la salud del disco).
    fn health_for(&self, device_path: &str) -> CoreResult<Option<(String, String)>> {
        let disks = self.health_disks()?;
        let d = disks
            .iter()
            .filter(|d| d.path.as_deref().is_some_and(|p| device_path.starts_with(p)))
            .max_by_key(|d| d.path.as_ref().map(String::len));
        Ok(d.map(|d| (d.model.clone().unwrap_or_default(), d.latest.as_ref().map(|s| s.overall.clone()).unwrap_or_else(|| "unknown".into()))))
    }

    pub fn preflight(&self, rebuild_id: i64) -> CoreResult<PreflightDto> {
        let r = self.get_rebuild(rebuild_id)?;
        let session = self.get_session(r.session_id)?;
        let loc = self.location(session.volume_id)?;
        let vol = self.get_volume(session.volume_id)?;
        let target = PathBuf::from(&loc.path);
        let a: Assessment = assess(&target);
        let undo_dir = self.undo_dir()?;
        let _ = std::fs::create_dir_all(&undo_dir);
        let mut checks = Vec::new();

        let applied = self.db().conn().query_row(
            "SELECT COUNT(*) FROM write_transactions WHERE rebuild_id = ?1 AND status IN ('finalized','verified')",
            [rebuild_id],
            |row| row.get::<_, i64>(0),
        )?;
        checks.push(if applied > 0 {
            check("plan", "Plan pendiente de aplicar", "block", "Esta reconstrucción ya se aplicó al origen.")
        } else {
            check("plan", "Plan pendiente de aplicar", "ok", format!("{} cambios revisados; se recalculará justo antes de escribir.", r.changes.iter().map(|c| c.count).sum::<i64>()))
        });

        let open_tx = self.db().conn().query_row(
            "SELECT COUNT(*) FROM write_transactions t JOIN rebuilds rb ON rb.id = t.rebuild_id JOIN sessions s ON s.id = rb.session_id
             WHERE s.volume_id = ?1 AND t.status IN ('prepared','writing','committed','interrupted')",
            [session.volume_id],
            |row| row.get::<_, i64>(0),
        )?;
        checks.push(if open_tx > 0 {
            check("pending_tx", "Sin transacciones a medias", "block", "Hay una escritura interrumpida en este volumen: revísala en «Transacciones» antes de continuar.")
        } else {
            check("pending_tx", "Sin transacciones a medias", "ok", "Ninguna escritura pendiente en este volumen.")
        });

        let kind_label = match a.kind {
            Some(blocksmith_txwriter::TargetKind::BlockDevice) => "disco",
            _ => "imagen",
        };
        checks.push(match (a.exists, a.kind) {
            (false, _) => check("target", "Origen disponible", "block", format!("{} ya no existe.", loc.path)),
            (true, None) => check("target", "Origen disponible", "block", "El origen no es una imagen ni un disco."),
            (true, Some(_)) if a.writable => check("target", "Origen disponible", "ok", format!("Se escribirá en la {kind_label} {}.", loc.path)),
            (true, Some(_)) => match elevation() {
                Some(_) if helper_path().is_some() => check("target", "Origen disponible", "warn", "Se pedirán permisos de administrador para escribir."),
                _ => check("target", "Origen disponible", "block", "No hay permisos para escribir y no se puede pedir elevación en este sistema."),
            },
        });
        checks.push(if a.is_boot {
            check("boot", "No es el disco de arranque", "block", "Contiene el sistema en uso. Arranca desde otro disco (medio de arranque) para repararlo.")
        } else {
            check("boot", "No es el disco de arranque", "ok", "El sistema en uso está en otro disco.")
        });
        checks.push(if a.mounted_at.is_empty() {
            check("mounted", "Volumen desmontado", "ok", match &a.loop_device {
                Some(l) => format!("La imagen está conectada a {l}, pero sin montar."),
                None => "No está en uso por el sistema.".into(),
            })
        } else {
            check("mounted", "Volumen desmontado", "block", format!("Montado en {}. Desmóntalo antes de reparar.", a.mounted_at.join(", ")))
        });
        checks.push(if vol.journal_state == "pending" {
            check("journal", "Journal sin transacciones pendientes", "block", "Monta el volumen una vez en macOS para que se apliquen y vuelve a analizarlo.")
        } else {
            check("journal", "Journal sin transacciones pendientes", "ok", if vol.journaled { "Journal limpio." } else { "El volumen no usa journal." })
        });
        checks.push(match (a.kind, self.health_for(&loc.path)?) {
            (Some(blocksmith_txwriter::TargetKind::BlockDevice), Some((model, overall))) => match overall.as_str() {
                "failing" => check("health", "Salud del disco", "block", format!("{model} predice un fallo: copia o clona los datos antes de cualquier reparación.")),
                "warning" => check("health", "Salud del disco", "warn", format!("{model} muestra signos de deterioro; ten una copia antes de escribir.")),
                "passed" => check("health", "Salud del disco", "ok", format!("{model} sin indicios de deterioro.")),
                _ => check("health", "Salud del disco", "warn", "No se pudo leer el estado S.M.A.R.T. del disco."),
            },
            (Some(blocksmith_txwriter::TargetKind::BlockDevice), None) => check("health", "Salud del disco", "warn", "No hay lecturas de salud de este disco; léelas en «Salud de los discos»."),
            _ => check("health", "Salud del disco", "ok", "No aplica a imágenes."),
        });
        let needed = 2 * r.bytes_to_write.max(0) as u64 + 16 * 1024 * 1024;
        let free = free_space(&undo_dir);
        checks.push(if undo_on_target_device(&undo_dir, &target) {
            check("undo", "Copia de deshacer en otro disco", "block", "La carpeta de deshacer está en el mismo disco que se va a reparar; elige otra en Ajustes.")
        } else if free.is_some_and(|f| f < needed) {
            check("undo", "Copia de deshacer en otro disco", "block", format!("No hay espacio suficiente en {} para la copia de deshacer.", undo_dir.display()))
        } else if a.kind == Some(blocksmith_txwriter::TargetKind::Image) && same_filesystem(&undo_dir, &target) {
            check("undo", "Copia de deshacer en otro disco", "warn", format!("La copia se guarda en {}, en el mismo disco que la imagen: protege ante cortes, pero no ante un fallo de ese disco.", undo_dir.display()))
        } else {
            check("undo", "Copia de deshacer en otro disco", "ok", format!("Se guardará en {}.", undo_dir.display()))
        });

        let force_helper = *self.force_helper.lock().expect("modo helper");
        let mode = if !a.exists || a.kind.is_none() {
            "unavailable"
        } else if a.writable && !force_helper {
            "in_process"
        } else if elevation().is_some() && helper_path().is_some() {
            "helper"
        } else {
            "unavailable"
        };
        let can_proceed = mode != "unavailable" && checks.iter().all(|c| c.status != "block");
        Ok(PreflightDto {
            rebuild_id,
            target: loc.path,
            volume_name: session.volume_name,
            mode,
            checks,
            can_proceed,
            undo_dir: undo_dir.display().to_string(),
        })
    }

    fn set_tx(&self, tx_id: i64, status: &str, written: Option<u32>, error: Option<&str>, finished: bool) -> CoreResult<()> {
        self.db().conn().execute(
            "UPDATE write_transactions SET status = ?1, blocks_written = COALESCE(?2, blocks_written), error = ?3,
                finished_at = CASE WHEN ?4 THEN ?5 ELSE finished_at END WHERE id = ?6",
            params![status, written.map(i64::from), error, finished, now(), tx_id],
        )?;
        Ok(())
    }

    /// Verifica el volumen tras escribir: análisis completo del origen.
    fn verify_source(&self, volume_id: i64, session_id: i64, progress: &mut dyn Progress) -> CoreResult<Vec<FindingDto>> {
        progress.phase("Verificando el volumen");
        let loc = self.location(volume_id)?;
        let dev = open_image(&loc.path)?;
        let slice = Slice::new(dev.as_ref(), loc.offset, loc.length)?;
        let vol = Volume::open(&slice)?.ok_or_else(|| CoreError::Invalid("el origen ya no contiene un volumen HFS+".into()))?;
        let scan = scan_volume(&vol, progress)?;
        Ok(scan.result.findings.iter().filter(|f| f.severity != Severity::Cosmetic).enumerate().map(|(i, f)| finding_dto(i as i64, session_id, f)).collect())
    }

    fn mark_repaired(&self, rebuild_id: i64, session_id: i64, path: &str, verified: bool) -> CoreResult<()> {
        let c = self.db().conn();
        c.execute(
            "UPDATE rebuilds SET status = 'done', target = 'in_place', finished_at = ?1, output_path = ?2, verified = ?3 WHERE id = ?4",
            params![now(), path, verified, rebuild_id],
        )?;
        c.execute("UPDATE sessions SET status = 'exported' WHERE id = ?1", [session_id])?;
        if verified {
            c.execute("UPDATE findings SET fixed_in_rebuild = 1 WHERE session_id = ?1", [session_id])?;
        }
        Ok(())
    }

    /// Aplica la reconstrucción sobre el propio origen.
    pub fn repair_in_place(&self, rebuild_id: i64, confirm: &str, progress: &mut dyn Progress) -> CoreResult<RepairResultDto> {
        progress.phase("Comprobaciones previas");
        let pre = self.preflight(rebuild_id)?;
        let expected = pre.volume_name.clone().unwrap_or_default();
        if confirm.trim() != expected.trim() {
            return Err(CoreError::Invalid(format!("para confirmar escribe exactamente el nombre del volumen: «{expected}»")));
        }
        if !pre.can_proceed {
            let why: Vec<String> = pre.checks.iter().filter(|c| c.status == "block").map(|c| c.detail.clone()).collect();
            return Err(CoreError::Invalid(format!("no se puede escribir: {}", why.join(" "))));
        }
        let r = self.get_rebuild(rebuild_id)?;
        let session = self.get_session(r.session_id)?;
        let loc = self.location(session.volume_id)?;

        // Siempre se recalcula sobre el contenido actual; debe coincidir con lo revisado.
        progress.phase("Recalculando el plan sobre el contenido actual");
        let plan = self.compute_plan(session.volume_id, progress)?;
        if plan.diff.len() != self.diff(rebuild_id)?.len() {
            return Err(CoreError::Invalid("el volumen cambió desde que revisaste el plan; vuelve a analizarlo".into()));
        }
        let undo_dir = self.undo_dir()?;
        std::fs::create_dir_all(&undo_dir)?;
        let target_len = open_image(&loc.path)?.len();
        let job = Job::from_plan(&plan, &loc.path, target_len, loc.offset, loc.length, &expected);
        let mode = pre.mode;
        self.db().conn().execute(
            "INSERT INTO write_transactions (rebuild_id, device_path, status, undo_log_path, blocks_total, started_at, helper_version, mode)
             VALUES (?1, ?2, 'prepared', '', ?3, ?4, ?5, ?6)",
            params![rebuild_id, loc.path, job.writes.len() as i64, now(), env!("CARGO_PKG_VERSION"), mode],
        )?;
        let tx_id = self.db().conn().last_insert_rowid();
        // El id de la transacción hace único el nombre aunque se reintente en el mismo segundo.
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let undo = undo_dir.join(format!("blocksmith-tx{tx_id}-r{rebuild_id}-{stamp}.undo"));
        self.db().conn().execute("UPDATE write_transactions SET undo_log_path = ?1 WHERE id = ?2", params![undo.display().to_string(), tx_id])?;
        let undo_str = undo.display().to_string();

        let verified;
        let remaining;
        if mode == "in_process" {
            progress.phase("Guardando la copia de deshacer");
            let writes = job.decode().map_err(CoreError::Invalid)?;
            let mut dest = FileTarget::open(Path::new(&loc.path))?;
            let planned = writes.into_iter().map(|(o, phase, label, data)| PlannedWrite { offset: loc.offset + o, data, phase, label }).collect();
            let meta = TxMeta {
                target: loc.path.clone(),
                target_len,
                base_offset: loc.offset,
                volume_len: loc.length,
                created_at: now(),
                plan_sha256: job.plan_sha256.clone(),
                label: expected.clone(),
                first_commit: None,
            };
            let mut tx = prepare(&dest, planned, &undo, meta)?;
            self.set_tx(tx_id, "writing", Some(0), None, false)?;
            let fail_at = *self.tx_fail_after.lock().expect("fallo simulado");
            let outcome = apply(&mut tx, &mut dest, &mut TxObs { progress, fail_at, phase: None });
            match outcome {
                Ok(()) => {}
                Err(TxError::Cancelled) => {
                    progress.phase("Revirtiendo");
                    rollback(&undo, &mut dest, false)?;
                    self.set_tx(tx_id, "rolled_back", Some(0), Some("cancelada por el usuario antes de modificar la cabecera"), true)?;
                    return Err(CoreError::Cancelled);
                }
                Err(e) => {
                    let written = inspect(&undo).map(|i| i.written).ok();
                    self.set_tx(tx_id, "interrupted", written, Some(&e.to_string()), false)?;
                    return Err(e.into());
                }
            }
            drop(dest);
            self.set_tx(tx_id, "finalized", Some(tx.total()), None, false)?;
            remaining = self.verify_source(session.volume_id, r.session_id, progress)?;
            verified = remaining.is_empty();
            if verified {
                mark_verified(&undo)?;
            }
        } else {
            let job_dir = self.data_dir.join("jobs");
            std::fs::create_dir_all(&job_dir)?;
            let job_path = job_dir.join(format!("job-{tx_id}.json"));
            let raw = serde_json::to_vec(&job).map_err(|e| CoreError::Invalid(e.to_string()))?;
            std::fs::write(&job_path, &raw)?;
            self.set_tx(tx_id, "writing", Some(0), None, false)?;
            let args = vec![
                "apply".into(), "--target".into(), loc.path.clone(), "--job".into(), job_path.display().to_string(),
                "--job-sha256".into(), sha256_hex(&raw), "--undo".into(), undo_str.clone(),
            ];
            let ev = run_helper(&args, progress);
            let _ = std::fs::remove_file(&job_path);
            match ev? {
                HelperEvent::Done { verified: v, findings, .. } => {
                    verified = v;
                    remaining = findings
                        .into_iter()
                        .enumerate()
                        .map(|(i, code)| FindingDto {
                            id: i as i64, session_id: r.session_id, code: code.clone(), severity: "minor".into(), object_type: "header".into(),
                            cnid: None, node_number: None, path: None, message: code, fixed_in_rebuild: false,
                        })
                        .collect();
                }
                HelperEvent::Cancelled { .. } => {
                    self.set_tx(tx_id, "rolled_back", Some(0), Some("cancelada por el usuario antes de modificar la cabecera"), true)?;
                    return Err(CoreError::Cancelled);
                }
                HelperEvent::Error { message, interrupted } => {
                    let status = if interrupted || undo.exists() { "interrupted" } else { "failed" };
                    let written = inspect(&undo).map(|i| i.written).ok();
                    self.set_tx(tx_id, status, written, Some(&message), status == "failed")?;
                    return Err(CoreError::Invalid(message));
                }
                other => return Err(CoreError::Invalid(format!("respuesta inesperada del helper: {other:?}"))),
            }
        }
        self.set_tx(tx_id, if verified { "verified" } else { "finalized" }, None, None, true)?;
        self.mark_repaired(rebuild_id, r.session_id, &loc.path, verified)?;
        self.plans.lock().expect("caché de planes").clear();
        Ok(RepairResultDto { tx_id, verified, remaining_findings: remaining, undo_log_path: undo_str })
    }

    pub fn transactions(&self) -> CoreResult<Vec<TxDto>> {
        let conn = self.db().conn();
        let mut st = conn.prepare(
            "SELECT t.id, t.rebuild_id, rb.session_id, v.name, t.device_path, t.status, t.mode, t.undo_log_path, t.blocks_total,
                    t.blocks_written, t.started_at, t.finished_at, t.error
             FROM write_transactions t JOIN rebuilds rb ON rb.id = t.rebuild_id JOIN sessions s ON s.id = rb.session_id
             JOIN volumes v ON v.id = s.volume_id ORDER BY t.id DESC",
        )?;
        let rows = st
            .query_map([], |r| {
                let status: String = r.get(5)?;
                let undo: String = r.get(7)?;
                let exists = Path::new(&undo).exists();
                Ok(TxDto {
                    id: r.get(0)?,
                    rebuild_id: r.get(1)?,
                    session_id: r.get(2)?,
                    volume_name: r.get(3)?,
                    device_path: r.get(4)?,
                    can_rollback: exists && matches!(status.as_str(), "interrupted" | "finalized" | "verified" | "writing" | "committed" | "prepared"),
                    can_complete: exists && status == "interrupted",
                    status,
                    mode: r.get(6)?,
                    undo_log_path: undo,
                    undo_exists: exists,
                    blocks_total: r.get(8)?,
                    blocks_written: r.get(9)?,
                    started_at: r.get(10)?,
                    finished_at: r.get(11)?,
                    error: r.get(12)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Al arrancar: las transacciones que quedaron a medias se marcan como interrumpidas
    /// (o se sincronizan con su registro si en realidad terminaron).
    pub fn detect_interrupted(&self) -> CoreResult<usize> {
        let pending: Vec<(i64, String)> = {
            let conn = self.db().conn();
            let mut st = conn.prepare("SELECT id, undo_log_path FROM write_transactions WHERE status IN ('prepared','writing','committed')")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
            rows
        };
        let mut n = 0;
        for (id, undo) in pending {
            match inspect(Path::new(&undo)) {
                Ok(info) if info.state == TxState::Verified => self.set_tx(id, "verified", Some(info.written), None, true)?,
                Ok(info) if info.state == TxState::Finalized => self.set_tx(id, "finalized", Some(info.written), None, true)?,
                Ok(info) if info.state == TxState::RolledBack => self.set_tx(id, "rolled_back", Some(0), None, true)?,
                Ok(info) => {
                    n += 1;
                    self.set_tx(id, "interrupted", Some(info.written), Some("la aplicación se cerró durante la escritura"), false)?
                }
                Err(e) => {
                    n += 1;
                    self.set_tx(id, "interrupted", None, Some(&format!("registro de deshacer ilegible: {e}")), false)?
                }
            }
        }
        Ok(n)
    }

    fn tx_row(&self, tx_id: i64) -> CoreResult<TxDto> {
        self.transactions()?.into_iter().find(|t| t.id == tx_id).ok_or_else(|| CoreError::NotFound(format!("transacción {tx_id} no encontrada")))
    }

    /// Revierte una transacción (interrumpida o completada) con su registro de deshacer.
    pub fn rollback_tx(&self, tx_id: i64, force: bool, progress: &mut dyn Progress) -> CoreResult<TxDto> {
        let t = self.tx_row(tx_id)?;
        if !t.can_rollback {
            return Err(CoreError::Invalid("esta transacción no se puede revertir".into()));
        }
        progress.phase("Revirtiendo la transacción");
        let target = Path::new(&t.device_path);
        let in_process = assess(target).writable && !*self.force_helper.lock().expect("modo helper");
        if in_process {
            if !assess(target).mounted_at.is_empty() {
                return Err(CoreError::Invalid("el volumen está montado; desmóntalo antes de revertir".into()));
            }
            let mut dest = FileTarget::open(target)?;
            rollback(Path::new(&t.undo_log_path), &mut dest, force)?;
        } else {
            let mut args = vec!["rollback".into(), "--target".into(), t.device_path.clone(), "--undo".into(), t.undo_log_path.clone()];
            if force {
                args.push("--force".into());
            }
            if let HelperEvent::Error { message, .. } = run_helper(&args, progress)? {
                return Err(CoreError::Invalid(message));
            }
        }
        self.set_tx(tx_id, "rolled_back", Some(0), None, true)?;
        // El origen vuelve a su estado anterior: la reconstrucción queda pendiente otra vez.
        self.db().conn().execute("UPDATE rebuilds SET status = 'planned', target = 'new_image', verified = 0, output_path = NULL WHERE id = ?1 AND target = 'in_place'", [t.rebuild_id])?;
        self.db().conn().execute("UPDATE findings SET fixed_in_rebuild = 0 WHERE session_id = ?1", [t.session_id])?;
        self.db().conn().execute("UPDATE sessions SET status = 'rebuilt' WHERE id = ?1", [t.session_id])?;
        self.plans.lock().expect("caché de planes").clear();
        self.tx_row(tx_id)
    }

    /// Completa una transacción interrumpida y verifica el volumen.
    pub fn complete_tx(&self, tx_id: i64, progress: &mut dyn Progress) -> CoreResult<TxDto> {
        let t = self.tx_row(tx_id)?;
        if !t.can_complete {
            return Err(CoreError::Invalid("esta transacción no está interrumpida".into()));
        }
        progress.phase("Completando la transacción");
        let target = Path::new(&t.device_path);
        let in_process = assess(target).writable && !*self.force_helper.lock().expect("modo helper");
        let verified = if in_process {
            let mut dest = FileTarget::open(target)?;
            roll_forward(Path::new(&t.undo_log_path), &mut dest)?;
            drop(dest);
            let session = self.get_session(t.session_id)?;
            let ok = self.verify_source(session.volume_id, t.session_id, progress)?.is_empty();
            if ok {
                mark_verified(Path::new(&t.undo_log_path))?;
            }
            ok
        } else {
            let args = vec!["roll-forward".into(), "--target".into(), t.device_path.clone(), "--undo".into(), t.undo_log_path.clone()];
            match run_helper(&args, progress)? {
                HelperEvent::Done { verified, .. } => verified,
                HelperEvent::Error { message, .. } => return Err(CoreError::Invalid(message)),
                other => return Err(CoreError::Invalid(format!("respuesta inesperada del helper: {other:?}"))),
            }
        };
        self.set_tx(tx_id, if verified { "verified" } else { "finalized" }, Some(t.blocks_total as u32), None, true)?;
        self.mark_repaired(t.rebuild_id, t.session_id, &t.device_path, verified)?;
        self.tx_row(tx_id)
    }

    /// ¿Hay un registro de transacción para esta fila? (usado por las pruebas)
    pub fn tx_undo_path(&self, tx_id: i64) -> CoreResult<Option<String>> {
        Ok(self.db().conn().query_row("SELECT undo_log_path FROM write_transactions WHERE id = ?1", [tx_id], |r| r.get(0)).optional()?)
    }
}
