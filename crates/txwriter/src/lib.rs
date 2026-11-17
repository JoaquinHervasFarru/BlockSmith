//! Escritura transaccional con registro de deshacer.
//!
//! 1. **Preparar**: se leen los bytes originales de cada rango que se va a escribir y
//!    se guardan, junto a los nuevos y sus hashes, en el registro de deshacer, que se
//!    sincroniza antes de tocar el destino.
//! 2. **Aplicar**: escrituras en orden de fase (datos → commit → finalización); tras
//!    cada una, sincronización, relectura y comprobación del hash; el progreso se
//!    anota en la cabecera del registro.
//! 3. **Recuperar**: con una transacción interrumpida se puede **revertir** (bytes
//!    originales en orden inverso) o **completar** (aplicar de nuevo los bytes nuevos).
//!    Antes de revertir se comprueba que nadie más ha modificado los rangos.
//!
//! Gracias al orden de fases, un corte antes del commit deja la versión vieja y uno
//! posterior la nueva; el registro permite además volver exactamente al original.

mod target;
mod tx;
mod undo;

pub use target::{MemTarget, Target};
pub use tx::{apply, prepare, Control, Observer, PlannedWrite, Transaction};
pub use undo::{inspect, mark_verified, roll_forward, rollback, TxMeta, TxState, UndoInfo, UndoRecordInfo};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TxError {
    #[error("el registro de deshacer ya existe: {0}")]
    UndoExists(String),
    #[error("registro de deshacer no válido: {0}")]
    BadUndo(String),
    #[error("la escritura «{label}» (offset {offset}, {len} bytes) se sale del volumen o del destino")]
    OutOfRange { label: String, offset: u64, len: usize },
    #[error("el destino no coincide con el de la transacción: {0}")]
    WrongTarget(String),
    #[error("verificación fallida en el bloque {seq} (offset {offset}): lo leído no coincide con lo escrito")]
    VerifyFailed { seq: u32, offset: u64 },
    #[error("los bloques de la transacción fueron modificados por otro programa (registro {seq}); usa «forzar» solo si sabes lo que haces")]
    ForeignChange { seq: u32 },
    #[error("transacción interrumpida tras {written} de {total} escrituras")]
    Interrupted { written: u32, total: u32 },
    #[error("cancelada antes de modificar la cabecera del volumen")]
    Cancelled,
    #[error("estado no válido para esta operación: {0:?}")]
    BadState(TxState),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type TxResult<T> = Result<T, TxError>;
