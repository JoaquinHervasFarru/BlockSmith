//! Lectura del contenido lógico de un fork a través de su lista de extents.

use blocksmith_io::{BlockDevice, IoError, IoResult};

use crate::volume_header::Extent;

pub struct ForkReader<'a> {
    dev: &'a dyn BlockDevice,
    vol_offset: u64,
    block_size: u64,
    extents: Vec<Extent>,
    logical_size: u64,
}

impl<'a> ForkReader<'a> {
    pub fn new(
        dev: &'a dyn BlockDevice,
        vol_offset: u64,
        block_size: u32,
        extents: Vec<Extent>,
        logical_size: u64,
    ) -> Self {
        let extents = extents.into_iter().filter(|e| e.block_count > 0).collect();
        Self { dev, vol_offset, block_size: block_size as u64, extents, logical_size }
    }

    pub fn logical_size(&self) -> u64 {
        self.logical_size
    }

    /// Bytes cubiertos por los extents conocidos.
    pub fn mapped_bytes(&self) -> u64 {
        self.extents.iter().map(|e| e.block_count as u64).sum::<u64>() * self.block_size
    }

    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> IoResult<()> {
        let mut done = 0usize;
        let mut logical = offset;
        let mut ext_start = 0u64;
        for e in &self.extents {
            let ext_len = e.block_count as u64 * self.block_size;
            if done == buf.len() {
                break;
            }
            if logical < ext_start + ext_len {
                let within = logical - ext_start;
                let n = ((ext_len - within) as usize).min(buf.len() - done);
                let phys = self.vol_offset + e.start_block as u64 * self.block_size + within;
                self.dev.read_exact_at(phys, &mut buf[done..done + n])?;
                done += n;
                logical += n as u64;
            }
            ext_start += ext_len;
        }
        if done < buf.len() {
            return Err(IoError::Corrupt(format!("el fork no tiene extents para el offset lógico {logical}")));
        }
        Ok(())
    }

    pub fn read_vec(&self, offset: u64, len: usize) -> IoResult<Vec<u8>> {
        let mut v = vec![0u8; len];
        self.read_at(offset, &mut v)?;
        Ok(v)
    }
}
