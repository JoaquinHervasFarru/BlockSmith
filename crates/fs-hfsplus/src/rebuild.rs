//! Reconstrucción del directorio HFS+/HFSX.
//!
//! A partir del catálogo recuperado por el escaneo se construye un catálogo y un
//! árbol de extents nuevos, compactos y ordenados, **en espacio libre**. La cabecera
//! del volumen se actualiza al final (fase `Commit`) y los bloques viejos se liberan
//! después (fase `Finalize`). Así el mismo plan sirve para exportar a una imagen
//! nueva (F2) y para la escritura directa transaccional (F5).
//!
//! Decisiones de reparación:
//! - Carpetas que faltan pero conservan su thread record: se recrean con su nombre.
//! - Elementos sin padre localizable o en ciclos: a «Recovered Items».
//! - Nombres en conflicto dentro de una carpeta: se renombra el segundo.
//! - Extents fuera del volumen: se truncan (los datos inaccesibles ya estaban perdidos).
//! - Bloques compartidos por dos archivos: el segundo recibe una copia propia.
//! - Recuentos (valence, archivos, carpetas, siguiente CNID, bloques libres) se recalculan.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use blocksmith_fs_core::{BlockWrite, DiffChange, DiffEntry, FsError, FsResult, RebuildPlan, TreeEntry, WritePhase};

use crate::bitmap::AllocationBitmap;
use crate::btree::attr as battr;
use crate::btree::pack::{map_nodes_needed, nodes_needed, pack_btree, tree_nodes, TreeSpec};
use crate::catalog::{
    cnid, CatalogKey, CatalogRecord, FileRecord, FolderRecord, ThreadRecord, FILE_THREAD_EXISTS, FOLDER_HAS_COUNT,
};
use crate::check::HfsScan;
use crate::extents::{ExtentKey, FORK_DATA, FORK_RSRC};
use crate::fixtures::info as default_info;
use crate::unicode::{compare_binary, compare_folded, from_hfs_name, to_hfs_name};
use crate::volume::{full_extents, Volume};
use crate::volume_header::{attr, Extent, ForkData, VolumeHeader};

pub const RECOVERED_FOLDER: &str = "Recovered Items";
/// Segundos entre 1904-01-01 y 1970-01-01.
const HFS_EPOCH_OFFSET: u64 = 2_082_844_800;
/// Códigos que impiden reconstruir con seguridad.
const BLOCKING: &[(&str, &str)] = &[
    ("HFS_JOURNAL_PENDING", "el journal tiene transacciones sin aplicar; monta el volumen una vez en macOS para que se apliquen y vuelve a escanear"),
    ("HFS_VOLUME_TRUNCATED", "el volumen es más grande que el dispositivo o la imagen; parte de los datos no está disponible"),
    ("HFS_BITMAP_TOO_SMALL", "el archivo de asignación no cubre todo el volumen"),
];

fn hfs_now() -> u32 {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    (secs + HFS_EPOCH_OFFSET).min(u32::MAX as u64) as u32
}

#[derive(Debug, Clone)]
enum Body {
    Folder(FolderRecord),
    File(FileRecord),
}

#[derive(Debug, Clone)]
struct Item {
    parent: u32,
    name: Vec<u16>,
    body: Body,
}

impl Item {
    fn is_folder(&self) -> bool {
        matches!(self.body, Body::Folder(_))
    }
}

/// Mapa de asignación con cursor para buscar huecos libres.
struct Allocator {
    /// Todo lo ocupado durante la reconstrucción, incluidas las estructuras viejas.
    used: AllocationBitmap,
    cursor: u32,
    /// Tramos asignados por este plan.
    allocated: Vec<Extent>,
}

impl Allocator {
    /// Reserva `blocks` bloques en como mucho `max_extents` tramos libres.
    fn alloc(&mut self, blocks: u32, max_extents: usize) -> FsResult<Vec<Extent>> {
        let total = self.used.total();
        let mut out = Vec::new();
        let mut need = blocks;
        let mut b = self.cursor;
        let mut scanned = 0u64;
        while need > 0 {
            if scanned > total as u64 * 2 {
                return Err(FsError::Invalid("no queda espacio libre suficiente para las estructuras nuevas".into()));
            }
            if b >= total {
                b = 0;
            }
            if self.used.is_set(b) {
                b += 1;
                scanned += 1;
                continue;
            }
            let start = b;
            while b < total && !self.used.is_set(b) && b - start < need {
                b += 1;
            }
            let run = b - start;
            scanned += run as u64;
            out.push(Extent { start_block: start, block_count: run });
            need -= run;
            if out.len() > max_extents {
                return Err(FsError::Invalid(
                    "el espacio libre está demasiado fragmentado para las estructuras nuevas".into(),
                ));
            }
        }
        for e in &out {
            self.used.set_range(e.start_block, e.block_count);
        }
        self.allocated.extend(out.iter().copied());
        self.cursor = b;
        Ok(out)
    }
}

fn fork_from(extents: &[Extent], logical: u64, clump: u32) -> ForkData {
    let mut f = ForkData { logical_size: logical, clump_size: clump, ..Default::default() };
    for (i, e) in extents.iter().take(8).enumerate() {
        f.extents[i] = *e;
    }
    f.total_blocks = extents.iter().map(|e| e.block_count).sum();
    f
}

/// Escrituras que colocan `bytes` en los bloques de `extents` (offsets relativos al volumen).
fn writes_for(extents: &[Extent], bs: u64, bytes: &[u8], phase: WritePhase, label: &str) -> Vec<BlockWrite> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    for e in extents {
        if pos >= bytes.len() {
            break;
        }
        let len = ((e.block_count as u64 * bs) as usize).min(bytes.len() - pos);
        out.push(BlockWrite {
            offset: e.start_block as u64 * bs,
            data: bytes[pos..pos + len].to_vec(),
            phase,
            label: label.into(),
        });
        pos += len;
    }
    out
}

fn bitmap_bytes(map: &AllocationBitmap, file_len: usize) -> Vec<u8> {
    let mut b = map.as_bytes().to_vec();
    b.resize(file_len, 0);
    b
}

/// Calcula el plan de reconstrucción. No escribe nada.
pub fn plan_rebuild(vol: &Volume<'_>, scan: &HfsScan) -> FsResult<RebuildPlan> {
    for (code, why) in BLOCKING {
        if scan.result.findings.iter().any(|f| f.code == *code) {
            return Err(FsError::Invalid(format!("no se puede reconstruir: {why}")));
        }
    }
    let vh = &vol.vh;
    let bs = vh.block_size as u64;
    let total = vh.total_blocks;
    let cat = &scan.catalog;
    let binary = scan.result.info.case_sensitive;
    let cmp_names = |a: &[u16], b: &[u16]| if binary { compare_binary(a, b) } else { compare_folded(a, b) };
    let now = hfs_now();
    let mut diff: Vec<DiffEntry> = Vec::new();
    let mut lost = 0u64;

    // ── 1. Modelo de trabajo ────────────────────────────────────────────────
    let mut items: BTreeMap<u32, Item> = BTreeMap::new();
    for (id, (k, r)) in &cat.folders {
        items.insert(*id, Item { parent: k.parent_id, name: k.name.clone(), body: Body::Folder(r.clone()) });
    }
    for (id, (k, r)) in &cat.files {
        items.insert(*id, Item { parent: k.parent_id, name: k.name.clone(), body: Body::File(r.clone()) });
    }
    let old_path = |id: u32| cat.path_of(id);
    let mut next_id = items.keys().copied().max().unwrap_or(0).max(vh.next_catalog_id.saturating_sub(1)) + 1;
    let new_folder = |id: u32, now: u32| {
        let mut info = default_info(0o040755, 0);
        info.create_date = now;
        info.content_mod_date = now;
        info.attribute_mod_date = now;
        info.access_date = now;
        FolderRecord { valence: 0, folder_id: id, info, folder_count: 0 }
    };

    // Carpeta raíz
    match items.get_mut(&cnid::ROOT_FOLDER) {
        Some(root) if root.is_folder() => root.parent = cnid::ROOT_PARENT,
        _ => {
            let name = cat
                .threads
                .get(&cnid::ROOT_FOLDER)
                .map(|(_, t)| t.name.clone())
                .unwrap_or_else(|| to_hfs_name("Sin título"));
            items.insert(
                cnid::ROOT_FOLDER,
                Item {
                    parent: cnid::ROOT_PARENT,
                    name: name.clone(),
                    body: Body::Folder(new_folder(cnid::ROOT_FOLDER, now)),
                },
            );
            diff.push(DiffEntry {
                object_id: cnid::ROOT_FOLDER as u64,
                change: DiffChange::Recovered,
                old_path: None,
                new_path: Some("/".into()),
                is_folder: true,
                size_bytes: 0,
                note: Some(format!("carpeta raíz «{}» recreada", from_hfs_name(&name))),
            });
        }
    }

    // ── 2. Carpetas que faltan, recreadas desde su thread record ────────────
    for _ in 0..64 {
        let missing: Vec<u32> = items
            .iter()
            .filter(|(id, it)| **id != cnid::ROOT_FOLDER && !items.get(&it.parent).is_some_and(Item::is_folder))
            .map(|(_, it)| it.parent)
            .collect();
        let mut created = false;
        for p in missing {
            if items.contains_key(&p) || p < cnid::FIRST_USER {
                continue;
            }
            if let Some((true, t)) = cat.threads.get(&p) {
                items.insert(
                    p,
                    Item { parent: t.parent_id, name: t.name.clone(), body: Body::Folder(new_folder(p, now)) },
                );
                diff.push(DiffEntry {
                    object_id: p as u64,
                    change: DiffChange::Recovered,
                    old_path: None,
                    new_path: None,
                    is_folder: true,
                    size_bytes: 0,
                    note: Some("carpeta recreada a partir de su thread record".into()),
                });
                created = true;
            }
        }
        if !created {
            break;
        }
    }

    // ── 3. Huérfanos y ciclos → «Recovered Items» ───────────────────────────
    let reaches_root = |items: &BTreeMap<u32, Item>, mut id: u32| {
        for _ in 0..4096 {
            if id == cnid::ROOT_FOLDER {
                return true;
            }
            match items.get(&id) {
                Some(it) if items.get(&it.parent).is_some_and(Item::is_folder) || it.parent == cnid::ROOT_FOLDER => {
                    id = it.parent
                }
                _ => return false,
            }
        }
        false
    };
    let stranded: Vec<u32> =
        items.keys().copied().filter(|id| *id != cnid::ROOT_FOLDER && !reaches_root(&items, *id)).collect();
    // Solo se mueven los elementos «cabeza» (cuyo padre no está también varado), conservando sus subárboles.
    let stranded_set: std::collections::HashSet<u32> = stranded.iter().copied().collect();
    let heads: Vec<u32> = stranded
        .iter()
        .copied()
        .filter(|id| {
            let p = items[id].parent;
            !stranded_set.contains(&p) || !items.get(&p).is_some_and(Item::is_folder)
        })
        .collect();
    // Los ciclos no tienen cabeza: se rompen moviendo su CNID menor.
    let mut heads = heads;
    for id in &stranded {
        if !heads.contains(id) && !heads.iter().any(|h| is_ancestor(&items, *h, *id)) {
            heads.push(*id);
        }
    }
    if !heads.is_empty() {
        let rec_id = next_id;
        next_id += 1;
        let base = to_hfs_name(RECOVERED_FOLDER);
        let mut name = base.clone();
        let mut n = 2;
        while items.values().any(|it| it.parent == cnid::ROOT_FOLDER && cmp_names(&it.name, &name) == Ordering::Equal) {
            name = to_hfs_name(&format!("{RECOVERED_FOLDER} {n}"));
            n += 1;
        }
        items.insert(rec_id, Item { parent: cnid::ROOT_FOLDER, name, body: Body::Folder(new_folder(rec_id, now)) });
        for id in heads {
            let path = old_path(id).unwrap_or_else(|| format!("…/{} (CNID {id})", from_hfs_name(&items[&id].name)));
            let it = items.get_mut(&id).expect("existe");
            it.parent = rec_id;
            diff.push(DiffEntry {
                object_id: id as u64,
                change: DiffChange::MovedToRecovered,
                old_path: Some(path),
                new_path: None,
                is_folder: it.is_folder(),
                size_bytes: file_size(it),
                note: None,
            });
        }
    }

    // ── 4. Nombres no válidos o en conflicto ────────────────────────────────
    let mut by_parent: HashMap<u32, Vec<u32>> = HashMap::new();
    for (id, it) in &items {
        if *id != cnid::ROOT_FOLDER {
            by_parent.entry(it.parent).or_default().push(*id);
        }
    }
    for children in by_parent.values_mut() {
        children.sort_by(|a, b| cmp_names(&items[a].name, &items[b].name).then(a.cmp(b)));
        let mut taken: Vec<Vec<u16>> = Vec::new();
        for id in children.iter() {
            let original = items[id].name.clone();
            let mut name = original.clone();
            if name.is_empty() || name.len() > 255 {
                name = to_hfs_name(&format!("Sin nombre {id}"));
            }
            let mut n = 2;
            while taken.iter().any(|t| cmp_names(t, &name) == Ordering::Equal) {
                name = with_suffix(&original, n);
                n += 1;
            }
            if name != original {
                diff.push(DiffEntry {
                    object_id: *id as u64,
                    change: DiffChange::Renamed,
                    old_path: old_path(*id),
                    new_path: None,
                    is_folder: items[id].is_folder(),
                    size_bytes: file_size(&items[id]),
                    note: Some(format!("«{}» → «{}»", from_hfs_name(&original), from_hfs_name(&name))),
                });
                items.get_mut(id).expect("existe").name = name.clone();
            }
            taken.push(name);
        }
    }

    // ── 5. Asignación: estructuras que se conservan y datos de archivos ─────
    let mut used = AllocationBitmap::new_empty(total);
    let head = 1536u64.div_ceil(bs) as u32;
    used.set_range(0, head);
    let tail = ((vh.volume_bytes().saturating_sub(1024)) / bs) as u32;
    used.set_range(tail, total.saturating_sub(tail));
    let in_range = |e: &Extent| e.start_block as u64 + e.block_count as u64 <= total as u64;
    let overflow = &scan.overflow;
    let keep_fork = |fork: &ForkData, id: u32, used: &mut AllocationBitmap| {
        for e in full_extents(fork, id, FORK_DATA, overflow).iter().filter(|e| in_range(e)) {
            used.set_range(e.start_block, e.block_count);
        }
    };
    keep_fork(&vh.allocation_file, cnid::ALLOCATION_FILE, &mut used);
    keep_fork(&vh.attributes_file, cnid::ATTRIBUTES_FILE, &mut used);
    keep_fork(&vh.startup_file, cnid::STARTUP_FILE, &mut used);
    // Bloques defectuosos: solo existen como extents del fileID 5.
    for (_, exts) in overflow.get(&(cnid::BAD_BLOCKS, FORK_DATA)).into_iter().flatten() {
        for e in exts.iter().filter(|e| e.block_count > 0 && in_range(e)) {
            used.set_range(e.start_block, e.block_count);
        }
    }
    // Estructuras viejas: siguen en uso hasta la fase de finalización.
    let mut old_structs: Vec<Extent> = Vec::new();
    for (fork, id) in [(&vh.catalog_file, cnid::CATALOG_FILE), (&vh.extents_file, cnid::EXTENTS_FILE)] {
        old_structs.extend(full_extents(fork, id, FORK_DATA, overflow).into_iter().filter(|e| in_range(e)));
    }

    // Archivos: truncado de extents inválidos y detección de bloques compartidos.
    let mut clones: Vec<(u32, u8, usize, Extent)> = Vec::new(); // (cnid, fork, índice de extent, extent original)
                                                                // Tramos de archivos ya asignados, para saber con quién se comparte un bloque.
    let mut owners: Vec<(u32, u32, u32)> = Vec::new(); // (inicio, fin exclusivo, cnid)
    let mut shared_with: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut file_extents: HashMap<(u32, u8), Vec<Extent>> = HashMap::new();
    let ids: Vec<u32> = items.keys().copied().collect();
    for id in &ids {
        let Body::File(rec) = &items[id].body else { continue };
        let rec = rec.clone();
        for (fork, ft) in [(&rec.data_fork, FORK_DATA), (&rec.rsrc_fork, FORK_RSRC)] {
            let all = full_extents(fork, *id, ft, overflow);
            let mut kept: Vec<Extent> = Vec::new();
            let mut covered = 0u64;
            for e in &all {
                if !in_range(e) || covered >= fork.total_blocks as u64 {
                    break;
                }
                let take = (e.block_count as u64).min(fork.total_blocks as u64 - covered) as u32;
                kept.push(Extent { start_block: e.start_block, block_count: take });
                covered += take as u64;
            }
            let kept_bytes = covered * bs;
            if covered < fork.total_blocks as u64 || fork.logical_size > kept_bytes {
                let lost_bytes = fork.logical_size.saturating_sub(kept_bytes);
                diff.push(DiffEntry {
                    object_id: *id as u64,
                    change: DiffChange::DataTruncated,
                    old_path: old_path(*id),
                    new_path: None,
                    is_folder: false,
                    size_bytes: fork.logical_size,
                    note: Some(format!(
                        "{}: se pierden {lost_bytes} bytes cuyos bloques estaban fuera del volumen o no tenían extents",
                        if ft == FORK_DATA { "datos" } else { "fork de recursos" }
                    )),
                });
            }
            for (i, e) in kept.iter().enumerate() {
                let shared = (e.start_block..e.start_block + e.block_count).any(|b| used.is_set(b));
                if shared {
                    let end = e.start_block + e.block_count;
                    let others: Vec<u32> =
                        owners.iter().filter(|o| o.0 < end && e.start_block < o.1).map(|o| o.2).collect();
                    for o in &others {
                        shared_with.entry(*o).or_default().push(*id);
                    }
                    clones.push((*id, ft, i, *e));
                } else {
                    used.set_range(e.start_block, e.block_count);
                    owners.push((e.start_block, e.start_block + e.block_count, *id));
                }
            }
            file_extents.insert((*id, ft), kept);
        }
    }

    // `claimed`: lo que se conserva tras la reconstrucción. El asignador reserva además
    // las estructuras viejas, que siguen en uso hasta la fase de finalización.
    let claimed = used.clone();
    let mut reserve = used;
    for e in &old_structs {
        reserve.set_range(e.start_block, e.block_count);
    }
    let mut alloc = Allocator { used: reserve, cursor: vh.next_allocation.min(total), allocated: Vec::new() };
    let mut data_writes: Vec<BlockWrite> = Vec::new();
    for (id, ft, i, e) in clones {
        let new = alloc.alloc(e.block_count, 1)?[0];
        let bytes = vol.dev.read_vec(vol.offset + e.start_block as u64 * bs, (e.block_count as u64 * bs) as usize)?;
        data_writes.push(BlockWrite {
            offset: new.start_block as u64 * bs,
            data: bytes,
            phase: WritePhase::Data,
            label: format!("copia de datos del CNID {id}"),
        });
        file_extents.get_mut(&(id, ft)).expect("existe")[i] = new;
        diff.push(DiffEntry {
            object_id: id as u64,
            change: DiffChange::DataCloned,
            old_path: old_path(id),
            new_path: None,
            is_folder: false,
            size_bytes: e.block_count as u64 * bs,
            note: Some(format!(
                "los bloques {}–{} los compartía con otro archivo; se copiaron a bloques propios. Revisa que su contenido sea el esperado",
                e.start_block,
                e.start_block + e.block_count - 1
            )),
        });
    }

    // El archivo que conserva los bloques compartidos también se marca: no se puede
    // saber cuál de los dos tenía los datos legítimos.
    for (owner, others) in &shared_with {
        diff.push(DiffEntry {
            object_id: *owner as u64,
            change: DiffChange::UnchangedFlagged,
            old_path: old_path(*owner),
            new_path: None,
            is_folder: false,
            size_bytes: items.get(owner).map(file_size).unwrap_or(0),
            note: Some(format!(
                "compartía bloques con {} otro(s) archivo(s) (CNID {}); conserva los bloques originales, pero revisa su contenido",
                others.len(),
                others.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
            )),
        });
    }

    // Forks finales y registros de desbordamiento de usuario.
    let mut ext_records: Vec<(ExtentKey, [Extent; 8])> = Vec::new();
    for id in &ids {
        let Some(item) = items.get_mut(id) else { continue };
        let Body::File(rec) = &mut item.body else { continue };
        for ft in [FORK_DATA, FORK_RSRC] {
            let exts = file_extents.remove(&(*id, ft)).unwrap_or_default();
            let fork = if ft == FORK_DATA { &mut rec.data_fork } else { &mut rec.rsrc_fork };
            let blocks: u64 = exts.iter().map(|e| e.block_count as u64).sum();
            let logical = fork.logical_size.min(blocks * bs);
            *fork = fork_from(&exts, logical, fork.clump_size);
            let mut fork_block = exts.iter().take(8).map(|e| e.block_count).sum::<u32>();
            for chunk in exts.get(8..).unwrap_or(&[]).chunks(8) {
                let mut rec8 = [Extent::default(); 8];
                rec8[..chunk.len()].copy_from_slice(chunk);
                ext_records.push((ExtentKey { fork_type: ft, file_id: *id, start_block: fork_block }, rec8));
                fork_block += chunk.iter().map(|e| e.block_count).sum::<u32>();
            }
        }
        rec.info.flags |= FILE_THREAD_EXISTS;
    }
    // Desbordamiento de archivos especiales que se conservan (atributos, arranque, bloques defectuosos).
    for special in [cnid::BAD_BLOCKS, cnid::STARTUP_FILE, cnid::ATTRIBUTES_FILE] {
        for ft in [FORK_DATA, FORK_RSRC] {
            for (start, exts) in overflow.get(&(special, ft)).into_iter().flatten() {
                ext_records.push((ExtentKey { fork_type: ft, file_id: special, start_block: *start }, *exts));
            }
        }
    }
    ext_records.sort_by(|a, b| a.0.compare(&b.0));
    ext_records.dedup_by(|a, b| a.0.compare(&b.0) == Ordering::Equal);

    // ── 6. Recuentos ────────────────────────────────────────────────────────
    let mut valence: HashMap<u32, u32> = HashMap::new();
    let mut subfolders: HashMap<u32, u32> = HashMap::new();
    for (id, it) in &items {
        if *id != cnid::ROOT_FOLDER {
            *valence.entry(it.parent).or_default() += 1;
            if it.is_folder() {
                *subfolders.entry(it.parent).or_default() += 1;
            }
        }
    }
    let original_valence: HashMap<u32, u32> = cat.folders.iter().map(|(id, (_, r))| (*id, r.valence)).collect();
    for (id, it) in items.iter_mut() {
        if let Body::Folder(f) = &mut it.body {
            let v = valence.get(id).copied().unwrap_or(0);
            if original_valence.get(id).is_some_and(|old| *old != v) {
                diff.push(DiffEntry {
                    object_id: *id as u64,
                    change: DiffChange::ValenceFixed,
                    old_path: cat.path_of(*id),
                    new_path: None,
                    is_folder: true,
                    size_bytes: 0,
                    note: Some(format!("{} → {v} elementos", f.valence)),
                });
            }
            f.valence = v;
            if f.info.flags & FOLDER_HAS_COUNT != 0 {
                f.folder_count = subfolders.get(id).copied().unwrap_or(0);
            }
        }
    }

    // Elementos solo encontrados por barrido: recuperados.
    for id in &cat.swept {
        if items.contains_key(id) && !diff.iter().any(|d| d.object_id == *id as u64) {
            diff.push(DiffEntry {
                object_id: *id as u64,
                change: DiffChange::Recovered,
                old_path: None,
                new_path: None,
                is_folder: items[id].is_folder(),
                size_bytes: file_size(&items[id]),
                note: Some("registro encontrado fuera de la cadena del directorio".into()),
            });
        }
    }
    // Threads cuyo elemento no se pudo recuperar: solo queda el nombre.
    for (id, (is_folder, t)) in &cat.threads {
        if !items.contains_key(id) {
            lost += 1;
            diff.push(DiffEntry {
                object_id: *id as u64,
                change: DiffChange::Dropped,
                old_path: Some(format!("…/{}", from_hfs_name(&t.name))),
                new_path: None,
                is_folder: *is_folder,
                size_bytes: 0,
                note: Some("solo se conservaba su thread record; el registro principal era ilegible".into()),
            });
        }
    }
    if scan.dropped_records > 0 {
        diff.push(DiffEntry {
            object_id: 0,
            change: DiffChange::Dropped,
            old_path: None,
            new_path: None,
            is_folder: false,
            size_bytes: 0,
            note: Some(format!("{} registros ilegibles descartados", scan.dropped_records)),
        });
    }

    // ── 7. Catálogo nuevo ───────────────────────────────────────────────────
    let mut records: Vec<(CatalogKey, CatalogRecord)> = Vec::with_capacity(items.len() * 2);
    for (id, it) in &items {
        let key = CatalogKey { parent_id: it.parent, name: it.name.clone() };
        let thread = ThreadRecord { parent_id: it.parent, name: it.name.clone() };
        match &it.body {
            Body::Folder(f) => {
                records.push((key, CatalogRecord::Folder(f.clone())));
                records.push((CatalogKey { parent_id: *id, name: vec![] }, CatalogRecord::FolderThread(thread)));
            }
            Body::File(f) => {
                records.push((key, CatalogRecord::File(f.clone())));
                records.push((CatalogKey { parent_id: *id, name: vec![] }, CatalogRecord::FileThread(thread)));
            }
        }
    }
    records.sort_by(|a, b| a.0.compare(&b.0, binary));
    let cat_raw: Vec<(Vec<u8>, Vec<u8>)> = records.iter().map(|(k, r)| (k.to_bytes(), r.to_bytes())).collect();
    let cat_spec = |total_nodes| TreeSpec {
        node_size: scan.catalog_node_size,
        max_key_length: 516,
        attributes: battr::BIG_KEYS | battr::VARIABLE_INDEX_KEYS,
        key_compare_type: if binary { 0xBC } else { 0xCF },
        total_nodes,
        max_records_per_leaf: None,
    };
    let (cat_nodes, cat_ext) = build_tree(&cat_raw, cat_spec, bs, 32, &mut alloc)?;

    let ext_raw: Vec<(Vec<u8>, Vec<u8>)> = ext_records
        .iter()
        .map(|(k, e)| {
            let mut d = vec![0u8; 64];
            for (i, x) in e.iter().enumerate() {
                d[i * 8..i * 8 + 4].copy_from_slice(&x.start_block.to_be_bytes());
                d[i * 8 + 4..i * 8 + 8].copy_from_slice(&x.block_count.to_be_bytes());
            }
            (k.to_bytes().to_vec(), d)
        })
        .collect();
    let ext_spec = |total_nodes| TreeSpec {
        node_size: scan.extents_node_size,
        max_key_length: 10,
        attributes: battr::BIG_KEYS,
        key_compare_type: 0,
        total_nodes,
        max_records_per_leaf: None,
    };
    let (_, ext_ext) = build_tree(&ext_raw, ext_spec, bs, 8, &mut alloc)?;

    // ── 8. Bitmap y cabecera ────────────────────────────────────────────────
    let alloc_exts = full_extents(&vh.allocation_file, cnid::ALLOCATION_FILE, FORK_DATA, overflow);
    let alloc_len = (vh.allocation_file.total_blocks as u64 * bs) as usize;
    let during = alloc.used.clone();
    let mut final_map = claimed;
    for e in &alloc.allocated {
        final_map.set_range(e.start_block, e.block_count);
    }

    let files = items.values().filter(|i| !i.is_folder()).count() as u32;
    let folders = items.values().filter(|i| i.is_folder()).count() as u32 - 1;
    let max_id = items.keys().copied().max().unwrap_or(cnid::FIRST_USER);
    let mut new_vh: VolumeHeader = vh.clone();
    new_vh.catalog_file =
        fork_from(&cat_ext.0, cat_nodes as u64 * scan.catalog_node_size as u64, vh.catalog_file.clump_size);
    new_vh.extents_file =
        fork_from(&ext_ext.0, ext_ext.1 as u64 * scan.extents_node_size as u64, vh.extents_file.clump_size);
    new_vh.file_count = files;
    new_vh.folder_count = folders;
    new_vh.next_catalog_id = vh.next_catalog_id.max(max_id + 1).max(next_id);
    new_vh.free_blocks = total - final_map.count_set();
    new_vh.attributes = (vh.attributes | attr::UNMOUNTED) & !attr::INCONSISTENT;
    new_vh.write_count = vh.write_count.wrapping_add(1);
    new_vh.modify_date = now;
    new_vh.checked_date = now;
    let vh_bytes = new_vh.to_bytes().to_vec();

    let mut writes = data_writes;
    writes.extend(writes_for(&ext_ext.0, bs, &ext_ext.2, WritePhase::Data, "árbol de extents nuevo"));
    writes.extend(writes_for(&cat_ext.0, bs, &cat_ext.2, WritePhase::Data, "catálogo nuevo"));
    writes.extend(writes_for(
        &alloc_exts,
        bs,
        &bitmap_bytes(&during, alloc_len),
        WritePhase::Data,
        "bitmap (transición)",
    ));
    writes.push(BlockWrite {
        offset: 1024,
        data: vh_bytes.clone(),
        phase: WritePhase::Commit,
        label: "Volume Header".into(),
    });
    writes.push(BlockWrite {
        offset: vh.volume_bytes() - 1024,
        data: vh_bytes,
        phase: WritePhase::Commit,
        label: "Volume Header alternativo".into(),
    });
    writes.extend(writes_for(
        &alloc_exts,
        bs,
        &bitmap_bytes(&final_map, alloc_len),
        WritePhase::Finalize,
        "bitmap final",
    ));

    let recovered =
        diff.iter().filter(|d| matches!(d.change, DiffChange::Recovered | DiffChange::MovedToRecovered)).count() as u64;
    let nodes_after = nodes_needed(&cat_raw, &cat_spec(cat_nodes)).map_err(FsError::Invalid)? as u64;
    // Rutas nuevas para el informe.
    let new_items = &items;
    for d in &mut diff {
        let id = d.object_id as u32;
        if new_items.contains_key(&id) {
            d.new_path = Some(path_in(new_items, id));
        }
    }
    let tree_after = items
        .iter()
        .map(|(id, it)| TreeEntry {
            id: *id as u64,
            parent: it.parent as u64,
            name: from_hfs_name(&it.name),
            is_folder: it.is_folder(),
            size_bytes: file_size(it),
        })
        .collect();
    Ok(RebuildPlan {
        writes,
        diff,
        nodes_before: scan.catalog_nodes_used,
        nodes_after,
        recovered_count: recovered,
        lost_count: lost,
        experimental: false,
        tree_after,
        root_id: cnid::ROOT_FOLDER as u64,
    })
}

/// (extents asignados, nodos totales, bytes del árbol)
type TreeExt = (Vec<Extent>, u32, Vec<u8>);

/// Empaqueta un árbol con margen de crecimiento y le reserva espacio libre.
/// Devuelve (bytes de los nodos, nodos totales, extents + nodos + bytes).
fn build_tree(
    records: &[(Vec<u8>, Vec<u8>)],
    spec: impl Fn(u32) -> TreeSpec,
    bs: u64,
    min_nodes: u32,
    alloc: &mut Allocator,
) -> FsResult<(u32, TreeExt)> {
    let ns = spec(0).node_size as u64;
    let per_block = (bs / ns).max(1) as u32;
    let blocks_per_node = (ns / bs).max(1) as u32;
    let base = tree_nodes(records, &spec(0)).map_err(FsError::Invalid)?;
    let round = |n: u32| n.div_ceil(per_block) * per_block;
    // 25 % de margen para que el volumen pueda crecer sin reorganizar enseguida.
    let mut total = round((base + base / 4 + 8).max(min_nodes));
    while base + map_nodes_needed(ns as usize, total) > total {
        total = round(total + per_block);
    }
    let (nodes, _) = pack_btree(records, &spec(total)).map_err(FsError::Invalid)?;
    let blocks = (total / per_block) * blocks_per_node;
    let exts = alloc.alloc(blocks, 8)?;
    let bytes = nodes.concat();
    Ok((total, (exts.clone(), total, bytes)))
}

fn file_size(it: &Item) -> u64 {
    match &it.body {
        Body::File(f) => f.data_fork.logical_size + f.rsrc_fork.logical_size,
        Body::Folder(_) => 0,
    }
}

fn is_ancestor(items: &BTreeMap<u32, Item>, ancestor: u32, mut id: u32) -> bool {
    for _ in 0..4096 {
        match items.get(&id) {
            Some(it) if it.parent == ancestor => return true,
            Some(it) => id = it.parent,
            None => return false,
        }
    }
    false
}

fn path_in(items: &BTreeMap<u32, Item>, mut id: u32) -> String {
    let mut parts = Vec::new();
    for _ in 0..4096 {
        if id == cnid::ROOT_FOLDER {
            break;
        }
        let Some(it) = items.get(&id) else { break };
        parts.push(from_hfs_name(&it.name));
        id = it.parent;
    }
    parts.reverse();
    format!("/{}", parts.join("/"))
}

/// Añade « 2», « 3»… antes de la extensión.
fn with_suffix(name: &[u16], n: u32) -> Vec<u16> {
    let s = from_hfs_name(name);
    let s = if s.is_empty() { "Sin nombre".to_string() } else { s };
    let (stem, ext) = match s.rfind('.') {
        Some(i) if i > 0 => (&s[..i], &s[i..]),
        _ => (s.as_str(), ""),
    };
    to_hfs_name(&format!("{stem} {n}{ext}"))
}
