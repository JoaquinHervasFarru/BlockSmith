use thiserror::Error;

#[derive(Debug, Error)]
pub enum IoError {
    #[error("lectura fuera de rango: offset {offset}, longitud {len}")]
    OutOfBounds { offset: u64, len: u64 },
    #[error("formato de imagen no reconocido: {0}")]
    UnknownFormat(String),
    #[error("imagen dañada: {0}")]
    Corrupt(String),
    #[error("no soportado todavía: {0}")]
    Unsupported(&'static str),
    #[error(transparent)]
    Os(#[from] std::io::Error),
}

pub type IoResult<T> = Result<T, IoError>;
