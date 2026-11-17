//! B-trees de HFS+: nodos de tamaño fijo con descriptor de 14 bytes y tabla de
//! offsets de registros al final del nodo (creciendo hacia atrás).

pub mod header;

use dirforge_io::endian::{be_u16, be_u32};
use dirforge_io::IoResult;

pub use header::HeaderRecord;

use crate::fork::ForkReader;

pub const DESCRIPTOR_SIZE: usize = 14;

pub mod kind {
    pub const LEAF: i8 = -1;
    pub const INDEX: i8 = 0;
    pub const HEADER: i8 = 1;
    pub const MAP: i8 = 2;
}

/// Atributos del header record.
pub mod attr {
    pub const BIG_KEYS: u32 = 1 << 1;
    pub const VARIABLE_INDEX_KEYS: u32 = 1 << 2;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeDescriptor {
    pub flink: u32,
    pub blink: u32,
    pub kind: i8,
    pub height: u8,
    pub num_records: u16,
}

impl NodeDescriptor {
    pub fn parse(b: &[u8]) -> Self {
        Self {
            flink: be_u32(b, 0),
            blink: be_u32(b, 4),
            kind: b.get(8).copied().unwrap_or(0) as i8,
            height: b.get(9).copied().unwrap_or(0),
            num_records: be_u16(b, 10),
        }
    }

    pub fn write(&self, b: &mut [u8]) {
        b[0..4].copy_from_slice(&self.flink.to_be_bytes());
        b[4..8].copy_from_slice(&self.blink.to_be_bytes());
        b[8] = self.kind as u8;
        b[9] = self.height;
        b[10..12].copy_from_slice(&self.num_records.to_be_bytes());
        b[12..14].fill(0);
    }
}

/// Error estructural de un nodo (no de E/S).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    BadKind(i8),
    TooManyRecords(u16),
    BadOffsets,
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadKind(k) => write!(f, "tipo de nodo desconocido ({k})"),
            Self::TooManyRecords(n) => write!(f, "número de registros imposible ({n})"),
            Self::BadOffsets => f.write_str("tabla de offsets de registros incoherente"),
        }
    }
}

/// Nodo leído y validado.
#[derive(Debug, Clone)]
pub struct Node {
    pub number: u32,
    pub desc: NodeDescriptor,
    pub data: Vec<u8>,
    /// (inicio, fin) de cada registro dentro de `data`.
    pub records: Vec<(usize, usize)>,
}

impl Node {
    pub fn parse(number: u32, data: Vec<u8>) -> Result<Self, NodeError> {
        let desc = NodeDescriptor::parse(&data);
        if !(-1..=2).contains(&desc.kind) {
            return Err(NodeError::BadKind(desc.kind));
        }
        let size = data.len();
        let n = desc.num_records as usize;
        if DESCRIPTOR_SIZE + 2 * (n + 1) > size {
            return Err(NodeError::TooManyRecords(desc.num_records));
        }
        let offs: Vec<usize> = (0..=n).map(|i| be_u16(&data, size - 2 * (i + 1)) as usize).collect();
        let table_start = size - 2 * (n + 1);
        if offs[0] != DESCRIPTOR_SIZE
            || offs.windows(2).any(|w| w[1] < w[0] || w[0] % 2 != 0)
            || offs[n] > table_start
        {
            return Err(NodeError::BadOffsets);
        }
        let records = offs.windows(2).map(|w| (w[0], w[1])).collect();
        Ok(Self { number, desc, data, records })
    }

    pub fn record(&self, i: usize) -> &[u8] {
        let (a, b) = self.records[i];
        &self.data[a..b]
    }

    /// Bytes ocupados por registros (para medir el aprovechamiento del nodo).
    pub fn used_bytes(&self) -> usize {
        self.records.last().map(|r| r.1).unwrap_or(DESCRIPTOR_SIZE) + 2 * (self.records.len() + 1)
    }
}

/// Separa clave y datos de un registro. La clave devuelta no incluye el campo de longitud.
pub fn split_record<'r>(rec: &'r [u8], is_index: bool, header: &HeaderRecord) -> Option<(&'r [u8], &'r [u8])> {
    let kl = be_u16(rec, 0) as usize;
    if kl > header.max_key_length as usize || 2 + kl > rec.len() {
        return None;
    }
    let key_total = if is_index && header.attributes & attr::VARIABLE_INDEX_KEYS == 0 {
        2 + header.max_key_length as usize
    } else {
        2 + kl
    };
    let data_start = (key_total + 1) & !1;
    if data_start > rec.len() {
        return None;
    }
    Some((&rec[2..2 + kl], &rec[data_start..]))
}

/// Acceso a un B-tree sobre su fork.
pub struct BTree<'a> {
    pub reader: ForkReader<'a>,
    pub header: HeaderRecord,
    pub header_node: Node,
    map: Vec<u8>,
    pub map_warnings: Vec<String>,
}

impl<'a> BTree<'a> {
    pub fn open(reader: ForkReader<'a>) -> IoResult<Result<Self, String>> {
        let first = reader.read_vec(0, 512)?;
        let node_size = be_u16(&first, DESCRIPTOR_SIZE + 18) as usize;
        if !(512..=32768).contains(&node_size) || !node_size.is_power_of_two() {
            return Ok(Err(format!("tamaño de nodo no válido ({node_size})")));
        }
        let raw = reader.read_vec(0, node_size)?;
        let header_node = match Node::parse(0, raw) {
            Ok(n) if n.desc.kind == kind::HEADER && n.records.len() >= 3 => n,
            Ok(_) => return Ok(Err("el nodo 0 no es un nodo cabecera".into())),
            Err(e) => return Ok(Err(format!("nodo cabecera ilegible: {e}"))),
        };
        let header = HeaderRecord::parse(header_node.record(0));
        let mut map = header_node.record(2).to_vec();
        let mut map_warnings = Vec::new();
        // Nodos de mapa adicionales encadenados desde la cabecera.
        let mut next = header_node.desc.flink;
        let mut guard = 0;
        while next != 0 && guard < 4096 {
            guard += 1;
            if next >= header.total_nodes {
                map_warnings.push(format!("enlace a nodo de mapa fuera de rango ({next})"));
                break;
            }
            let raw = reader.read_vec(next as u64 * node_size as u64, node_size)?;
            match Node::parse(next, raw) {
                Ok(n) if n.desc.kind == kind::MAP && !n.records.is_empty() => {
                    map.extend_from_slice(n.record(0));
                    next = n.desc.flink;
                }
                _ => {
                    map_warnings.push(format!("nodo de mapa {next} no válido"));
                    break;
                }
            }
        }
        Ok(Ok(Self { reader, header, header_node, map, map_warnings }))
    }

    pub fn node_size(&self) -> usize {
        self.header.node_size as usize
    }

    pub fn is_allocated(&self, node: u32) -> bool {
        self.map.get(node as usize / 8).map(|b| b & (0x80 >> (node % 8)) != 0).unwrap_or(false)
    }

    pub fn read_raw(&self, node: u32) -> IoResult<Vec<u8>> {
        self.reader.read_vec(node as u64 * self.node_size() as u64, self.node_size())
    }

    pub fn read_node(&self, node: u32) -> IoResult<Result<Node, NodeError>> {
        Ok(Node::parse(node, self.read_raw(node)?))
    }
}
