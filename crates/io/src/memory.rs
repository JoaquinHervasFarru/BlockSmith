use crate::{check_bounds, BlockDevice, IoResult};

/// Dispositivo en memoria, útil para pruebas y fixtures pequeñas.
pub struct MemoryDevice {
    data: Vec<u8>,
}

impl MemoryDevice {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl BlockDevice for MemoryDevice {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> IoResult<()> {
        check_bounds(self.len(), offset, buf.len())?;
        let o = offset as usize;
        buf.copy_from_slice(&self.data[o..o + buf.len()]);
        Ok(())
    }
}
