//! Empaquetado de B-trees: construye todos los nodos de un árbol a partir de
//! registros hoja ya ordenados. Lo usan el reconstructor y el generador de fixtures.
//!
//! Las hojas se llenan al máximo (o hasta `max_records_per_leaf`), los niveles de
//! índice se generan hasta tener una única raíz, y si el mapa de nodos no cabe en
//! el nodo cabecera se añaden nodos de mapa encadenados.

use blocksmith_io::endian::put_be_u16;

use super::{attr, kind, HeaderRecord, NodeDescriptor, DESCRIPTOR_SIZE};

/// Parámetros de un B-tree a empaquetar.
#[derive(Debug, Clone)]
pub struct TreeSpec {
    pub node_size: u16,
    pub max_key_length: u16,
    pub attributes: u32,
    pub key_compare_type: u8,
    pub total_nodes: u32,
    pub max_records_per_leaf: Option<usize>,
}

/// Bytes de mapa que caben en el nodo cabecera.
pub fn header_map_bytes(node_size: usize) -> usize {
    node_size - DESCRIPTOR_SIZE - HeaderRecord::SIZE - 128 - 2 * 4
}

/// Bytes de mapa de cada nodo de mapa adicional.
pub fn map_node_bytes(node_size: usize) -> usize {
    node_size - DESCRIPTOR_SIZE - 2 * 2
}

/// Nodos de mapa adicionales necesarios para `total_nodes`.
pub fn map_nodes_needed(node_size: usize, total_nodes: u32) -> u32 {
    let head_bits = header_map_bytes(node_size) * 8;
    if total_nodes as usize <= head_bits {
        0
    } else {
        ((total_nodes as usize - head_bits).div_ceil(map_node_bytes(node_size) * 8)) as u32
    }
}

/// Nodos de cabecera, hojas e índices (sin contar nodos de mapa).
pub fn tree_nodes(records: &[(Vec<u8>, Vec<u8>)], spec: &TreeSpec) -> Result<u32, String> {
    let levels = layout(records, spec)?;
    Ok(1 + levels.iter().map(|l| l.len() as u32).sum::<u32>())
}

/// Nodos que ocupará el árbol (cabecera + hojas + índices + mapas) para `spec.total_nodes`.
pub fn nodes_needed(records: &[(Vec<u8>, Vec<u8>)], spec: &TreeSpec) -> Result<u32, String> {
    Ok(tree_nodes(records, spec)? + map_nodes_needed(spec.node_size as usize, spec.total_nodes))
}

type Group = Vec<Vec<u8>>;
/// Un nodo en construcción: (clave del primer registro, registros).
type PendingNode = (Vec<u8>, Group);

/// Agrupa registros en nodos respetando capacidad y límite.
fn group(recs: Vec<Vec<u8>>, ns: usize, limit: Option<usize>) -> Result<Vec<Group>, String> {
    let fits = |bytes: usize, count: usize| DESCRIPTOR_SIZE + bytes + 2 * (count + 1) <= ns;
    let mut out: Vec<Group> = Vec::new();
    let mut cur: Group = Vec::new();
    let mut cur_bytes = 0usize;
    for r in recs {
        if r.len() % 2 != 0 {
            return Err("registro de longitud impar".into());
        }
        if !fits(r.len(), 1) {
            return Err("registro mayor que un nodo".into());
        }
        if !cur.is_empty() && (!fits(cur_bytes + r.len(), cur.len() + 1) || limit.is_some_and(|l| cur.len() >= l)) {
            out.push(std::mem::take(&mut cur));
            cur_bytes = 0;
        }
        cur_bytes += r.len();
        cur.push(r);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

/// Niveles del árbol: [hojas, índice 1, índice 2, …]; cada nivel es una lista de nodos.
/// Junto a cada nodo va la clave (con keyLength) de su primer registro.
fn layout(records: &[(Vec<u8>, Vec<u8>)], spec: &TreeSpec) -> Result<Vec<Vec<PendingNode>>, String> {
    let ns = spec.node_size as usize;
    if records.is_empty() {
        return Ok(vec![]);
    }
    let leaf_recs: Vec<Vec<u8>> = records.iter().map(|(k, d)| [k.as_slice(), d.as_slice()].concat()).collect();
    let mut keys = records.iter().map(|(k, _)| k.clone());
    let leaves: Vec<(Vec<u8>, Group)> = group(leaf_recs, ns, spec.max_records_per_leaf)?
        .into_iter()
        .map(|g| {
            let first = keys.next().expect("hay registros");
            for _ in 1..g.len() {
                keys.next();
            }
            (first, g)
        })
        .collect();
    let mut levels = vec![leaves];
    // Los números de nodo se asignan después; aquí solo hace falta la forma.
    while levels.last().map(Vec::len).unwrap_or(0) > 1 {
        let below = levels.last().expect("hay niveles");
        let mut recs = Vec::with_capacity(below.len());
        for (key, _) in below {
            let mut k = key.clone();
            if spec.attributes & attr::VARIABLE_INDEX_KEYS == 0 {
                k.resize(2 + spec.max_key_length as usize, 0);
            }
            k.extend_from_slice(&[0, 0, 0, 0]); // puntero, se rellena al numerar
            recs.push(k);
        }
        let mut firsts = below.iter().map(|(k, _)| k.clone());
        let next: Vec<(Vec<u8>, Group)> = group(recs, ns, None)?
            .into_iter()
            .map(|g| {
                let first = firsts.next().expect("hay claves");
                for _ in 1..g.len() {
                    firsts.next();
                }
                (first, g)
            })
            .collect();
        levels.push(next);
    }
    Ok(levels)
}

/// Empaqueta registros hoja ya ordenados (`clave con keyLength`, datos) en nodos.
/// Devuelve todos los nodos (`total_nodes`, el 0 es la cabecera) y el header record.
pub fn pack_btree(records: &[(Vec<u8>, Vec<u8>)], spec: &TreeSpec) -> Result<(Vec<Vec<u8>>, HeaderRecord), String> {
    let ns = spec.node_size as usize;
    let mut levels = layout(records, spec)?;

    // Numeración: hojas desde el 1, después cada nivel de índice; la raíz queda la última.
    let mut numbers: Vec<Vec<u32>> = Vec::new();
    let mut next = 1u32;
    for lvl in &levels {
        numbers.push((next..next + lvl.len() as u32).collect());
        next += lvl.len() as u32;
    }
    let map_nodes = map_nodes_needed(ns, spec.total_nodes);
    let used = next + map_nodes;
    if used > spec.total_nodes {
        return Err(format!("el árbol necesita {used} nodos y solo hay {}", spec.total_nodes));
    }
    // Punteros de los registros de índice.
    for l in 1..levels.len() {
        let mut child = numbers[l - 1].iter();
        for (_, g) in &mut levels[l] {
            for rec in g.iter_mut() {
                let n = *child.next().expect("un puntero por nodo hijo");
                let len = rec.len();
                rec[len - 4..].copy_from_slice(&n.to_be_bytes());
            }
        }
    }

    let mut nodes = vec![vec![0u8; ns]; spec.total_nodes as usize];
    for (l, lvl) in levels.iter().enumerate() {
        for (i, (_, recs)) in lvl.iter().enumerate() {
            // Los nodos de cada nivel (hojas e índices) se enlazan con sus hermanos.
            let (flink, blink) = (numbers[l].get(i + 1).copied().unwrap_or(0), if i == 0 { 0 } else { numbers[l][i - 1] });
            let desc = NodeDescriptor {
                flink,
                blink,
                kind: if l == 0 { kind::LEAF } else { kind::INDEX },
                height: l as u8 + 1,
                num_records: recs.len() as u16,
            };
            write_node(&mut nodes[numbers[l][i] as usize], &desc, recs);
        }
    }

    // Mapa de nodos: cabecera + nodos de mapa encadenados.
    let mut bits = vec![0u8; header_map_bytes(ns) + map_nodes as usize * map_node_bytes(ns)];
    for n in 0..used {
        bits[n as usize / 8] |= 0x80 >> (n % 8);
    }
    let first_map = next;
    let (head_bits, rest) = bits.split_at(header_map_bytes(ns));
    for (i, chunk) in rest.chunks(map_node_bytes(ns)).enumerate() {
        let n = first_map + i as u32;
        let desc = NodeDescriptor {
            flink: if (i as u32) + 1 < map_nodes { n + 1 } else { 0 },
            blink: 0,
            kind: kind::MAP,
            height: 0,
            num_records: 1,
        };
        write_node(&mut nodes[n as usize], &desc, &[chunk.to_vec()]);
    }

    let empty = records.is_empty();
    let header = HeaderRecord {
        tree_depth: levels.len() as u16,
        root_node: if empty { 0 } else { *numbers.last().and_then(|v| v.first()).expect("raíz") },
        leaf_records: records.len() as u32,
        first_leaf_node: if empty { 0 } else { numbers[0][0] },
        last_leaf_node: if empty { 0 } else { *numbers[0].last().expect("hojas") },
        node_size: spec.node_size,
        max_key_length: spec.max_key_length,
        total_nodes: spec.total_nodes,
        free_nodes: spec.total_nodes - used,
        clump_size: spec.total_nodes * spec.node_size as u32,
        btree_type: 0,
        key_compare_type: spec.key_compare_type,
        attributes: spec.attributes,
    };
    let hdesc = NodeDescriptor {
        flink: if map_nodes > 0 { first_map } else { 0 },
        blink: 0,
        kind: kind::HEADER,
        height: 0,
        num_records: 3,
    };
    write_node(&mut nodes[0], &hdesc, &[header.to_bytes().to_vec(), vec![0u8; 128], head_bits.to_vec()]);
    Ok((nodes, header))
}

fn write_node(node: &mut [u8], desc: &NodeDescriptor, recs: &[Vec<u8>]) {
    desc.write(node);
    let ns = node.len();
    let mut off = DESCRIPTOR_SIZE;
    for (i, r) in recs.iter().enumerate() {
        put_be_u16(node, ns - 2 * (i + 1), off as u16);
        node[off..off + r.len()].copy_from_slice(r);
        off += r.len();
    }
    put_be_u16(node, ns - 2 * (recs.len() + 1), off as u16);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btree::Node;

    fn spec(total: u32) -> TreeSpec {
        TreeSpec { node_size: 512, max_key_length: 10, attributes: attr::BIG_KEYS, key_compare_type: 0, total_nodes: total, max_records_per_leaf: None }
    }

    fn recs(n: u32) -> Vec<(Vec<u8>, Vec<u8>)> {
        (0..n).map(|i| {
            let mut k = vec![0, 10, 0, 0];
            k.extend_from_slice(&i.to_be_bytes());
            k.extend_from_slice(&[0, 0, 0, 0]);
            (k, vec![0xAB; 64])
        }).collect()
    }

    #[test]
    fn builds_multi_level_tree_with_map_nodes() {
        // Nodos de 512 B: la cabecera cubre 2048 nodos y cada nodo de mapa 3952 más.
        let records = recs(2000);
        let s = spec(6000);
        assert_eq!(map_nodes_needed(512, 6000), 1);
        assert_eq!(map_nodes_needed(512, 9000), 2);
        let (nodes, h) = pack_btree(&records, &s).unwrap();
        assert!(h.tree_depth >= 3);
        assert_eq!(h.leaf_records, 2000);
        assert_eq!(nodes_needed(&records, &s).unwrap(), h.total_nodes - h.free_nodes);
        // La raíz es un nodo índice con la altura correcta, y el primer mapa está encadenado.
        let root = Node::parse(h.root_node, nodes[h.root_node as usize].clone()).unwrap();
        assert_eq!(root.desc.kind, kind::INDEX);
        assert_eq!(root.desc.height as u16, h.tree_depth);
        let head = Node::parse(0, nodes[0].clone()).unwrap();
        let map = Node::parse(head.desc.flink, nodes[head.desc.flink as usize].clone()).unwrap();
        assert_eq!(map.desc.kind, kind::MAP);
        // Recorrido de hojas: 2000 registros en orden.
        let mut n = h.first_leaf_node;
        let mut count = 0;
        while n != 0 {
            let leaf = Node::parse(n, nodes[n as usize].clone()).unwrap();
            count += leaf.records.len();
            n = leaf.desc.flink;
        }
        assert_eq!(count, 2000);
    }

    #[test]
    fn rejects_too_small_tree() {
        assert!(pack_btree(&recs(2000), &spec(20)).is_err());
    }
}
