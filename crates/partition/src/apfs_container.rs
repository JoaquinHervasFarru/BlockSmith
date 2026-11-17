//! Detección de contenedores APFS: el superbloque `NXSB` empieza en el bloque 0
//! de la partición, con la firma en el offset 32.

use blocksmith_io::{BlockDevice, IoResult};

pub const NX_MAGIC: &[u8; 4] = b"NXSB";

pub fn is_apfs_container(dev: &dyn BlockDevice) -> IoResult<bool> {
    if dev.len() < 4096 {
        return Ok(false);
    }
    Ok(dev.read_vec(32, 4)? == NX_MAGIC)
}
