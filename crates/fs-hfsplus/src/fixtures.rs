//! Generador de volúmenes HFS+/HFSX de prueba y daños deterministas.
//!
//! El entorno de CI no puede montar HFS+, así que las fixtures se construyen
//! byte a byte. Su validez se comprueba con un verificador independiente
//! (`fsck.hfsplus`) en los tests de integración. Usa el mismo empaquetado de
//! B-trees que el reconstructor ([`crate::btree::pack`]).

use std::cmp::Ordering;

use blocksmith_io::endian::{be_u16, be_u32, put_be_u16, put_be_u32};

use crate::bitmap::AllocationBitmap;
use crate::btree::pack::{pack_btree, TreeSpec};
use crate::btree::{attr as battr, HeaderRecord, DESCRIPTOR_SIZE};
use crate::catalog::{cnid, CatalogKey, CatalogRecord, CommonInfo, FileRecord, FolderRecord, ThreadRecord, FILE_THREAD_EXISTS};
use crate::unicode::to_hfs_name;
use crate::volume_header::{attr, Extent, ForkData, VolumeHeader};

/// 1 de enero de 2024 en segundos desde 1904 (fecha fija: fixtures deterministas).
pub const FIXED_DATE: u32 = 3_786_825_600;

#[derive(Debug, Clone)]
pub enum Entry {
    Folder { name: String, children: Vec<Entry> },
    File { name: String, data: Vec<u8> },
}

impl Entry {
    pub fn folder(name: &str, children: Vec<Entry>) -> Self {
        Self::Folder { name: name.into(), children }
    }
    pub fn file(name: &str, data: impl Into<Vec<u8>>) -> Self {
        Self::File { name: name.into(), data: data.into() }
    }
}

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub volume_name: String,
    pub size_bytes: u64,
    pub block_size: u32,
    pub node_size: u16,
    /// HFSX con comparación binaria (sensible a mayúsculas).
    pub case_sensitive: bool,
    /// Límite de registros por hoja para forzar árboles de varios niveles con pocos datos.
    pub max_records_per_leaf: Option<usize>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            volume_name: "BlockSmith Test".into(),
            size_bytes: 32 * 1024 * 1024,
            block_size: 4096,
            node_size: 4096,
            case_sensitive: false,
            max_records_per_leaf: None,
        }
    }
}

/// Árbol de ejemplo con carpetas anidadas, nombres acentuados y archivos de varios tamaños.
pub fn sample_tree() -> Vec<Entry> {
    let photos: Vec<Entry> = (1..=24)
        .map(|i| Entry::file(&format!("IMG_{:04}.HEIC", 4400 + i), vec![(i % 251) as u8; 1500 + i * 700]))
        .collect();
    vec![
        Entry::folder("Documentos", vec![
            Entry::folder("Facturación", vec![
                Entry::file("resumen.pdf", b"%PDF-1.7 resumen".repeat(300)),
                Entry::file("facturas_q4.csv", "fecha;importe\n2024-10-01;120,50\n".repeat(50)),
            ]),
            Entry::file("Carta de presentación.txt", "Estimada señora:\n".repeat(20)),
            Entry::file("vacío.txt", Vec::new()),
        ]),
        Entry::folder("Fotos", vec![
            Entry::folder("2023", vec![Entry::folder("Viaje Lisboa", photos)]),
            Entry::folder("2024", vec![Entry::file("Año nuevo.jpg", vec![0xFF; 9000])]),
        ]),
        Entry::folder("Proyectos", vec![
            Entry::folder("Catálogo primavera", (1..=12).map(|i| Entry::file(&format!("pieza-{i:02}.txt"), format!("pieza {i}\n"))).collect()),
        ]),
        Entry::file("Léeme.txt", "Volumen de prueba generado por BlockSmith.\n"),
    ]
}

pub(crate) fn info(mode: u16, flags: u16) -> CommonInfo {
    let mut permissions = [0u8; 16];
    put_be_u16(&mut permissions, 10, mode);
    CommonInfo {
        flags,
        create_date: FIXED_DATE,
        content_mod_date: FIXED_DATE,
        attribute_mod_date: FIXED_DATE,
        access_date: FIXED_DATE,
        backup_date: 0,
        permissions,
        user_info: [0; 16],
        finder_info: [0; 16],
        text_encoding: 0,
    }
}

struct Flat {
    records: Vec<(CatalogKey, CatalogRecord)>,
    files: u32,
    folders: u32,
    next_id: u32,
    data: Vec<(u32, Vec<u8>)>,
}

fn flatten(entries: &[Entry], parent: u32, flat: &mut Flat) -> u32 {
    for e in entries {
        let id = flat.next_id;
        flat.next_id += 1;
        match e {
            Entry::Folder { name, children } => {
                let n = to_hfs_name(name);
                let valence = flatten(children, id, flat);
                flat.folders += 1;
                flat.records.push((CatalogKey { parent_id: parent, name: n.clone() },
                    CatalogRecord::Folder(FolderRecord { valence, folder_id: id, info: info(0o040755, 0), folder_count: 0 })));
                flat.records.push((CatalogKey { parent_id: id, name: vec![] },
                    CatalogRecord::FolderThread(ThreadRecord { parent_id: parent, name: n })));
            }
            Entry::File { name, data } => {
                let n = to_hfs_name(name);
                flat.files += 1;
                flat.data.push((id, data.clone()));
                flat.records.push((CatalogKey { parent_id: parent, name: n.clone() },
                    CatalogRecord::File(FileRecord {
                        file_id: id,
                        info: info(0o100644, FILE_THREAD_EXISTS),
                        data_fork: ForkData::default(),
                        rsrc_fork: ForkData::default(),
                    })));
                flat.records.push((CatalogKey { parent_id: id, name: vec![] },
                    CatalogRecord::FileThread(ThreadRecord { parent_id: parent, name: n })));
            }
        }
    }
    entries.len() as u32
}

/// Construye la imagen completa de un volumen HFS+/HFSX.
pub fn build_volume(opts: &BuildOptions, root: &[Entry]) -> Result<Vec<u8>, String> {
    let bs = opts.block_size as u64;
    let total_blocks = (opts.size_bytes / bs) as u32;
    let nodes_per_block = (bs / opts.node_size as u64).max(1) as u32;

    let vname = to_hfs_name(&opts.volume_name);
    let mut flat = Flat { records: Vec::new(), files: 0, folders: 0, next_id: cnid::FIRST_USER, data: Vec::new() };
    let root_valence = flatten(root, cnid::ROOT_FOLDER, &mut flat);
    flat.records.push((CatalogKey { parent_id: cnid::ROOT_PARENT, name: vname.clone() },
        CatalogRecord::Folder(FolderRecord { valence: root_valence, folder_id: cnid::ROOT_FOLDER, info: info(0o040755, 0), folder_count: 0 })));
    flat.records.push((CatalogKey { parent_id: cnid::ROOT_FOLDER, name: vec![] },
        CatalogRecord::FolderThread(ThreadRecord { parent_id: cnid::ROOT_PARENT, name: vname })));

    // Distribución de bloques.
    let mut bitmap = AllocationBitmap::new_empty(total_blocks);
    let head = 1536u64.div_ceil(bs) as u32;
    bitmap.set_range(0, head);
    let tail = ((opts.size_bytes - 1024) / bs) as u32;
    bitmap.set_range(tail, total_blocks - tail);
    let mut next_block = head;
    let mut alloc = |blocks: u32, bitmap: &mut AllocationBitmap| -> Result<Extent, String> {
        if next_block + blocks > tail {
            return Err("el volumen es demasiado pequeño para el contenido".into());
        }
        let e = Extent { start_block: next_block, block_count: blocks };
        bitmap.set_range(next_block, blocks);
        next_block += blocks;
        Ok(e)
    };
    let fork = |e: Extent, logical: u64| {
        let mut extents = [Extent::default(); 8];
        extents[0] = e;
        ForkData { logical_size: logical, clump_size: opts.block_size, total_blocks: e.block_count, extents }
    };

    let bitmap_bytes = (total_blocks as u64).div_ceil(8);
    let alloc_blocks = bitmap_bytes.div_ceil(bs) as u32;
    let alloc_ext = alloc(alloc_blocks, &mut bitmap)?;
    let ext_nodes = 8u32.div_ceil(nodes_per_block) * nodes_per_block;
    let ext_ext = alloc(ext_nodes / nodes_per_block, &mut bitmap)?;
    // Catálogo: el doble de los nodos necesarios aproximados, mínimo 32.
    let approx: usize = flat.records.iter().map(|(k, r)| k.to_bytes().len() + r.to_bytes().len() + 2).sum();
    let leaf_estimate = match opts.max_records_per_leaf {
        Some(l) => flat.records.len().div_ceil(l),
        None => approx.div_ceil(opts.node_size as usize - 64),
    };
    let cat_nodes = ((leaf_estimate as u32 * 2 + 16).max(32)).div_ceil(nodes_per_block) * nodes_per_block;
    let cat_ext = alloc(cat_nodes / nodes_per_block, &mut bitmap)?;

    // Datos de los archivos, contiguos.
    let mut image = vec![0u8; opts.size_bytes as usize];
    let mut file_forks = std::collections::HashMap::new();
    for (id, data) in &flat.data {
        if data.is_empty() {
            continue;
        }
        let blocks = (data.len() as u64).div_ceil(bs) as u32;
        let e = alloc(blocks, &mut bitmap)?;
        let off = e.start_block as usize * bs as usize;
        image[off..off + data.len()].copy_from_slice(data);
        file_forks.insert(*id, fork(e, data.len() as u64));
    }
    for (_, rec) in &mut flat.records {
        if let CatalogRecord::File(f) = rec {
            if let Some(fd) = file_forks.get(&f.file_id) {
                f.data_fork = *fd;
            }
        }
    }

    // Catálogo ordenado y empaquetado.
    let binary = opts.case_sensitive;
    flat.records.sort_by(|a, b| a.0.compare(&b.0, binary));
    if flat.records.windows(2).any(|w| w[0].0.compare(&w[1].0, binary) == Ordering::Equal) {
        return Err("nombres duplicados en la misma carpeta".into());
    }
    let cat_records: Vec<(Vec<u8>, Vec<u8>)> = flat.records.iter().map(|(k, r)| (k.to_bytes(), r.to_bytes())).collect();
    let (cat_tree, _) = pack_btree(&cat_records, &TreeSpec {
        node_size: opts.node_size,
        max_key_length: 516,
        attributes: battr::BIG_KEYS | battr::VARIABLE_INDEX_KEYS,
        key_compare_type: if binary { 0xBC } else { 0xCF },
        total_nodes: cat_nodes,
        max_records_per_leaf: opts.max_records_per_leaf,
    })?;
    let (ext_tree, _) = pack_btree(&[], &TreeSpec {
        node_size: opts.node_size,
        max_key_length: 10,
        attributes: battr::BIG_KEYS,
        key_compare_type: 0,
        total_nodes: ext_nodes,
        max_records_per_leaf: None,
    })?;
    let place = |image: &mut Vec<u8>, e: Extent, bytes: &[u8]| {
        let off = e.start_block as usize * bs as usize;
        image[off..off + bytes.len()].copy_from_slice(bytes);
    };
    place(&mut image, cat_ext, &cat_tree.concat());
    place(&mut image, ext_ext, &ext_tree.concat());
    place(&mut image, alloc_ext, bitmap.as_bytes());

    let vh = VolumeHeader {
        signature: if opts.case_sensitive { *b"HX" } else { *b"H+" },
        version: if opts.case_sensitive { 5 } else { 4 },
        attributes: attr::UNMOUNTED,
        last_mounted_version: u32::from_be_bytes(*b"10.0"),
        journal_info_block: 0,
        create_date: FIXED_DATE,
        modify_date: FIXED_DATE,
        backup_date: 0,
        checked_date: FIXED_DATE,
        file_count: flat.files,
        folder_count: flat.folders,
        block_size: opts.block_size,
        total_blocks,
        free_blocks: total_blocks - bitmap.count_set(),
        next_allocation: next_block,
        rsrc_clump_size: 65536,
        data_clump_size: 65536,
        next_catalog_id: flat.next_id,
        write_count: 1,
        encodings_bitmap: 1,
        finder_info: [0; 8],
        allocation_file: fork(alloc_ext, alloc_blocks as u64 * bs),
        extents_file: fork(ext_ext, ext_nodes as u64 * opts.node_size as u64),
        catalog_file: fork(cat_ext, cat_nodes as u64 * opts.node_size as u64),
        attributes_file: ForkData::default(),
        startup_file: ForkData::default(),
    };
    let vhb = vh.to_bytes();
    image[1024..1536].copy_from_slice(&vhb);
    let alt = opts.size_bytes as usize - 1024;
    image[alt..alt + 512].copy_from_slice(&vhb);
    Ok(image)
}

/// Daños deterministas para generar fixtures corruptas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Damage {
    KeyOrder,
    BrokenLink,
    NodeUnreadable,
    ValenceWrong,
    FileCountWrong,
    NextCnidLow,
    ExtentOverlap,
    ExtentOutOfRange,
    BitmapLeak,
    BitmapUnderalloc,
    /// Registro de carpeta ilegible; su thread record sobrevive.
    FolderRecordLost,
    /// Registro de carpeta y su thread ilegibles: el contenido queda huérfano.
    FolderAndThreadLost,
}

impl Damage {
    pub const ALL: [Damage; 12] = [
        Self::KeyOrder, Self::BrokenLink, Self::NodeUnreadable, Self::ValenceWrong, Self::FileCountWrong,
        Self::NextCnidLow, Self::ExtentOverlap, Self::ExtentOutOfRange, Self::BitmapLeak, Self::BitmapUnderalloc,
        Self::FolderRecordLost, Self::FolderAndThreadLost,
    ];

    /// Código de hallazgo que el escaneo debe producir.
    pub fn expected_code(self) -> &'static str {
        match self {
            Self::KeyOrder => "HFS_BTREE_KEY_ORDER",
            Self::BrokenLink => "HFS_BTREE_LINK_BROKEN",
            Self::NodeUnreadable => "HFS_NODE_UNREADABLE",
            Self::ValenceWrong => "HFS_VALENCE_MISMATCH",
            Self::FileCountWrong => "HFS_FILE_COUNT",
            Self::NextCnidLow => "HFS_NEXT_CNID",
            Self::ExtentOverlap => "HFS_EXTENT_OVERLAP",
            Self::ExtentOutOfRange => "HFS_EXTENT_OUT_OF_RANGE",
            Self::BitmapLeak => "HFS_BITMAP_LEAK",
            Self::BitmapUnderalloc => "HFS_BITMAP_UNDERALLOC",
            Self::FolderRecordLost | Self::FolderAndThreadLost => "HFS_RECORD_INVALID",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::KeyOrder => "key-order",
            Self::BrokenLink => "broken-link",
            Self::NodeUnreadable => "node-unreadable",
            Self::ValenceWrong => "valence",
            Self::FileCountWrong => "file-count",
            Self::NextCnidLow => "next-cnid",
            Self::ExtentOverlap => "extent-overlap",
            Self::ExtentOutOfRange => "extent-out-of-range",
            Self::BitmapLeak => "bitmap-leak",
            Self::BitmapUnderalloc => "bitmap-underalloc",
            Self::FolderRecordLost => "folder-record-lost",
            Self::FolderAndThreadLost => "folder-and-thread-lost",
        }
    }
}

/// Acceso a la disposición de una imagen generada por [`build_volume`] (catálogo contiguo).
struct Layout {
    vh: VolumeHeader,
    cat_off: usize,
    node_size: usize,
}

impl Layout {
    fn read(img: &[u8]) -> Result<Self, String> {
        let vh = VolumeHeader::parse(&img[1024..1536]).ok_or("no es HFS+")?;
        let bs = vh.block_size as usize;
        let cat_off = vh.catalog_file.extents[0].start_block as usize * bs;
        let node_size = be_u16(img, cat_off + DESCRIPTOR_SIZE + 18) as usize;
        Ok(Self { vh, cat_off, node_size })
    }
    fn node(&self, n: u32) -> usize {
        self.cat_off + n as usize * self.node_size
    }
    fn header(&self, img: &[u8]) -> HeaderRecord {
        HeaderRecord::parse(&img[self.cat_off + DESCRIPTOR_SIZE..])
    }
    fn write_vh(&self, img: &mut [u8]) {
        img[1024..1536].copy_from_slice(&self.vh.to_bytes());
    }
    /// (offset absoluto del registro, longitud) de los registros de un nodo.
    fn records(&self, img: &[u8], n: u32) -> Vec<(usize, usize)> {
        let base = self.node(n);
        let count = be_u16(img, base + 10) as usize;
        (0..count)
            .map(|i| {
                let a = be_u16(img, base + self.node_size - 2 * (i + 1)) as usize;
                let b = be_u16(img, base + self.node_size - 2 * (i + 2)) as usize;
                (base + a, b - a)
            })
            .collect()
    }
    /// Recorre las hojas en orden: (nodo, registros).
    fn leaves(&self, img: &[u8]) -> Vec<u32> {
        let mut out = vec![];
        let mut n = self.header(img).first_leaf_node;
        while n != 0 && out.len() < 100_000 {
            out.push(n);
            n = be_u32(img, self.node(n));
        }
        out
    }
    /// Registros de archivo: (offset absoluto de los datos del registro).
    fn file_records(&self, img: &[u8]) -> Vec<usize> {
        let mut out = vec![];
        for n in self.leaves(img) {
            for (off, _) in self.records(img, n) {
                let kl = be_u16(img, off) as usize;
                let data = off + 2 + kl;
                if be_u16(img, data) == crate::catalog::rtype::FILE && be_u32(img, data + 88 + 12) > 0 {
                    out.push(data);
                }
            }
        }
        out
    }
}

/// Aplica un daño a una imagen generada por [`build_volume`].
pub fn apply_damage(img: &mut [u8], damage: Damage) -> Result<(), String> {
    let mut l = Layout::read(img)?;
    let bs = l.vh.block_size as usize;
    let alloc_off = l.vh.allocation_file.extents[0].start_block as usize * bs;
    let set_bit = |img: &mut [u8], b: u32, on: bool| {
        let byte = &mut img[alloc_off + b as usize / 8];
        if on { *byte |= 0x80 >> (b % 8) } else { *byte &= !(0x80 >> (b % 8)) }
    };
    match damage {
        Damage::KeyOrder => {
            // Cambia la primera letra de un nombre por '~' (ordena después de las letras).
            for n in l.leaves(img) {
                let recs = l.records(img, n);
                if recs.len() < 2 {
                    continue;
                }
                for w in recs.windows(2) {
                    let ((a, _), (b, _)) = (w[0], w[1]);
                    // Mismo padre y ambos con nombre: '~' invierte el orden entre ellos.
                    if be_u32(img, a + 2) == be_u32(img, b + 2) && be_u16(img, a + 6) > 0 && be_u16(img, b + 6) > 0 {
                        put_be_u16(img, a + 8, b'~' as u16);
                        return Ok(());
                    }
                }
            }
            Err("no hay registros adecuados".into())
        }
        Damage::BrokenLink => {
            let leaves = l.leaves(img);
            if leaves.len() < 2 {
                return Err("el catálogo necesita al menos dos hojas".into());
            }
            let total = l.header(img).total_nodes;
            put_be_u32(img, l.node(leaves[0]), total - 1);
            Ok(())
        }
        Damage::NodeUnreadable => {
            let leaves = l.leaves(img);
            let n = *leaves.get(1).ok_or("el catálogo necesita al menos dos hojas")?;
            img[l.node(n) + 8] = 0x55;
            Ok(())
        }
        Damage::ValenceWrong => {
            for n in l.leaves(img) {
                for (off, _) in l.records(img, n) {
                    let data = off + 2 + be_u16(img, off) as usize;
                    if be_u16(img, data) == crate::catalog::rtype::FOLDER && be_u32(img, data + 8) != cnid::ROOT_FOLDER {
                        let v = be_u32(img, data + 4);
                        put_be_u32(img, data + 4, v + 3);
                        return Ok(());
                    }
                }
            }
            Err("no hay carpetas".into())
        }
        Damage::FileCountWrong => {
            l.vh.file_count += 5;
            l.write_vh(img);
            Ok(())
        }
        Damage::NextCnidLow => {
            l.vh.next_catalog_id = cnid::FIRST_USER + 2;
            l.write_vh(img);
            Ok(())
        }
        Damage::ExtentOverlap => {
            let files = l.file_records(img);
            if files.len() < 2 {
                return Err("hacen falta dos archivos con datos".into());
            }
            let start = be_u32(img, files[0] + 88 + 16);
            put_be_u32(img, files[1] + 88 + 16, start);
            Ok(())
        }
        Damage::ExtentOutOfRange => {
            let f = *l.file_records(img).first().ok_or("no hay archivos con datos")?;
            put_be_u32(img, f + 88 + 16, l.vh.total_blocks + 10);
            Ok(())
        }
        Damage::BitmapLeak => {
            let end = l.vh.total_blocks - 64;
            for b in end - 10..end {
                set_bit(img, b, true);
            }
            Ok(())
        }
        Damage::FolderRecordLost | Damage::FolderAndThreadLost => {
            // La carpeta con más hijos (la de fotos en sample_tree).
            let mut best: Option<(usize, u32, u32)> = None; // (offset de datos, cnid, valence)
            for n in l.leaves(img) {
                for (off, _) in l.records(img, n) {
                    let data = off + 2 + be_u16(img, off) as usize;
                    if be_u16(img, data) == crate::catalog::rtype::FOLDER && be_u32(img, data + 8) != cnid::ROOT_FOLDER {
                        let v = be_u32(img, data + 4);
                        if best.is_none_or(|b| v > b.2) {
                            best = Some((data, be_u32(img, data + 8), v));
                        }
                    }
                }
            }
            let (data, id, _) = best.ok_or("no hay carpetas")?;
            put_be_u16(img, data, 0x00FF);
            if damage == Damage::FolderAndThreadLost {
                for n in l.leaves(img) {
                    for (off, _) in l.records(img, n) {
                        let d = off + 2 + be_u16(img, off) as usize;
                        if be_u32(img, off + 2) == id && be_u16(img, off + 6) == 0 {
                            put_be_u16(img, d, 0x00FF);
                        }
                    }
                }
            }
            Ok(())
        }
        Damage::BitmapUnderalloc => {
            let f = *l.file_records(img).first().ok_or("no hay archivos con datos")?;
            set_bit(img, be_u32(img, f + 88 + 16), false);
            Ok(())
        }
    }
}
