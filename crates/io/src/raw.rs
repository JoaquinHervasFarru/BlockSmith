use std::fs::File;
use std::path::{Path, PathBuf};

use crate::{check_bounds, BlockDevice, IoResult};

/// Imagen raw (bloque a bloque) abierta en solo lectura.
pub struct RawImage {
    path: PathBuf,
    file: File,
    len: u64,
}

impl RawImage {
    pub fn open(path: impl AsRef<Path>) -> IoResult<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let len = file.metadata()?.len();
        Ok(Self { path, file, len })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl BlockDevice for RawImage {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> IoResult<()> {
        check_bounds(self.len, offset, buf.len())?;
        read_at(&self.file, offset, buf)?;
        Ok(())
    }
}

#[cfg(unix)]
fn read_at(f: &File, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    f.read_exact_at(buf, offset)
}

#[cfg(windows)]
fn read_at(f: &File, mut offset: u64, mut buf: &mut [u8]) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = f.seek_read(buf, offset)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf = &mut buf[n..];
        offset += n as u64;
    }
    Ok(())
}
