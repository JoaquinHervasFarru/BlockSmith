//! Extents Overflow B-tree: extents a partir del noveno de cada fork.

use std::cmp::Ordering;

use dirforge_io::endian::{be_u32, put_be_u16, put_be_u32};

use crate::volume_header::Extent;

pub const FORK_DATA: u8 = 0x00;
pub const FORK_RSRC: u8 = 0xFF;
pub const KEY_LENGTH: u16 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExtentKey {
    pub fork_type: u8,
    pub file_id: u32,
    pub start_block: u32,
}

impl ExtentKey {
    pub fn parse(b: &[u8]) -> Option<Self> {
        if b.len() < KEY_LENGTH as usize {
            return None;
        }
        Some(Self { fork_type: b[0], file_id: be_u32(b, 2), start_block: be_u32(b, 6) })
    }

    pub fn to_bytes(&self) -> [u8; 12] {
        let mut b = [0u8; 12];
        put_be_u16(&mut b, 0, KEY_LENGTH);
        b[2] = self.fork_type;
        put_be_u32(&mut b, 4, self.file_id);
        put_be_u32(&mut b, 8, self.start_block);
        b
    }

    /// Orden: fileID, tipo de fork y bloque inicial (en bloques del fork).
    pub fn compare(&self, other: &Self) -> Ordering {
        self.file_id
            .cmp(&other.file_id)
            .then(self.fork_type.cmp(&other.fork_type))
            .then(self.start_block.cmp(&other.start_block))
    }
}

pub fn parse_record(b: &[u8]) -> Option<[Extent; 8]> {
    if b.len() < 64 {
        return None;
    }
    let mut out = [Extent::default(); 8];
    for (i, e) in out.iter_mut().enumerate() {
        e.start_block = be_u32(b, i * 8);
        e.block_count = be_u32(b, i * 8 + 4);
    }
    Some(out)
}
