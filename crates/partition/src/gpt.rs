//! GUID Partition Table (UEFI). Valida los CRC32 de cabecera y entradas y, si la
//! cabecera principal está dañada, recurre a la de respaldo (último LBA).

use blocksmith_io::endian::{le_u16, le_u32, le_u64};
use blocksmith_io::{BlockDevice, IoResult};

use crate::{Hint, Partition, PartitionTable, Scheme};

const SIGNATURE: &[u8; 8] = b"EFI PART";
const MAX_ENTRIES: u32 = 1024;

/// Formatea un GUID almacenado en el orden mixto de GPT.
pub fn guid_to_string(b: &[u8]) -> String {
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        le_u32(b, 0), le_u16(b, 4), le_u16(b, 6), b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

pub fn hint_for(type_guid: &str) -> Hint {
    match type_guid {
        "48465300-0000-11AA-AA11-00306543ECAC" => Hint::HfsPlus,
        "7C3457EF-0000-11AA-AA11-00306543ECAC" => Hint::Apfs,
        "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7" => Hint::Ntfs,
        "0FC63DAF-8483-4772-8E79-3D69D8477DE4" => Hint::LinuxFs,
        "CA7D7CCB-63ED-4C53-861C-1742536059CC" => Hint::Luks,
        "C12A7328-F81F-11D2-BA4B-00A0C93EC93B" => Hint::EfiSystem,
        "E3C9E316-0B5C-4DB8-817D-F92DF00215AE" => Hint::Reserved,
        _ => Hint::Unknown,
    }
}

struct Header {
    entries_lba: u64,
    count: u32,
    entry_size: u32,
    entries_crc: u32,
}

fn read_header(dev: &dyn BlockDevice, lba: u64, sector: u64) -> IoResult<Option<(Header, bool)>> {
    let off = lba * sector;
    if off + 92 > dev.len() {
        return Ok(None);
    }
    let h = dev.read_vec(off, 92)?;
    if &h[0..8] != SIGNATURE {
        return Ok(None);
    }
    let size = (le_u32(&h, 12) as usize).clamp(92, 512);
    let mut full = dev.read_vec(off, size)?;
    let stored = le_u32(&full, 16);
    full[16..20].fill(0);
    let crc_ok = crc32fast::hash(&full) == stored;
    Ok(Some((
        Header {
            entries_lba: le_u64(&h, 72),
            count: le_u32(&h, 80).min(MAX_ENTRIES),
            entry_size: le_u32(&h, 84),
            entries_crc: le_u32(&h, 88),
        },
        crc_ok,
    )))
}

pub fn read(dev: &dyn BlockDevice) -> IoResult<Option<PartitionTable>> {
    // Sectores de 512 y de 4096 bytes.
    for sector in [512u64, 4096] {
        let mut warnings = Vec::new();
        let primary = read_header(dev, 1, sector)?;
        let header = match primary {
            Some((h, true)) => h,
            other => {
                let last = dev.len() / sector - 1;
                match read_header(dev, last, sector)? {
                    Some((h, true)) => {
                        warnings.push("Cabecera GPT principal dañada o ausente: se usa la de respaldo".into());
                        h
                    }
                    _ => match other {
                        Some((h, false)) => {
                            warnings.push("CRC de la cabecera GPT incorrecto y sin respaldo válido".into());
                            h
                        }
                        _ => continue,
                    },
                }
            }
        };
        if header.entry_size < 128 || header.entry_size > 4096 {
            continue;
        }
        let table = dev.read_vec(header.entries_lba * sector, (header.count * header.entry_size) as usize)?;
        if crc32fast::hash(&table) != header.entries_crc {
            warnings.push("CRC de la tabla de entradas GPT incorrecto".into());
        }
        let mut partitions = Vec::new();
        for i in 0..header.count as usize {
            let e = &table[i * header.entry_size as usize..][..128];
            if e[0..16].iter().all(|&b| b == 0) {
                continue;
            }
            let type_id = guid_to_string(&e[0..16]);
            let first = le_u64(e, 32);
            let last = le_u64(e, 40);
            if last < first {
                warnings.push(format!("Entrada GPT {} con LBA final anterior al inicial", i + 1));
                continue;
            }
            let name_units: Vec<u16> =
                (0..36).map(|k| le_u16(e, 56 + k * 2)).take_while(|&c| c != 0).collect();
            partitions.push(Partition {
                index: i as u32 + 1,
                start: first * sector,
                len: (last - first + 1) * sector,
                hint: hint_for(&type_id),
                type_id,
                name: String::from_utf16_lossy(&name_units),
            });
        }
        return Ok(Some(PartitionTable { scheme: Scheme::Gpt, partitions, warnings }));
    }
    Ok(None)
}
