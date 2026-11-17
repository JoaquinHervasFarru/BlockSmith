//! Localización y apertura de un volumen HFS+/HFSX dentro de un dispositivo.

use std::collections::HashMap;

use dirforge_io::endian::{be_u16, be_u32};
use dirforge_io::{BlockDevice, IoResult};

use crate::extents::{FORK_DATA, FORK_RSRC};
use crate::fork::ForkReader;
use crate::volume_header::{Extent, ForkData, VolumeHeader, SIZE};

pub const VOLUME_HEADER_OFFSET: u64 = 1024;

/// Extents adicionales por (fileID, tipo de fork): (bloque inicial dentro del fork, extents).
pub type OverflowMap = HashMap<(u32, u8), Vec<(u32, [Extent; 8])>>;

pub struct Volume<'a> {
    pub dev: &'a dyn BlockDevice,
    /// Offset del volumen HFS+ dentro del dispositivo.
    pub offset: u64,
    pub vh: VolumeHeader,
    /// `true` si está embebido en un wrapper HFS clásico.
    pub wrapped: bool,
}

/// Busca la cabecera HFS+: directa en el offset 1024 o embebida en un wrapper HFS.
pub fn locate(dev: &dyn BlockDevice) -> IoResult<Option<(u64, bool)>> {
    if dev.len() < VOLUME_HEADER_OFFSET + SIZE as u64 {
        return Ok(None);
    }
    let b = dev.read_vec(VOLUME_HEADER_OFFSET, SIZE)?;
    if &b[0..2] == b"H+" || &b[0..2] == b"HX" {
        return Ok(Some((0, false)));
    }
    // Master Directory Block de HFS con volumen HFS+ embebido.
    if &b[0..2] == b"BD" && &b[0x7C..0x7E] == b"H+" {
        let al_blk_size = be_u32(&b, 0x14) as u64;
        let al_bl_st = be_u16(&b, 0x1C) as u64;
        let start = be_u16(&b, 0x7E) as u64;
        let offset = al_bl_st * 512 + start * al_blk_size;
        if offset + VOLUME_HEADER_OFFSET + SIZE as u64 <= dev.len() {
            let inner = dev.read_vec(offset + VOLUME_HEADER_OFFSET, 2)?;
            if &inner == b"H+" {
                return Ok(Some((offset, true)));
            }
        }
    }
    Ok(None)
}

impl<'a> Volume<'a> {
    pub fn open(dev: &'a dyn BlockDevice) -> IoResult<Option<Self>> {
        let Some((offset, wrapped)) = locate(dev)? else { return Ok(None) };
        let b = dev.read_vec(offset + VOLUME_HEADER_OFFSET, SIZE)?;
        Ok(VolumeHeader::parse(&b).map(|vh| Self { dev, offset, vh, wrapped }))
    }

    /// Lector de un fork con sus extents en línea y los de desbordamiento conocidos.
    pub fn fork_reader(&self, fork: &ForkData, file_id: u32, data_fork: bool, overflow: &OverflowMap) -> ForkReader<'a> {
        let extents = full_extents(fork, file_id, if data_fork { FORK_DATA } else { FORK_RSRC }, overflow);
        ForkReader::new(self.dev, self.offset, self.vh.block_size, extents, fork.logical_size)
    }

    /// Lee la copia alternativa del Volume Header (1024 bytes antes del final del volumen).
    pub fn alternate_header(&self) -> IoResult<Option<VolumeHeader>> {
        let end = self.offset + self.vh.volume_bytes();
        if end < 1024 || end > self.dev.len() {
            return Ok(None);
        }
        Ok(VolumeHeader::parse(&self.dev.read_vec(end - 1024, SIZE)?))
    }
}

/// Extents completos de un fork: los 8 en línea más los del Extents Overflow, en orden.
pub fn full_extents(fork: &ForkData, file_id: u32, fork_type: u8, overflow: &OverflowMap) -> Vec<Extent> {
    let mut out: Vec<Extent> = fork.extents.iter().copied().filter(|e| e.block_count > 0).collect();
    if let Some(list) = overflow.get(&(file_id, fork_type)) {
        let mut list = list.clone();
        list.sort_by_key(|(start, _)| *start);
        for (_, exts) in list {
            out.extend(exts.iter().copied().filter(|e| e.block_count > 0));
        }
    }
    out
}
