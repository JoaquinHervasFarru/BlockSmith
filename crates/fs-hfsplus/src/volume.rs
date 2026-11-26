//! Localización y apertura de un volumen HFS+/HFSX dentro de un dispositivo.

use std::collections::HashMap;

use blocksmith_io::endian::{be_u16, be_u32};
use blocksmith_io::{BlockDevice, IoResult};

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
    pub fn fork_reader(
        &self,
        fork: &ForkData,
        file_id: u32,
        data_fork: bool,
        overflow: &OverflowMap,
    ) -> ForkReader<'a> {
        let extents = full_extents(fork, file_id, if data_fork { FORK_DATA } else { FORK_RSRC }, overflow);
        ForkReader::new(self.dev, self.offset, self.vh.block_size, extents, fork.logical_size)
    }

    /// Posiciones posibles de la copia alternativa del Volume Header, por orden de
    /// preferencia. TN1150 la sitúa en el penúltimo sector de 512 bytes del volumen, es
    /// decir, de la partición que lo contiene; si la partición no mide un número entero
    /// de bloques, eso queda después del último bloque. Se acepta también la posición
    /// relativa al último bloque, y es la única para un volumen dentro de un wrapper HFS.
    pub fn alternate_header_offsets(&self) -> Vec<u64> {
        let mut out = Vec::new();
        if !self.wrapped && self.dev.len() >= self.offset + 2048 {
            out.push(self.dev.len() - 1024);
        }
        let by_blocks = self.offset + self.vh.volume_bytes();
        if by_blocks >= self.offset + 2048 && by_blocks <= self.dev.len() && !out.contains(&(by_blocks - 1024)) {
            out.push(by_blocks - 1024);
        }
        out
    }

    /// Lee la copia alternativa del Volume Header: (offset absoluto, cabecera).
    pub fn alternate_header_at(&self) -> IoResult<Option<(u64, VolumeHeader)>> {
        for off in self.alternate_header_offsets() {
            if let Some(vh) = VolumeHeader::parse(&self.dev.read_vec(off, SIZE)?) {
                return Ok(Some((off, vh)));
            }
        }
        Ok(None)
    }

    /// Lee la copia alternativa del Volume Header.
    pub fn alternate_header(&self) -> IoResult<Option<VolumeHeader>> {
        Ok(self.alternate_header_at()?.map(|(_, vh)| vh))
    }
}

/// Nombre del volumen (nombre de la carpeta raíz) sin escanear todo el catálogo:
/// busca el thread record de la raíz descendiendo por el índice. `None` si el
/// catálogo está demasiado dañado para encontrarlo así.
pub fn volume_name(vol: &Volume<'_>) -> Option<String> {
    use crate::btree::{kind, split_record, BTree};
    use crate::catalog::{cnid, CatalogKey, CatalogRecord};
    let reader = vol.fork_reader(&vol.vh.catalog_file, cnid::CATALOG_FILE, true, &OverflowMap::new());
    let tree = BTree::open(reader).ok()?.ok()?;
    let binary = vol.vh.is_hfsx() && tree.header.key_compare_type == 0xBC;
    let target = CatalogKey { parent_id: cnid::ROOT_FOLDER, name: vec![] };
    let mut node_no = tree.header.root_node;
    for _ in 0..16 {
        let node = tree.read_node(node_no).ok()?.ok()?;
        let is_index = node.desc.kind == kind::INDEX;
        let mut chosen = None;
        for i in 0..node.records.len() {
            let (k, d) = split_record(node.record(i), is_index, &tree.header)?;
            let key = CatalogKey::parse(k)?;
            match key.compare(&target, binary) {
                std::cmp::Ordering::Greater => break,
                std::cmp::Ordering::Equal if !is_index => {
                    return match CatalogRecord::parse(d)? {
                        CatalogRecord::FolderThread(t) => Some(crate::unicode::from_hfs_name(&t.name)),
                        _ => None,
                    };
                }
                _ => chosen = Some(d),
            }
        }
        if !is_index {
            return None;
        }
        node_no = be_u32(chosen?, 0);
    }
    None
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
