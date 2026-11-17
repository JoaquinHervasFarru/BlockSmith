//! Orquestador de trabajos largos (escaneo, reconstrucción, exportación).
//! F3: cola con cancelación y eventos de progreso hacia la UI.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Default)]
pub struct JobQueue {
    cancel: Arc<AtomicBool>,
}

impl JobQueue {
    #[allow(dead_code)]
    pub fn cancel_current(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}
