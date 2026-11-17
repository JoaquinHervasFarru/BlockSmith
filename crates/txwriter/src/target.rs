//! Destinos de escritura.

use std::fs::File;
use std::io;

pub trait Target {
    fn len(&self) -> u64;
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
    fn write_at(&mut self, offset: u64, data: &[u8]) -> io::Result<()>;
    /// Garantiza que lo escrito ha llegado al medio físico.
    fn sync(&mut self) -> io::Result<()>;
    /// Identificación para registrar en la transacción (ruta, dispositivo…).
    fn describe(&self) -> String;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Destino en memoria para pruebas.
pub struct MemTarget(pub Vec<u8>);

impl Target for MemTarget {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let o = offset as usize;
        let src = self.0.get(o..o + buf.len()).ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
        buf.copy_from_slice(src);
        Ok(())
    }
    fn write_at(&mut self, offset: u64, data: &[u8]) -> io::Result<()> {
        let o = offset as usize;
        let dst = self.0.get_mut(o..o + data.len()).ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
        dst.copy_from_slice(data);
        Ok(())
    }
    fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn describe(&self) -> String {
        format!("memoria ({} bytes)", self.0.len())
    }
}

#[cfg(unix)]
pub(crate) fn pread(f: &File, offset: u64, buf: &mut [u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    f.read_exact_at(buf, offset)
}

#[cfg(unix)]
pub(crate) fn pwrite(f: &File, offset: u64, data: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    f.write_all_at(data, offset)
}

#[cfg(windows)]
pub(crate) fn pread(f: &File, mut offset: u64, mut buf: &mut [u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = f.seek_read(buf, offset)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        buf = &mut buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn pwrite(f: &File, mut offset: u64, mut data: &[u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !data.is_empty() {
        let n = f.seek_write(data, offset)?;
        data = &data[n..];
        offset += n as u64;
    }
    Ok(())
}
