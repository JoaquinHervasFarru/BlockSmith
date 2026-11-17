//! Trabajos largos (análisis, plan, exportación) en hilos propios, con eventos de
//! progreso hacia la interfaz y cancelación cooperativa.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use blocksmith_fs_core::Progress;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::core::CoreError;

pub const EVENT_PROGRESS: &str = "job-progress";
pub const EVENT_FINISHED: &str = "job-finished";
const EMIT_EVERY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub job_id: u64,
    pub kind: &'static str,
    pub phase: String,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedEvent {
    pub job_id: u64,
    pub kind: &'static str,
    pub ok: bool,
    pub cancelled: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct JobManager {
    next: AtomicU64,
    cancels: Mutex<HashMap<u64, Arc<AtomicBool>>>,
}

pub struct JobProgress {
    app: AppHandle,
    job_id: u64,
    kind: &'static str,
    cancel: Arc<AtomicBool>,
    phase: String,
    last_emit: Instant,
}

impl JobProgress {
    fn emit(&mut self, done: u64, total: u64) {
        self.last_emit = Instant::now();
        let _ = self.app.emit(
            EVENT_PROGRESS,
            ProgressEvent { job_id: self.job_id, kind: self.kind, phase: self.phase.clone(), done, total },
        );
    }
}

impl Progress for JobProgress {
    fn phase(&mut self, name: &str) {
        self.phase = name.to_string();
        self.emit(0, 0);
    }
    fn advance(&mut self, done: u64, total: u64) {
        if self.last_emit.elapsed() >= EMIT_EVERY || done >= total {
            self.emit(done, total);
        }
    }
    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl JobManager {
    /// Lanza `work` en un hilo y devuelve el id del trabajo. El resultado llega
    /// por el evento `job-finished`.
    pub fn spawn<F>(&self, app: AppHandle, kind: &'static str, work: F) -> u64
    where
        F: FnOnce(&mut JobProgress) -> Result<serde_json::Value, CoreError> + Send + 'static,
    {
        let job_id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancels.lock().expect("trabajos").insert(job_id, cancel.clone());
        let mut progress =
            JobProgress { app: app.clone(), job_id, kind, cancel, phase: String::new(), last_emit: Instant::now() };
        std::thread::Builder::new()
            .name(format!("job-{kind}-{job_id}"))
            .spawn(move || {
                let outcome = work(&mut progress);
                let cancelled = progress.is_cancelled()
                    || matches!(&outcome, Err(CoreError::Cancelled) | Err(CoreError::Fs(blocksmith_fs_core::FsError::Cancelled)));
                let event = match outcome {
                    Ok(result) => FinishedEvent { job_id, kind, ok: true, cancelled: false, result: Some(result), error: None },
                    Err(e) => FinishedEvent { job_id, kind, ok: false, cancelled, result: None, error: Some(e.to_string()) },
                };
                let _ = app.emit(EVENT_FINISHED, event);
            })
            .expect("no se pudo crear el hilo del trabajo");
        job_id
    }

    pub fn cancel(&self, job_id: u64) -> bool {
        match self.cancels.lock().expect("trabajos").get(&job_id) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }
}
