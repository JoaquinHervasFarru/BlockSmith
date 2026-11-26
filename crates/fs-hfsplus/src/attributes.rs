//! Árbol de atributos (TN1150, «Attributes File»).
//!
//! Los atributos extendidos pequeños van en línea (registro 0x10). Los grandes guardan
//! sus datos en bloques propios del volumen, descritos por un registro de fork (0x20,
//! con un `HFSPlusForkData`) y, si no caben en 8 extents, por registros de extents
//! adicionales (0x30). Esos bloques no pertenecen a ningún archivo del catálogo, pero
//! están en uso: la comprobación del bitmap y la reconstrucción tienen que contarlos.

use std::cmp::Ordering;

use blocksmith_io::endian::{be_u16, be_u32};

use crate::extents::parse_record;
use crate::unicode::compare_binary;
use crate::volume_header::{Extent, ForkData};

/// Tipos de registro del árbol de atributos.
pub mod rtype {
    pub const INLINE_DATA: u32 = 0x10;
    pub const FORK_DATA: u32 = 0x20;
    pub const EXTENTS: u32 = 0x30;
}

/// Clave de un registro de atributos (sin el campo keyLength).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrKey {
    pub file_id: u32,
    pub start_block: u32,
    pub name: Vec<u16>,
}

impl AttrKey {
    /// Disposición: pad (2), fileID (4), startBlock (4), nameLength (2), nombre (UTF-16BE).
    pub fn parse(b: &[u8]) -> Option<Self> {
        if b.len() < 12 {
            return None;
        }
        let len = be_u16(b, 10) as usize;
        if len > 127 || 12 + 2 * len > b.len() {
            return None;
        }
        Some(Self {
            file_id: be_u32(b, 2),
            start_block: be_u32(b, 6),
            name: (0..len).map(|i| be_u16(b, 12 + 2 * i)).collect(),
        })
    }

    /// Orden del árbol: fileID, nombre (comparación binaria) y bloque inicial.
    pub fn compare(&self, other: &Self) -> Ordering {
        self.file_id
            .cmp(&other.file_id)
            .then_with(|| compare_binary(&self.name, &other.name))
            .then_with(|| self.start_block.cmp(&other.start_block))
    }
}

/// Extents con datos de un registro de atributos (vacío para los registros en línea).
pub fn record_extents(data: &[u8]) -> Vec<Extent> {
    if data.len() < 8 {
        return vec![];
    }
    let exts: Vec<Extent> = match be_u32(data, 0) {
        rtype::FORK_DATA if data.len() >= 8 + ForkData::SIZE => {
            ForkData::parse(&data[8..8 + ForkData::SIZE]).extents.to_vec()
        }
        rtype::EXTENTS => parse_record(&data[8..]).map(|e| e.to_vec()).unwrap_or_default(),
        _ => vec![],
    };
    exts.into_iter().filter(|e| e.block_count > 0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use blocksmith_io::endian::{put_be_u16, put_be_u32, put_be_u64};

    fn key(file_id: u32, name: &str, start: u32) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut b = vec![0u8; 12 + 2 * units.len()];
        put_be_u32(&mut b, 2, file_id);
        put_be_u32(&mut b, 6, start);
        put_be_u16(&mut b, 10, units.len() as u16);
        for (i, u) in units.iter().enumerate() {
            put_be_u16(&mut b, 12 + 2 * i, *u);
        }
        b
    }

    #[test]
    fn keys_order_by_file_name_and_block() {
        let a = AttrKey::parse(&key(20, "com.apple.ResourceFork", 0)).unwrap();
        let b = AttrKey::parse(&key(20, "dev.blocksmith", 0)).unwrap();
        let c = AttrKey::parse(&key(20, "dev.blocksmith", 8)).unwrap();
        let d = AttrKey::parse(&key(21, "a", 0)).unwrap();
        assert_eq!(a.compare(&b), Ordering::Less);
        assert_eq!(b.compare(&c), Ordering::Less);
        assert_eq!(c.compare(&d), Ordering::Less);
        assert_eq!(b.name.len(), 14);
        assert!(AttrKey::parse(&[0u8; 8]).is_none());
    }

    #[test]
    fn fork_and_extent_records_yield_their_blocks() {
        // Registro 0x20: tipo, reservado y HFSPlusForkData con dos extents.
        let mut fork = vec![0u8; 8 + ForkData::SIZE];
        put_be_u32(&mut fork, 0, rtype::FORK_DATA);
        put_be_u64(&mut fork, 8, 12_000);
        put_be_u32(&mut fork, 8 + 12, 3);
        put_be_u32(&mut fork, 8 + 16, 500);
        put_be_u32(&mut fork, 8 + 20, 2);
        put_be_u32(&mut fork, 8 + 24, 900);
        put_be_u32(&mut fork, 8 + 28, 1);
        assert_eq!(
            record_extents(&fork),
            vec![Extent { start_block: 500, block_count: 2 }, Extent { start_block: 900, block_count: 1 }]
        );
        // Registro 0x30: tipo, reservado y 8 extents.
        let mut ext = vec![0u8; 8 + 64];
        put_be_u32(&mut ext, 0, rtype::EXTENTS);
        put_be_u32(&mut ext, 8, 1000);
        put_be_u32(&mut ext, 12, 4);
        assert_eq!(record_extents(&ext), vec![Extent { start_block: 1000, block_count: 4 }]);
        // Registro en línea: no ocupa bloques.
        let mut inline = vec![0u8; 24];
        put_be_u32(&mut inline, 0, rtype::INLINE_DATA);
        assert!(record_extents(&inline).is_empty());
    }
}
