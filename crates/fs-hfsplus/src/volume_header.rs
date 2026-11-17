//! Volume Header HFS+/HFSX (TN1150). Está a 1024 bytes del inicio del volumen y
//! hay una copia (alternate) a 1024 bytes del final.

use dirforge_io::endian::{be_u16, be_u32, be_u64, put_be_u16, put_be_u32, put_be_u64};
use serde::Serialize;

pub const SIZE: usize = 512;

/// Bits de `attributes`.
pub mod attr {
    pub const UNMOUNTED: u32 = 1 << 8;
    pub const SPARED_BLOCKS: u32 = 1 << 9;
    pub const INCONSISTENT: u32 = 1 << 11;
    pub const CNIDS_REUSED: u32 = 1 << 12;
    pub const JOURNALED: u32 = 1 << 13;
    pub const SOFTWARE_LOCK: u32 = 1 << 15;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Extent {
    pub start_block: u32,
    pub block_count: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ForkData {
    pub logical_size: u64,
    pub clump_size: u32,
    pub total_blocks: u32,
    pub extents: [Extent; 8],
}

impl ForkData {
    pub const SIZE: usize = 80;

    pub fn parse(b: &[u8]) -> Self {
        let mut extents = [Extent::default(); 8];
        for (i, e) in extents.iter_mut().enumerate() {
            e.start_block = be_u32(b, 16 + i * 8);
            e.block_count = be_u32(b, 20 + i * 8);
        }
        Self { logical_size: be_u64(b, 0), clump_size: be_u32(b, 8), total_blocks: be_u32(b, 12), extents }
    }

    pub fn write(&self, b: &mut [u8]) {
        put_be_u64(b, 0, self.logical_size);
        put_be_u32(b, 8, self.clump_size);
        put_be_u32(b, 12, self.total_blocks);
        for (i, e) in self.extents.iter().enumerate() {
            put_be_u32(b, 16 + i * 8, e.start_block);
            put_be_u32(b, 20 + i * 8, e.block_count);
        }
    }

    /// Bloques descritos por los 8 extents en línea.
    pub fn inline_blocks(&self) -> u64 {
        self.extents.iter().map(|e| e.block_count as u64).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VolumeHeader {
    pub signature: [u8; 2],
    pub version: u16,
    pub attributes: u32,
    pub last_mounted_version: u32,
    pub journal_info_block: u32,
    pub create_date: u32,
    pub modify_date: u32,
    pub backup_date: u32,
    pub checked_date: u32,
    pub file_count: u32,
    pub folder_count: u32,
    pub block_size: u32,
    pub total_blocks: u32,
    pub free_blocks: u32,
    pub next_allocation: u32,
    pub rsrc_clump_size: u32,
    pub data_clump_size: u32,
    pub next_catalog_id: u32,
    pub write_count: u32,
    pub encodings_bitmap: u64,
    pub finder_info: [u32; 8],
    pub allocation_file: ForkData,
    pub extents_file: ForkData,
    pub catalog_file: ForkData,
    pub attributes_file: ForkData,
    pub startup_file: ForkData,
}

impl VolumeHeader {
    pub fn parse(b: &[u8]) -> Option<Self> {
        if b.len() < SIZE {
            return None;
        }
        let signature = [b[0], b[1]];
        if &signature != b"H+" && &signature != b"HX" {
            return None;
        }
        let mut finder_info = [0u32; 8];
        for (i, f) in finder_info.iter_mut().enumerate() {
            *f = be_u32(b, 80 + i * 4);
        }
        Some(Self {
            signature,
            version: be_u16(b, 2),
            attributes: be_u32(b, 4),
            last_mounted_version: be_u32(b, 8),
            journal_info_block: be_u32(b, 12),
            create_date: be_u32(b, 16),
            modify_date: be_u32(b, 20),
            backup_date: be_u32(b, 24),
            checked_date: be_u32(b, 28),
            file_count: be_u32(b, 32),
            folder_count: be_u32(b, 36),
            block_size: be_u32(b, 40),
            total_blocks: be_u32(b, 44),
            free_blocks: be_u32(b, 48),
            next_allocation: be_u32(b, 52),
            rsrc_clump_size: be_u32(b, 56),
            data_clump_size: be_u32(b, 60),
            next_catalog_id: be_u32(b, 64),
            write_count: be_u32(b, 68),
            encodings_bitmap: be_u64(b, 72),
            finder_info,
            allocation_file: ForkData::parse(&b[112..192]),
            extents_file: ForkData::parse(&b[192..272]),
            catalog_file: ForkData::parse(&b[272..352]),
            attributes_file: ForkData::parse(&b[352..432]),
            startup_file: ForkData::parse(&b[432..512]),
        })
    }

    pub fn to_bytes(&self) -> [u8; SIZE] {
        let mut b = [0u8; SIZE];
        b[0..2].copy_from_slice(&self.signature);
        put_be_u16(&mut b, 2, self.version);
        for (off, v) in [
            (4, self.attributes), (8, self.last_mounted_version), (12, self.journal_info_block),
            (16, self.create_date), (20, self.modify_date), (24, self.backup_date), (28, self.checked_date),
            (32, self.file_count), (36, self.folder_count), (40, self.block_size), (44, self.total_blocks),
            (48, self.free_blocks), (52, self.next_allocation), (56, self.rsrc_clump_size),
            (60, self.data_clump_size), (64, self.next_catalog_id), (68, self.write_count),
        ] {
            put_be_u32(&mut b, off, v);
        }
        put_be_u64(&mut b, 72, self.encodings_bitmap);
        for (i, f) in self.finder_info.iter().enumerate() {
            put_be_u32(&mut b, 80 + i * 4, *f);
        }
        self.allocation_file.write(&mut b[112..192]);
        self.extents_file.write(&mut b[192..272]);
        self.catalog_file.write(&mut b[272..352]);
        self.attributes_file.write(&mut b[352..432]);
        self.startup_file.write(&mut b[432..512]);
        b
    }

    pub fn is_hfsx(&self) -> bool {
        &self.signature == b"HX"
    }

    pub fn is_journaled(&self) -> bool {
        self.attributes & attr::JOURNALED != 0
    }

    pub fn volume_bytes(&self) -> u64 {
        self.total_blocks as u64 * self.block_size as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut b = [0u8; SIZE];
        b[0..2].copy_from_slice(b"H+");
        put_be_u16(&mut b, 2, 4);
        put_be_u32(&mut b, 40, 4096);
        put_be_u32(&mut b, 44, 2048);
        put_be_u32(&mut b, 272 + 16, 7);
        put_be_u32(&mut b, 272 + 20, 3);
        let vh = VolumeHeader::parse(&b).unwrap();
        assert_eq!(vh.block_size, 4096);
        assert_eq!(vh.catalog_file.extents[0], Extent { start_block: 7, block_count: 3 });
        assert_eq!(vh.to_bytes(), b);
        b[0] = b'X';
        assert!(VolumeHeader::parse(&b).is_none());
    }
}
