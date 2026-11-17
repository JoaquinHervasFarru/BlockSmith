//! Imágenes UDIF (.dmg). Implementación propia a partir del formato público
//! del trailer `koly` y de la tabla de bloques `mish`.
//!
//! F1: detección del trailer. Pendiente: tabla `mish` y descompresión por chunk.

use crate::{BlockDevice, IoResult};

pub const KOLY_SIGNATURE: &[u8; 4] = b"koly";
pub const KOLY_SIZE: u64 = 512;

/// Devuelve `true` si el origen termina en un trailer UDIF.
pub fn has_koly_trailer(dev: &dyn BlockDevice) -> IoResult<bool> {
    if dev.len() < KOLY_SIZE {
        return Ok(false);
    }
    let sig = dev.read_vec(dev.len() - KOLY_SIZE, 4)?;
    Ok(sig == KOLY_SIGNATURE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryDevice;

    #[test]
    fn detects_trailer() {
        let mut data = vec![0u8; 2048];
        data[2048 - 512..2048 - 508].copy_from_slice(b"koly");
        assert!(has_koly_trailer(&MemoryDevice::new(data)).unwrap());
        assert!(!has_koly_trailer(&MemoryDevice::new(vec![0; 2048])).unwrap());
    }
}
