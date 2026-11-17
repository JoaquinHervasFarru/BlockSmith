//! Apple Partition Map: bloque 0 con firma `ER` y entradas `PM` desde el bloque 1.

use blocksmith_io::endian::{be_u16, be_u32};
use blocksmith_io::{BlockDevice, IoResult};

use crate::{Hint, Partition, PartitionTable, Scheme};

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

pub fn read(dev: &dyn BlockDevice) -> IoResult<Option<PartitionTable>> {
    if dev.len() < 1024 {
        return Ok(None);
    }
    let b0 = dev.read_vec(0, 512)?;
    if &b0[0..2] != b"ER" {
        return Ok(None);
    }
    let block = match be_u16(&b0, 2) {
        512 | 1024 | 2048 | 4096 => be_u16(&b0, 2) as u64,
        _ => 512,
    };
    let first = dev.read_vec(block, 512)?;
    if &first[0..2] != b"PM" {
        return Ok(None);
    }
    let count = be_u32(&first, 4).min(256);
    let mut partitions = Vec::new();
    let mut warnings = Vec::new();
    for i in 0..count as u64 {
        let off = block * (1 + i);
        if off + 512 > dev.len() {
            warnings.push("Mapa de particiones APM truncado".into());
            break;
        }
        let e = dev.read_vec(off, 512)?;
        if &e[0..2] != b"PM" {
            warnings.push(format!("Entrada APM {} sin firma PM", i + 1));
            continue;
        }
        let type_id = cstr(&e[48..80]);
        let hint = match type_id.as_str() {
            "Apple_HFS" | "Apple_HFSX" => Hint::HfsPlus,
            "Apple_Free" => Hint::Free,
            "Apple_partition_map" | "Apple_Driver" | "Apple_Driver43" | "Apple_Patches" => Hint::Reserved,
            _ => Hint::Unknown,
        };
        partitions.push(Partition {
            index: i as u32 + 1,
            start: be_u32(&e, 8) as u64 * block,
            len: be_u32(&e, 12) as u64 * block,
            type_id,
            name: cstr(&e[16..48]),
            hint,
        });
    }
    Ok(Some(PartitionTable { scheme: Scheme::Apm, partitions, warnings }))
}
