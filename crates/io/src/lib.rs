//! Acceso a bloques para BlockSmith.
//!
//! Todo el motor lee a través del trait [`BlockDevice`]. Las implementaciones
//! de este crate son **solo lectura**; la escritura vive en `txwriter` y solo
//! la ejecuta `blocksmith-helper` con privilegios.

mod device;
pub mod endian;
mod error;
mod memory;
mod raw;
pub mod udif;

pub use device::DeviceInfo;
pub use error::{IoError, IoResult};
pub use memory::MemoryDevice;
pub use raw::RawImage;
pub use udif::UdifImage;

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

impl<T: BlockDevice + ?Sized> BlockDevice for Box<T> {
    fn len(&self) -> u64 {
        (**self).len()
    }
    fn sector_size(&self) -> u32 {
        (**self).sector_size()
    }
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> IoResult<()> {
        (**self).read_exact_at(offset, buf)
    }
}

/// Abre un archivo de imagen detectando el formato (UDIF o raw).
pub fn open_image(path: impl AsRef<std::path::Path>) -> IoResult<Box<dyn BlockDevice>> {
    let raw = RawImage::open(path)?;
    if udif::has_koly_trailer(&raw)? {
        Ok(Box::new(UdifImage::open(Box::new(raw))?))
    } else {
        Ok(Box::new(raw))
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
