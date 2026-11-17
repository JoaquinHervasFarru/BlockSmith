//! Acceso a bloques para DirForge.
//!
//! Todo el motor lee a través del trait [`BlockDevice`]. Las implementaciones
//! de este crate son **solo lectura**; la escritura vive en `txwriter` y solo
//! la ejecuta `dirforge-helper` con privilegios.

mod device;
mod error;
mod memory;
mod raw;
pub mod udif;

pub use device::DeviceInfo;
pub use error::{IoError, IoResult};
pub use memory::MemoryDevice;
pub use raw::RawImage;

/// Origen de bloques direccionable por offset de byte.
pub trait BlockDevice: Send + Sync {
    /// Tamaño total en bytes.
    fn len(&self) -> u64;

    /// Tamaño de sector lógico (512 o 4096 normalmente).
    fn sector_size(&self) -> u32 {
        512
    }

    /// Lee exactamente `buf.len()` bytes desde `offset`.
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> IoResult<()>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lee `len` bytes en un `Vec` nuevo.
    fn read_vec(&self, offset: u64, len: usize) -> IoResult<Vec<u8>> {
        let mut v = vec![0u8; len];
        self.read_exact_at(offset, &mut v)?;
        Ok(v)
    }
}

/// Vista de un rango de otro dispositivo (p. ej. una partición).
pub struct Slice<'a> {
    inner: &'a dyn BlockDevice,
    start: u64,
    len: u64,
}

impl<'a> Slice<'a> {
    pub fn new(inner: &'a dyn BlockDevice, start: u64, len: u64) -> IoResult<Self> {
        let end = start.checked_add(len).ok_or(IoError::OutOfBounds { offset: start, len })?;
        if end > inner.len() {
            return Err(IoError::OutOfBounds { offset: start, len });
        }
        Ok(Self { inner, start, len })
    }
}

impl BlockDevice for Slice<'_> {
    fn len(&self) -> u64 {
        self.len
    }
    fn sector_size(&self) -> u32 {
        self.inner.sector_size()
    }
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> IoResult<()> {
        check_bounds(self.len, offset, buf.len())?;
        self.inner.read_exact_at(self.start + offset, buf)
    }
}

pub(crate) fn check_bounds(total: u64, offset: u64, len: usize) -> IoResult<()> {
    match offset.checked_add(len as u64) {
        Some(end) if end <= total => Ok(()),
        _ => Err(IoError::OutOfBounds { offset, len: len as u64 }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_reads_are_relative_and_bounded() {
        let dev = MemoryDevice::new((0u8..=255).collect());
        let s = Slice::new(&dev, 16, 32).unwrap();
        assert_eq!(s.read_vec(0, 4).unwrap(), vec![16, 17, 18, 19]);
        assert!(s.read_vec(30, 4).is_err());
        assert!(Slice::new(&dev, 250, 10).is_err());
    }
}
