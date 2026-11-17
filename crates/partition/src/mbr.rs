//! Master Boot Record: cuatro entradas primarias en el offset 446 y firma 0x55AA.
//! Las particiones extendidas (0x05/0x0F) se listan pero no se recorren todavía.

use blocksmith_io::endian::le_u32;
use blocksmith_io::{BlockDevice, IoResult};

use crate::{Hint, Partition, PartitionTable, Scheme};

pub fn read(dev: &dyn BlockDevice) -> IoResult<Option<PartitionTable>> {
    if dev.len() < 512 {
        return Ok(None);
    }
    let s = dev.read_vec(0, 512)?;
    if s[510] != 0x55 || s[511] != 0xAA {
        return Ok(None);
    }
    // Un boot sector NTFS/FAT también termina en 0x55AA: si empieza como sector de
    // arranque de un sistema de archivos, no es una tabla de particiones.
    if &s[3..11] == b"NTFS    " || &s[3..8] == b"MSDOS" || &s[3..11] == b"-FVE-FS-" {
        return Ok(None);
    }
    let mut partitions = Vec::new();
    let mut warnings = Vec::new();
    for i in 0..4 {
        let e = &s[446 + i * 16..][..16];
        let kind = e[4];
        let start = le_u32(e, 8) as u64 * 512;
        let len = le_u32(e, 12) as u64 * 512;
        if kind == 0 || len == 0 {
            continue;
        }
        if start + len > dev.len() {
            warnings.push(format!("Partición MBR {} excede el tamaño del dispositivo", i + 1));
        }
        let hint = match kind {
            0x07 => Hint::Ntfs,
            0x83 => Hint::LinuxFs,
            0xAF => Hint::HfsPlus,
            0xEE => Hint::Reserved,
            _ => Hint::Unknown,
        };
        if matches!(kind, 0x05 | 0x0F | 0x85) {
            warnings.push(format!("Partición extendida {} no recorrida (pendiente)", i + 1));
        }
        partitions.push(Partition {
            index: i as u32 + 1,
            start,
            len,
            type_id: format!("0x{kind:02X}"),
            name: String::new(),
            hint,
        });
    }
    if partitions.is_empty() {
        return Ok(None);
    }
    Ok(Some(PartitionTable { scheme: Scheme::Mbr, partitions, warnings }))
}
