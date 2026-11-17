//! Capa genérica de cifrado. Cada esquema detecta su cabecera, enumera los
//! protectores disponibles y, al desbloquear, devuelve un dispositivo que
//! descifra de forma transparente para los parsers de sistemas de archivos.
//!
//! Las claves solo viven en memoria ([`Secret`] se borra al soltarse) y nunca
//! se registran ni se guardan.

pub mod bitlocker;
pub mod corestorage;
pub mod luks;

use dirforge_io::BlockDevice;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemeKind {
    Bitlocker,
    Luks1,
    Luks2,
    Corestorage,
    Apfs,
    Fscrypt,
    Veracrypt,
    Hardware,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProtectorKind {
    Tpm,
    Password,
    Recovery,
    Keyfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionInfo {
    pub scheme: SchemeKind,
    pub cipher: String,
    pub header_ok: bool,
    pub backup_headers_ok: bool,
    pub protectors: Vec<ProtectorKind>,
}

/// Secreto introducido por el usuario. Se borra de memoria al soltarse y su
/// `Debug` nunca muestra el contenido.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error(transparent)]
    Io(#[from] dirforge_io::IoError),
    #[error("secreto incorrecto")]
    WrongSecret,
    #[error("cabecera de cifrado dañada: {0}")]
    BadHeader(String),
    #[error("no soportado todavía: {0}")]
    Unsupported(&'static str),
}

pub trait EncryptionScheme {
    fn probe(&self, dev: &dyn BlockDevice) -> Result<Option<EncryptionInfo>, CryptoError>;
    fn unlock<'a>(
        &self,
        dev: &'a dyn BlockDevice,
        protector: ProtectorKind,
        secret: &Secret,
    ) -> Result<Box<dyn BlockDevice + 'a>, CryptoError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        assert_eq!(format!("{:?}", Secret::new(*b"hunter2")), "Secret(***)");
    }
}
