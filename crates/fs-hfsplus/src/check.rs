//! Escaneo de solo lectura de un volumen HFS+/HFSX.
//!
//! Fases: cabecera → journal → Extents Overflow → catálogo (estructura) →
//! catálogo (semántica) → extents y bitmap de asignación.
//!
//! Los registros se recogen de **todos** los nodos hoja válidos, estén o no
//! enlazados, para que un enlace roto no oculte datos (barrido).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use blocksmith_fs_core::{
    Finding, FsError, FsInfo, FsKind, FsResult, ObjectType, Progress, ScanResult, ScanStats, Severity,
};
use blocksmith_io::endian::{be_u32, be_u64};
use serde::Serialize;
use serde_json::json;

use crate::bitmap::AllocationBitmap;
use crate::btree::{kind, split_record, BTree, Node};
use crate::catalog::{cnid, CatalogKey, CatalogRecord, FileRecord, FolderRecord, ThreadRecord, FILE_THREAD_EXISTS};
use crate::extents::{parse_record, ExtentKey, FORK_DATA, FORK_RSRC};
use crate::unicode::from_hfs_name;
use crate::volume::{full_extents, OverflowMap, Volume};
use crate::volume_header::{attr, ForkData};

/// Máximo de hallazgos individuales por código; el resto se resume.
const MAX_PER_CODE: usize = 200;
const KEY_COMPARE_BINARY: u8 = 0xBC;

/// Catálogo reconstruido a partir de los registros encontrados.
#[derive(Debug, Default, Clone, Serialize)]
pub struct CatalogSnapshot {
    pub folders: BTreeMap<u32, (CatalogKey, FolderRecord)>,
    pub files: BTreeMap<u32, (CatalogKey, FileRecord)>,
    /// cnid → (es carpeta, thread)
    pub threads: BTreeMap<u32, (bool, ThreadRecord)>,
    /// CNIDs cuyos registros solo se encontraron por barrido (hojas no enlazadas).
    pub swept: BTreeSet<u32>,
}

impl CatalogSnapshot {
    /// Ruta legible de un CNID (carpeta o archivo), si su cadena de padres es completa.
    pub fn path_of(&self, id: u32) -> Option<String> {
        let mut parts = Vec::new();
        let mut cur = id;
        for _ in 0..512 {
            if cur == cnid::ROOT_FOLDER {
                parts.reverse();
                return Some(format!("/{}", parts.join("/")));
            }
            let key = self.folders.get(&cur).map(|f| &f.0).or_else(|| self.files.get(&cur).map(|f| &f.0))?;
            parts.push(from_hfs_name(&key.name));
            cur = key.parent_id;
        }
        None
    }

    /// Árbol plano (carpetas y archivos) para las vistas de comparación.
    pub fn entries(&self) -> Vec<blocksmith_fs_core::TreeEntry> {
        let folders = self.folders.iter().map(|(id, (k, _))| (id, k, true, 0u64));
        let files =
            self.files.iter().map(|(id, (k, r))| (id, k, false, r.data_fork.logical_size + r.rsrc_fork.logical_size));
        folders
            .chain(files)
            .map(|(id, k, is_folder, size_bytes)| blocksmith_fs_core::TreeEntry {
                id: *id as u64,
                parent: k.parent_id as u64,
                name: from_hfs_name(&k.name),
                is_folder,
                size_bytes,
            })
            .collect()
    }

    fn label(&self, id: u32, key: &CatalogKey) -> String {
        self.path_of(id).unwrap_or_else(|| format!("…/{} (CNID {id})", from_hfs_name(&key.name)))
    }
}

pub struct HfsScan {
    pub result: ScanResult,
    pub catalog: CatalogSnapshot,
    /// Extents de desbordamiento válidos, por (fileID, tipo de fork).
    pub overflow: OverflowMap,
    /// Nodos del catálogo marcados como asignados en su mapa (incluye la cabecera).
    pub catalog_nodes_used: u64,
    pub catalog_node_size: u16,
    pub extents_node_size: u16,
    /// Registros del catálogo descartados por ilegibles.
    pub dropped_records: u64,
}

#[derive(Default)]
struct Findings {
    list: Vec<Finding>,
    per_code: HashMap<String, usize>,
    suppressed: HashMap<String, usize>,
}

impl Findings {
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        code: &str,
        severity: Severity,
        object_type: ObjectType,
        object_id: Option<u64>,
        node: Option<u32>,
        path: Option<String>,
        message: String,
        details: serde_json::Value,
    ) {
        let n = self.per_code.entry(code.to_string()).or_default();
        *n += 1;
        if *n > MAX_PER_CODE {
            *self.suppressed.entry(code.to_string()).or_default() += 1;
            return;
        }
        self.list.push(Finding {
            code: code.into(),
            severity,
            object_type,
            object_id,
            node_number: node.map(u64::from),
            path,
            message,
            details,
        });
    }

    fn simple(&mut self, code: &str, severity: Severity, object_type: ObjectType, message: String) {
        self.push(code, severity, object_type, None, None, None, message, json!({}));
    }

    fn finish(mut self) -> Vec<Finding> {
        for (code, n) in self.suppressed {
            self.list.push(Finding {
                code: code.clone(),
                severity: Severity::Minor,
                object_type: ObjectType::Header,
                object_id: None,
                node_number: None,
                path: None,
                message: format!("{n} hallazgos más con el código {code} no se listan individualmente"),
                details: json!({ "suppressed": n }),
            });
        }
        self.list.sort_by_key(|f| std::cmp::Reverse(f.severity));
        self.list
    }
}

/// Resultado del recorrido estructural de un B-tree.
#[derive(Default)]
struct TreeScan {
    /// (clave sin longitud, datos, nodo, enlazado) en orden de la cadena de hojas
    /// y después los de hojas no enlazadas.
    records: Vec<RawRecord>,
    total_nodes: u64,
    allocated_nodes: u64,
    damaged_nodes: u64,
    leaf_nodes: u64,
    leaf_used_bytes: u64,
}

/// (clave sin longitud, datos, nodo, alcanzable por la cadena de hojas)
type RawRecord = (Vec<u8>, Vec<u8>, u32, bool);

type KeyCmp<'c> = &'c dyn Fn(&[u8], &[u8]) -> Ordering;

fn check_cancel(progress: &dyn Progress) -> FsResult<()> {
    if progress.is_cancelled() {
        Err(FsError::Cancelled)
    } else {
        Ok(())
    }
}

fn scan_tree(
    name: &str,
    tree: &BTree<'_>,
    cmp: KeyCmp<'_>,
    f: &mut Findings,
    progress: &mut dyn Progress,
) -> FsResult<TreeScan> {
    let h = tree.header.clone();
    let ns = tree.node_size() as u64;
    let mut out = TreeScan { total_nodes: h.total_nodes as u64, ..Default::default() };
    let tree_details = |extra: serde_json::Value| {
        let mut d = json!({ "tree": name });
        if let (Some(d), Some(e)) = (d.as_object_mut(), extra.as_object()) {
            d.extend(e.clone());
        }
        d
    };
    for w in &tree.map_warnings {
        f.push(
            "HFS_BTREE_MAP",
            Severity::Minor,
            ObjectType::Node,
            None,
            None,
            None,
            format!("Árbol {name}: {w}"),
            tree_details(json!({})),
        );
    }

    if h.total_nodes as u64 * ns > tree.reader.logical_size() {
        f.push(
            "HFS_BTREE_HEADER",
            Severity::Critical,
            ObjectType::Header,
            None,
            Some(0),
            None,
            format!(
                "Árbol {name}: la cabecera declara {} nodos y el archivo solo tiene espacio para {}",
                h.total_nodes,
                tree.reader.logical_size() / ns
            ),
            tree_details(json!({})),
        );
    }

    // 1) Leer y validar todos los nodos asignados.
    let mut nodes: HashMap<u32, Node> = HashMap::new();
    let mut rescued_nodes: HashSet<u32> = HashSet::new();
    let mut allocated = 1u32; // el nodo cabecera
    let limit = h.total_nodes.min((tree.reader.logical_size() / ns) as u32);
    for n in 1..limit {
        if n % 512 == 0 {
            progress.advance(n as u64, limit as u64);
            check_cancel(progress)?;
        }
        if !tree.is_allocated(n) {
            continue;
        }
        allocated += 1;
        match tree.read_node(n) {
            Ok(Ok(node)) => {
                if matches!(node.desc.kind, kind::LEAF | kind::INDEX) {
                    nodes.insert(n, node);
                }
            }
            Ok(Err(e)) => {
                out.damaged_nodes += 1;
                // Si solo está dañado el tipo de nodo pero los registros son legibles, se rescata como hoja.
                let rescued = rescue_leaf(tree, n, &h)?;
                let extra = match &rescued {
                    Some(node) => format!("; se rescataron sus {} registros", node.records.len()),
                    None => String::new(),
                };
                f.push(
                    "HFS_NODE_UNREADABLE",
                    Severity::Critical,
                    ObjectType::Node,
                    None,
                    Some(n),
                    None,
                    format!("Árbol {name}: nodo {n} ilegible ({e}){extra}"),
                    tree_details(json!({ "rescued": rescued.is_some() })),
                );
                if let Some(node) = rescued {
                    rescued_nodes.insert(n);
                    nodes.insert(n, node);
                }
            }
            Err(e) => {
                out.damaged_nodes += 1;
                f.push(
                    "HFS_NODE_UNREADABLE",
                    Severity::Critical,
                    ObjectType::Node,
                    None,
                    Some(n),
                    None,
                    format!("Árbol {name}: no se pudo leer el nodo {n} ({e})"),
                    tree_details(json!({})),
                );
            }
        }
    }
    out.allocated_nodes = allocated as u64;
    let expected_free = h.total_nodes.saturating_sub(allocated);
    if h.free_nodes != expected_free {
        f.push(
            "HFS_BTREE_FREE_COUNT",
            Severity::Cosmetic,
            ObjectType::Header,
            None,
            Some(0),
            None,
            format!("Árbol {name}: la cabecera indica {} nodos libres y el mapa {expected_free}", h.free_nodes),
            tree_details(json!({ "header": h.free_nodes, "map": expected_free })),
        );
    }

    let key_of = |node: &Node, i: usize| -> Option<Vec<u8>> {
        split_record(node.record(i), node.desc.kind == kind::INDEX, &h).map(|(k, _)| k.to_vec())
    };

    // 2) Cadena de hojas.
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut chain_broken = false;
    let mut prev = 0u32;
    let mut cur = h.first_leaf_node;
    while cur != 0 {
        let node = nodes.get(&cur).filter(|n| n.desc.kind == kind::LEAF);
        let Some(node) = node else {
            chain_broken = true;
            f.push(
                "HFS_BTREE_LINK_BROKEN",
                Severity::Critical,
                ObjectType::Node,
                None,
                Some(prev),
                None,
                format!("Árbol {name}: el enlace al siguiente nodo hoja apunta a {cur}, que no es una hoja válida"),
                tree_details(json!({ "target": cur })),
            );
            break;
        };
        if !visited.insert(cur) {
            chain_broken = true;
            f.push(
                "HFS_BTREE_LINK_BROKEN",
                Severity::Critical,
                ObjectType::Node,
                None,
                Some(cur),
                None,
                format!("Árbol {name}: ciclo en la cadena de nodos hoja (nodo {cur})"),
                tree_details(json!({})),
            );
            break;
        }
        if node.desc.blink != prev {
            f.push(
                "HFS_BTREE_BACKLINK",
                Severity::Minor,
                ObjectType::Node,
                None,
                Some(cur),
                None,
                format!(
                    "Árbol {name}: el enlace hacia atrás del nodo {cur} es {} y debería ser {prev}",
                    node.desc.blink
                ),
                tree_details(json!({})),
            );
        }
        chain.push(cur);
        prev = cur;
        cur = node.desc.flink;
    }
    if !chain_broken && prev != h.last_leaf_node {
        f.push(
            "HFS_BTREE_HEADER",
            Severity::Minor,
            ObjectType::Header,
            None,
            Some(0),
            None,
            format!("Árbol {name}: el último nodo hoja es {prev} y la cabecera indica {}", h.last_leaf_node),
            tree_details(json!({})),
        );
    }

    // 3) Orden de claves a lo largo de la cadena y recogida de registros.
    let mut last_key: Option<Vec<u8>> = None;
    let mut chain_records = 0u32;
    let take_leaf =
        |node: &Node, out: &mut TreeScan, f: &mut Findings, check_order: bool, last_key: &mut Option<Vec<u8>>| {
            out.leaf_nodes += 1;
            out.leaf_used_bytes += node.used_bytes() as u64;
            let mut reported = false;
            for i in 0..node.records.len() {
                let Some((k, d)) = split_record(node.record(i), false, &h) else {
                    f.push(
                        "HFS_RECORD_INVALID",
                        Severity::Critical,
                        ObjectType::Node,
                        None,
                        Some(node.number),
                        None,
                        format!("Árbol {name}: registro {i} del nodo {} con clave no válida", node.number),
                        tree_details(json!({ "record": i })),
                    );
                    continue;
                };
                if check_order {
                    if let Some(prev) = last_key.as_ref() {
                        if cmp(prev, k) != Ordering::Less && !reported {
                            reported = true;
                            f.push(
                                "HFS_BTREE_KEY_ORDER",
                                Severity::Critical,
                                ObjectType::Node,
                                None,
                                Some(node.number),
                                None,
                                format!(
                                    "Árbol {name}: claves fuera de orden en el nodo hoja {} (registro {i})",
                                    node.number
                                ),
                                tree_details(json!({ "record": i })),
                            );
                        }
                    }
                    *last_key = Some(k.to_vec());
                }
                out.records.push((
                    k.to_vec(),
                    d.to_vec(),
                    node.number,
                    check_order && !rescued_nodes.contains(&node.number),
                ));
            }
        };
    for n in &chain {
        let node = &nodes[n];
        chain_records += node.records.len() as u32;
        take_leaf(node, &mut out, f, true, &mut last_key);
    }
    if !chain_broken && chain_records != h.leaf_records {
        f.push(
            "HFS_LEAF_COUNT",
            Severity::Minor,
            ObjectType::Header,
            None,
            Some(0),
            None,
            format!(
                "Árbol {name}: la cabecera indica {} registros hoja y la cadena contiene {chain_records}",
                h.leaf_records
            ),
            tree_details(json!({ "header": h.leaf_records, "found": chain_records })),
        );
    }

    // 4) Hojas válidas no enlazadas (barrido).
    let mut unlinked: Vec<u32> = nodes
        .iter()
        .filter(|(n, node)| node.desc.kind == kind::LEAF && !visited.contains(n))
        .map(|(n, _)| *n)
        .collect();
    unlinked.sort_unstable();
    if !unlinked.is_empty() {
        let before = out.records.len();
        for n in &unlinked {
            let mut none = None;
            take_leaf(&nodes[n], &mut out, f, false, &mut none);
        }
        f.push(
            "HFS_LEAF_NOT_LINKED",
            Severity::Critical,
            ObjectType::Node,
            None,
            unlinked.first().copied(),
            None,
            format!(
                "Árbol {name}: {} nodos hoja con {} registros no están enlazados; se recuperan por barrido",
                unlinked.len(),
                out.records.len() - before
            ),
            tree_details(json!({ "nodes": unlinked })),
        );
    }

    // 5) Estructura del índice.
    let mut reachable_leaves = HashSet::new();
    if h.tree_depth == 0 || h.root_node == 0 {
        if h.leaf_records > 0 || !chain.is_empty() {
            f.push(
                "HFS_BTREE_ROOT_INVALID",
                Severity::Critical,
                ObjectType::Header,
                None,
                Some(0),
                None,
                format!("Árbol {name}: la cabecera no tiene nodo raíz pero el árbol contiene registros"),
                tree_details(json!({})),
            );
        }
    } else {
        let mut stack = vec![(h.root_node, h.tree_depth as u8, None::<Vec<u8>>)];
        let mut seen = HashSet::new();
        // (altura, primera clave, nodo) de cada nodo de índice, para comprobar enlaces entre hermanos.
        let mut index_nodes: Vec<(u8, Vec<u8>, u32)> = Vec::new();
        let mut depth_reported = false;
        while let Some((n, expected_height, parent_key)) = stack.pop() {
            if !seen.insert(n) {
                continue;
            }
            let Some(node) = nodes.get(&n) else {
                f.push(
                    "HFS_BTREE_ROOT_INVALID",
                    Severity::Critical,
                    ObjectType::Node,
                    None,
                    Some(n),
                    None,
                    format!("Árbol {name}: el índice apunta al nodo {n}, que no es válido"),
                    tree_details(json!({})),
                );
                continue;
            };
            if node.desc.height != expected_height && !depth_reported {
                depth_reported = true;
                f.push(
                    "HFS_BTREE_DEPTH_MISMATCH",
                    Severity::Minor,
                    ObjectType::Node,
                    None,
                    Some(n),
                    None,
                    format!(
                        "Árbol {name}: el nodo {n} tiene altura {} y se esperaba {expected_height}",
                        node.desc.height
                    ),
                    tree_details(json!({})),
                );
            }
            if let (Some(pk), Some(first)) = (parent_key, (!node.records.is_empty()).then(|| key_of(node, 0)).flatten())
            {
                if cmp(&pk, &first) != Ordering::Equal {
                    f.push(
                        "HFS_INDEX_KEY_MISMATCH",
                        Severity::Minor,
                        ObjectType::Node,
                        None,
                        Some(n),
                        None,
                        format!("Árbol {name}: la clave de índice del nodo {n} no coincide con su primera clave"),
                        tree_details(json!({})),
                    );
                }
            }
            match node.desc.kind {
                kind::LEAF => {
                    reachable_leaves.insert(n);
                }
                kind::INDEX => {
                    if let Some(k) = (!node.records.is_empty()).then(|| key_of(node, 0)).flatten() {
                        index_nodes.push((node.desc.height, k, n));
                    }
                    for i in 0..node.records.len() {
                        let Some((k, d)) = split_record(node.record(i), true, &h) else {
                            f.push(
                                "HFS_RECORD_INVALID",
                                Severity::Critical,
                                ObjectType::Node,
                                None,
                                Some(n),
                                None,
                                format!("Árbol {name}: registro de índice {i} del nodo {n} no válido"),
                                tree_details(json!({})),
                            );
                            continue;
                        };
                        let child = be_u32(d, 0);
                        stack.push((child, expected_height.saturating_sub(1), Some(k.to_vec())));
                    }
                }
                _ => {}
            }
        }
        // Enlaces entre nodos de índice del mismo nivel, en orden de clave.
        index_nodes.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| cmp(&a.1, &b.1)));
        let mut bad_siblings = 0;
        for (i, (height, _, n)) in index_nodes.iter().enumerate() {
            let next = index_nodes.get(i + 1).filter(|x| x.0 == *height).map(|x| x.2).unwrap_or(0);
            let prev =
                i.checked_sub(1).and_then(|j| index_nodes.get(j)).filter(|x| x.0 == *height).map(|x| x.2).unwrap_or(0);
            let d = nodes[n].desc;
            if d.flink != next || d.blink != prev {
                bad_siblings += 1;
            }
        }
        if bad_siblings > 0 {
            f.push(
                "HFS_BTREE_SIBLING_LINK",
                Severity::Minor,
                ObjectType::Node,
                None,
                None,
                None,
                format!("Árbol {name}: {bad_siblings} nodos de índice con enlaces incorrectos a sus hermanos"),
                tree_details(json!({ "nodes": bad_siblings })),
            );
        }
        let missing = chain.iter().filter(|n| !reachable_leaves.contains(n)).count();
        if missing > 0 {
            f.push(
                "HFS_INDEX_INCOMPLETE",
                Severity::Minor,
                ObjectType::Node,
                None,
                None,
                None,
                format!("Árbol {name}: {missing} nodos hoja no son accesibles desde el índice"),
                tree_details(json!({ "leaves": missing })),
            );
        }
    }
    progress.advance(limit as u64, limit as u64);
    Ok(out)
}

/// Escanea un volumen ya localizado.
pub fn scan_volume(vol: &Volume<'_>, progress: &mut dyn Progress) -> FsResult<HfsScan> {
    let vh = &vol.vh;
    let mut f = Findings::default();
    progress.phase("Cabecera del volumen");

    let bs = vh.block_size;
    if bs < 512 || !bs.is_power_of_two() {
        return Err(FsError::Invalid(format!("tamaño de bloque no válido: {bs}")));
    }
    let available = vol.dev.len().saturating_sub(vol.offset);
    if vh.volume_bytes() > available {
        f.simple(
            "HFS_VOLUME_TRUNCATED",
            Severity::Critical,
            ObjectType::Header,
            format!("El volumen declara {} bytes y el dispositivo solo tiene {available}", vh.volume_bytes()),
        );
    }
    match vol.alternate_header()? {
        Some(alt) if alt.block_size == vh.block_size && alt.total_blocks == vh.total_blocks => {}
        Some(_) => f.simple(
            "HFS_ALT_HEADER_MISMATCH",
            Severity::Minor,
            ObjectType::Header,
            "La copia alternativa del Volume Header no coincide con la principal".into(),
        ),
        None => f.simple(
            "HFS_ALT_HEADER_MISSING",
            Severity::Minor,
            ObjectType::Header,
            "No se encontró la copia alternativa del Volume Header".into(),
        ),
    }
    if vh.attributes & attr::UNMOUNTED == 0 {
        f.simple(
            "HFS_NOT_UNMOUNTED",
            Severity::Cosmetic,
            ObjectType::Header,
            "El volumen no se desmontó limpiamente la última vez".into(),
        );
    }
    if vh.attributes & attr::INCONSISTENT != 0 {
        f.simple(
            "HFS_MARKED_INCONSISTENT",
            Severity::Minor,
            ObjectType::Header,
            "El sistema marcó el volumen como inconsistente".into(),
        );
    }
    if vh.is_journaled() {
        check_journal(vol, &mut f)?;
    }

    // Extents Overflow
    progress.phase("Árbol de extents");
    check_cancel(progress)?;
    let no_overflow = OverflowMap::new();
    let ext_reader = vol.fork_reader(&vh.extents_file, cnid::EXTENTS_FILE, true, &no_overflow);
    let mut overflow = OverflowMap::new();
    let mut stats = ScanStats::default();
    let mut extents_node_size = 4096u16;
    match BTree::open(ext_reader)? {
        Ok(tree) => {
            let cmp = |a: &[u8], b: &[u8]| match (ExtentKey::parse(a), ExtentKey::parse(b)) {
                (Some(x), Some(y)) => x.compare(&y),
                _ => Ordering::Equal,
            };
            extents_node_size = tree.header.node_size;
            let ts = scan_tree("extents", &tree, &cmp, &mut f, progress)?;
            stats.nodes_damaged += ts.damaged_nodes;
            for (k, d, node, _) in ts.records {
                match (ExtentKey::parse(&k), parse_record(&d)) {
                    (Some(key), Some(exts)) => {
                        overflow.entry((key.file_id, key.fork_type)).or_default().push((key.start_block, exts))
                    }
                    _ => f.push(
                        "HFS_RECORD_INVALID",
                        Severity::Critical,
                        ObjectType::Extent,
                        None,
                        Some(node),
                        None,
                        format!("Registro de extents ilegible en el nodo {node}"),
                        json!({ "tree": "extents" }),
                    ),
                }
            }
        }
        Err(e) => f.simple(
            "HFS_BTREE_HEADER",
            Severity::Critical,
            ObjectType::Header,
            format!("Árbol de extents ilegible: {e}"),
        ),
    }

    // Catálogo
    progress.phase("Catálogo: estructura");
    check_cancel(progress)?;
    let cat_reader = vol.fork_reader(&vh.catalog_file, cnid::CATALOG_FILE, true, &overflow);
    let tree = match BTree::open(cat_reader)? {
        Ok(t) => t,
        Err(e) => {
            return Err(FsError::Invalid(format!(
                "catálogo ilegible ({e}); hará falta el barrido completo del volumen (fase F2)"
            )))
        }
    };
    let binary = vh.is_hfsx() && tree.header.key_compare_type == KEY_COMPARE_BINARY;
    let cmp = move |a: &[u8], b: &[u8]| match (CatalogKey::parse(a), CatalogKey::parse(b)) {
        (Some(x), Some(y)) => x.compare(&y, binary),
        _ => Ordering::Equal,
    };
    let ts = scan_tree("catálogo", &tree, &cmp, &mut f, progress)?;
    stats.nodes_total = ts.total_nodes;
    stats.nodes_damaged += ts.damaged_nodes;
    if ts.leaf_nodes > 0 {
        let capacity = ts.leaf_nodes * tree.node_size() as u64;
        stats.fragmentation_pct = (100.0 * (1.0 - ts.leaf_used_bytes as f64 / capacity as f64)) as f32;
    }

    progress.phase("Catálogo: coherencia");
    check_cancel(progress)?;
    let catalog_nodes_used = ts.allocated_nodes;
    let before_invalid = f.per_code.get("HFS_RECORD_INVALID").copied().unwrap_or(0);
    let catalog = build_catalog(ts.records, &mut f);
    let dropped_records = (f.per_code.get("HFS_RECORD_INVALID").copied().unwrap_or(0) - before_invalid) as u64;
    check_catalog(vh, &catalog, &mut f);
    stats.files = catalog.files.len() as u64;
    stats.folders = catalog.folders.len().saturating_sub(1) as u64;

    progress.phase("Extents y bitmap de asignación");
    check_cancel(progress)?;
    check_allocation(vol, &catalog, &overflow, &mut f, progress)?;

    let name = catalog.folders.get(&cnid::ROOT_FOLDER).map(|(k, _)| from_hfs_name(&k.name));
    let info = FsInfo {
        kind: if vh.is_hfsx() { FsKind::Hfsx } else { FsKind::HfsPlus },
        name,
        block_size: bs,
        total_blocks: vh.total_blocks as u64,
        free_blocks: vh.free_blocks as u64,
        journaled: vh.is_journaled(),
        case_sensitive: binary,
    };
    Ok(HfsScan {
        result: ScanResult { info, stats, findings: f.finish() },
        catalog,
        overflow,
        catalog_nodes_used,
        catalog_node_size: tree.header.node_size,
        extents_node_size,
        dropped_records,
    })
}

/// Intenta leer como hoja un nodo cuyo descriptor tiene un tipo no válido.
fn rescue_leaf(tree: &BTree<'_>, n: u32, h: &crate::btree::HeaderRecord) -> FsResult<Option<Node>> {
    let mut raw = tree.read_raw(n)?;
    raw[8] = kind::LEAF as u8;
    raw[9] = 1;
    let Ok(node) = Node::parse(n, raw) else { return Ok(None) };
    let valid = !node.records.is_empty()
        && (0..node.records.len())
            .all(|i| split_record(node.record(i), false, h).is_some_and(|(k, d)| !k.is_empty() && d.len() >= 2));
    Ok(valid.then_some(node))
}

fn check_journal(vol: &Volume<'_>, f: &mut Findings) -> FsResult<()> {
    let vh = &vol.vh;
    let jib_off = vol.offset + vh.journal_info_block as u64 * vh.block_size as u64;
    if jib_off + 52 > vol.dev.len() {
        f.simple(
            "HFS_JOURNAL_INVALID",
            Severity::Minor,
            ObjectType::Header,
            "El bloque de información del journal está fuera del volumen".into(),
        );
        return Ok(());
    }
    let jib = vol.dev.read_vec(jib_off, 52)?;
    let flags = be_u32(&jib, 0);
    if flags & 0x2 != 0 {
        f.simple(
            "HFS_JOURNAL_EXTERNAL",
            Severity::Minor,
            ObjectType::Header,
            "El journal está en otro dispositivo; no se puede comprobar si tiene transacciones pendientes".into(),
        );
        return Ok(());
    }
    // kJIJournalNeedInitMask: el journal aún no se ha inicializado, no puede tener transacciones.
    if flags & 0x4 != 0 {
        return Ok(());
    }
    let jh_off = vol.offset + be_u64(&jib, 36);
    if jh_off + 24 > vol.dev.len() {
        f.simple(
            "HFS_JOURNAL_INVALID",
            Severity::Minor,
            ObjectType::Header,
            "La cabecera del journal está fuera del volumen".into(),
        );
        return Ok(());
    }
    let jh = vol.dev.read_vec(jh_off, 24)?;
    let (start, end) = if jh[0..4] == 0x4a4e_4c78u32.to_be_bytes() {
        (be_u64(&jh, 8), be_u64(&jh, 16))
    } else if jh[0..4] == 0x4a4e_4c78u32.to_le_bytes() {
        (u64::from_le_bytes(jh[8..16].try_into().expect("8")), u64::from_le_bytes(jh[16..24].try_into().expect("8")))
    } else {
        f.simple(
            "HFS_JOURNAL_INVALID",
            Severity::Minor,
            ObjectType::Header,
            "Firma de la cabecera del journal no válida".into(),
        );
        return Ok(());
    };
    if start != end {
        f.simple("HFS_JOURNAL_PENDING", Severity::Minor, ObjectType::Header,
            "El journal tiene transacciones sin aplicar: el análisis refleja el estado sin reproducirlas y algunos hallazgos podrían desaparecer al montar el volumen".into());
    }
    Ok(())
}

fn build_catalog(records: Vec<RawRecord>, f: &mut Findings) -> CatalogSnapshot {
    let mut cat = CatalogSnapshot::default();
    let mut seen_keys: HashMap<CatalogKey, Vec<u8>> = HashMap::new();
    for (k, d, node, linked) in records {
        let (Some(key), Some(rec)) = (CatalogKey::parse(&k), CatalogRecord::parse(&d)) else {
            f.push(
                "HFS_RECORD_INVALID",
                Severity::Critical,
                ObjectType::Node,
                None,
                Some(node),
                None,
                format!("Registro del catálogo ilegible en el nodo {node}"),
                json!({ "tree": "catálogo" }),
            );
            continue;
        };
        if let Some(prev) = seen_keys.get(&key) {
            if *prev != d {
                f.push(
                    "HFS_DUPLICATE_KEY",
                    Severity::Critical,
                    ObjectType::Node,
                    None,
                    Some(node),
                    None,
                    format!("Clave duplicada con contenido distinto en el nodo {node} ({})", from_hfs_name(&key.name)),
                    json!({ "parent": key.parent_id }),
                );
            }
            continue;
        }
        seen_keys.insert(key.clone(), d);
        match &rec {
            CatalogRecord::Folder(r) if !linked => {
                cat.swept.insert(r.folder_id);
            }
            CatalogRecord::File(r) if !linked => {
                cat.swept.insert(r.file_id);
            }
            _ => {}
        }
        match rec {
            CatalogRecord::Folder(r) => {
                if let Some(prev) = cat.folders.insert(r.folder_id, (key.clone(), r.clone())) {
                    f.push(
                        "HFS_DUPLICATE_CNID",
                        Severity::Critical,
                        ObjectType::Folder,
                        Some(r.folder_id as u64),
                        Some(node),
                        None,
                        format!(
                            "El CNID {} lo usan dos carpetas («{}» y «{}»)",
                            r.folder_id,
                            from_hfs_name(&prev.0.name),
                            from_hfs_name(&key.name)
                        ),
                        json!({}),
                    );
                }
            }
            CatalogRecord::File(r) => {
                if cat.folders.contains_key(&r.file_id)
                    || cat.files.insert(r.file_id, (key.clone(), r.clone())).is_some()
                {
                    f.push(
                        "HFS_DUPLICATE_CNID",
                        Severity::Critical,
                        ObjectType::File,
                        Some(r.file_id as u64),
                        Some(node),
                        None,
                        format!("El CNID {} está asignado a más de un elemento", r.file_id),
                        json!({}),
                    );
                }
            }
            CatalogRecord::FolderThread(t) => {
                cat.threads.insert(key.parent_id, (true, t));
            }
            CatalogRecord::FileThread(t) => {
                cat.threads.insert(key.parent_id, (false, t));
            }
        }
    }
    cat
}

fn check_catalog(vh: &crate::volume_header::VolumeHeader, cat: &CatalogSnapshot, f: &mut Findings) {
    if !cat.folders.contains_key(&cnid::ROOT_FOLDER) {
        f.simple(
            "HFS_ROOT_MISSING",
            Severity::Critical,
            ObjectType::Folder,
            "No existe el registro de la carpeta raíz (CNID 2)".into(),
        );
    }
    let mut children: HashMap<u32, u32> = HashMap::new();
    let mut max_id = 0u32;

    let mut check_item = |id: u32, key: &CatalogKey, is_folder: bool, needs_thread: bool, f: &mut Findings| {
        max_id = max_id.max(id);
        let ot = if is_folder { ObjectType::Folder } else { ObjectType::File };
        if id != cnid::ROOT_FOLDER {
            *children.entry(key.parent_id).or_default() += 1;
            if !cat.folders.contains_key(&key.parent_id) {
                f.push(
                    "HFS_ORPHAN_RECORD",
                    Severity::Critical,
                    ot,
                    Some(id as u64),
                    None,
                    Some(cat.label(id, key)),
                    format!(
                        "{} cuya carpeta padre (CNID {}) no existe en el catálogo",
                        if is_folder { "Carpeta" } else { "Archivo" },
                        key.parent_id
                    ),
                    json!({ "parent": key.parent_id }),
                );
            }
        } else if key.parent_id != cnid::ROOT_PARENT {
            f.simple(
                "HFS_ROOT_MISSING",
                Severity::Critical,
                ObjectType::Folder,
                "La carpeta raíz tiene un padre incorrecto".into(),
            );
        }
        match cat.threads.get(&id) {
            None if needs_thread => f.push(
                "HFS_MISSING_THREAD",
                Severity::Minor,
                ot,
                Some(id as u64),
                None,
                Some(cat.label(id, key)),
                "Falta el thread record; el elemento no se puede localizar por su CNID".into(),
                json!({}),
            ),
            Some((tf, t)) if *tf != is_folder || t.parent_id != key.parent_id || t.name != key.name => f.push(
                "HFS_THREAD_MISMATCH",
                Severity::Minor,
                ot,
                Some(id as u64),
                None,
                Some(cat.label(id, key)),
                "El thread record no coincide con el registro del elemento".into(),
                json!({}),
            ),
            _ => {}
        }
    };
    for (id, (key, _)) in &cat.folders {
        check_item(*id, key, true, true, f);
    }
    for (id, (key, rec)) in &cat.files {
        check_item(*id, key, false, rec.info.flags & FILE_THREAD_EXISTS != 0, f);
    }
    for (id, (is_folder, t)) in &cat.threads {
        let exists = if *is_folder { cat.folders.contains_key(id) } else { cat.files.contains_key(id) };
        if !exists {
            f.push(
                "HFS_ORPHAN_THREAD",
                Severity::Minor,
                ObjectType::Node,
                Some(*id as u64),
                None,
                None,
                format!("Thread record del CNID {id} («{}») sin elemento asociado", from_hfs_name(&t.name)),
                json!({}),
            );
        }
    }
    for (id, (key, rec)) in &cat.folders {
        let real = children.get(id).copied().unwrap_or(0);
        if real != rec.valence {
            f.push(
                "HFS_VALENCE_MISMATCH",
                Severity::Minor,
                ObjectType::Folder,
                Some(*id as u64),
                None,
                Some(cat.label(*id, key)),
                format!("La carpeta declara {} elementos y contiene {real}", rec.valence),
                json!({ "declared": rec.valence, "found": real }),
            );
        }
    }
    let files = cat.files.len() as u32;
    let folders = cat.folders.len().saturating_sub(1) as u32;
    if vh.file_count != files {
        f.simple(
            "HFS_FILE_COUNT",
            Severity::Minor,
            ObjectType::Header,
            format!("El Volume Header indica {} archivos; el catálogo contiene {files}", vh.file_count),
        );
    }
    if vh.folder_count != folders {
        f.simple(
            "HFS_FOLDER_COUNT",
            Severity::Minor,
            ObjectType::Header,
            format!("El Volume Header indica {} carpetas; el catálogo contiene {folders}", vh.folder_count),
        );
    }
    if vh.attributes & attr::CNIDS_REUSED == 0 && max_id >= vh.next_catalog_id {
        f.simple("HFS_NEXT_CNID", Severity::Critical, ObjectType::Header,
            format!("El siguiente CNID libre ({}) no supera el mayor en uso ({max_id}); los archivos nuevos podrían reutilizar IDs", vh.next_catalog_id));
    }
}

struct Claim {
    start: u32,
    count: u32,
    owner: String,
    id: u32,
}

fn check_allocation(
    vol: &Volume<'_>,
    cat: &CatalogSnapshot,
    overflow: &OverflowMap,
    f: &mut Findings,
    progress: &mut dyn Progress,
) -> FsResult<()> {
    let vh = &vol.vh;
    let total = vh.total_blocks;
    let bs = vh.block_size as u64;
    let mut claims: Vec<Claim> = Vec::new();

    let mut check_fork =
        |fork: &ForkData, id: u32, fork_type: u8, owner: String, path: Option<String>, f: &mut Findings| {
            if fork.logical_size == 0 && fork.total_blocks == 0 {
                return;
            }
            let exts = full_extents(fork, id, fork_type, overflow);
            let sum: u64 = exts.iter().map(|e| e.block_count as u64).sum();
            let ot = ObjectType::Extent;
            if sum < fork.total_blocks as u64 {
                f.push(
                    "HFS_FORK_EXTENTS_MISSING",
                    Severity::Critical,
                    ot,
                    Some(id as u64),
                    None,
                    path.clone(),
                    format!("{owner}: los extents cubren {sum} de {} bloques; faltan datos", fork.total_blocks),
                    json!({}),
                );
            } else if sum > fork.total_blocks as u64 {
                f.push(
                    "HFS_FORK_BLOCKS_MISMATCH",
                    Severity::Minor,
                    ot,
                    Some(id as u64),
                    None,
                    path.clone(),
                    format!("{owner}: los extents suman {sum} bloques y el fork declara {}", fork.total_blocks),
                    json!({}),
                );
            }
            if fork.logical_size > fork.total_blocks as u64 * bs {
                f.push(
                    "HFS_FORK_SIZE",
                    Severity::Critical,
                    ot,
                    Some(id as u64),
                    None,
                    path.clone(),
                    format!("{owner}: tamaño lógico mayor que el espacio asignado"),
                    json!({}),
                );
            }
            for e in exts {
                if e.start_block as u64 + e.block_count as u64 > total as u64 {
                    f.push(
                        "HFS_EXTENT_OUT_OF_RANGE",
                        Severity::Critical,
                        ot,
                        Some(id as u64),
                        None,
                        path.clone(),
                        format!(
                            "{owner}: extent {}+{} fuera de los límites del volumen ({total} bloques)",
                            e.start_block, e.block_count
                        ),
                        json!({ "start": e.start_block, "count": e.block_count }),
                    );
                } else {
                    claims.push(Claim { start: e.start_block, count: e.block_count, owner: owner.clone(), id });
                }
            }
        };

    for (fork, id, label) in [
        (&vh.allocation_file, cnid::ALLOCATION_FILE, "Archivo de asignación"),
        (&vh.extents_file, cnid::EXTENTS_FILE, "Árbol de extents"),
        (&vh.catalog_file, cnid::CATALOG_FILE, "Catálogo"),
        (&vh.attributes_file, cnid::ATTRIBUTES_FILE, "Árbol de atributos"),
        (&vh.startup_file, cnid::STARTUP_FILE, "Archivo de arranque"),
    ] {
        check_fork(fork, id, FORK_DATA, label.to_string(), None, f);
    }
    for (id, (key, rec)) in &cat.files {
        let path = cat.label(*id, key);
        check_fork(&rec.data_fork, *id, FORK_DATA, path.clone(), Some(path.clone()), f);
        check_fork(&rec.rsrc_fork, *id, FORK_RSRC, format!("{path} (fork de recursos)"), Some(path), f);
    }

    // Bloques reservados: los primeros 1536 bytes y los últimos 1024 del volumen.
    let reserved_head = 1536u64.div_ceil(bs) as u32;
    let tail_start = ((vh.volume_bytes().saturating_sub(1024)) / bs) as u32;
    claims.push(Claim { start: 0, count: reserved_head, owner: "Zona reservada inicial".into(), id: 0 });
    if tail_start < total {
        claims.push(Claim {
            start: tail_start,
            count: total - tail_start,
            owner: "Zona reservada final".into(),
            id: 0,
        });
    }

    // Solapes
    claims.sort_by_key(|c| (c.start, c.count));
    let mut max_end = 0u64;
    let mut max_owner = 0usize;
    for (i, c) in claims.iter().enumerate() {
        let end = c.start as u64 + c.count as u64;
        if i > 0 && (c.start as u64) < max_end && !(c.id == 0 && claims[max_owner].id == 0) {
            let other = &claims[max_owner];
            f.push(
                "HFS_EXTENT_OVERLAP",
                Severity::Critical,
                ObjectType::Extent,
                Some(c.id as u64),
                None,
                (c.id >= cnid::FIRST_USER).then(|| c.owner.clone()),
                format!("Los bloques {}–{} de «{}» se solapan con «{}»", c.start, end - 1, c.owner, other.owner),
                json!({ "start": c.start, "count": c.count, "other": other.owner }),
            );
        }
        if end > max_end {
            max_end = end;
            max_owner = i;
        }
    }

    // Bitmap de asignación
    let alloc_reader = vol.fork_reader(&vh.allocation_file, cnid::ALLOCATION_FILE, true, overflow);
    let need = (total as usize).div_ceil(8);
    if (alloc_reader.mapped_bytes() as usize) < need || (vh.allocation_file.logical_size as usize) < need {
        f.simple(
            "HFS_BITMAP_TOO_SMALL",
            Severity::Critical,
            ObjectType::Bitmap,
            "El archivo de asignación es demasiado pequeño para el volumen".into(),
        );
        return Ok(());
    }
    let bitmap = AllocationBitmap::from_bytes(alloc_reader.read_vec(0, need)?, total);
    let mut claimed = AllocationBitmap::new_empty(total);
    for c in &claims {
        claimed.set_range(c.start, c.count);
    }
    let (mut leaked, mut unprotected) = (0u64, 0u64);
    let mut first_unprotected = None;
    for b in 0..total {
        if b % (1 << 20) == 0 {
            progress.advance(b as u64, total as u64);
            check_cancel(progress)?;
        }
        match (bitmap.is_set(b), claimed.is_set(b)) {
            (true, false) => leaked += 1,
            (false, true) => {
                unprotected += 1;
                first_unprotected.get_or_insert(b);
            }
            _ => {}
        }
    }
    if unprotected > 0 {
        f.push(
            "HFS_BITMAP_UNDERALLOC",
            Severity::Critical,
            ObjectType::Bitmap,
            None,
            None,
            None,
            format!("{unprotected} bloques en uso figuran como libres: podrían sobrescribirse"),
            json!({ "blocks": unprotected, "first": first_unprotected }),
        );
    }
    if leaked > 0 {
        f.push(
            "HFS_BITMAP_LEAK",
            Severity::Minor,
            ObjectType::Bitmap,
            None,
            None,
            None,
            format!("{leaked} bloques marcados como usados no pertenecen a ningún archivo"),
            json!({ "blocks": leaked }),
        );
    }
    let free = total - bitmap.count_set();
    if free != vh.free_blocks {
        f.simple(
            "HFS_FREE_COUNT",
            Severity::Minor,
            ObjectType::Header,
            format!("El Volume Header indica {} bloques libres y el bitmap {free}", vh.free_blocks),
        );
    }
    Ok(())
}
