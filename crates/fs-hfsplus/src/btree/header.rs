//! Header record (106 bytes), primer registro del nodo 0.

use dirforge_io::endian::{be_u16, be_u32, put_be_u16, put_be_u32};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderRecord {
    pub tree_depth: u16,
    pub root_node: u32,
    pub leaf_records: u32,
    pub first_leaf_node: u32,
    pub last_leaf_node: u32,
    pub node_size: u16,
    pub max_key_length: u16,
    pub total_nodes: u32,
    pub free_nodes: u32,
    pub clump_size: u32,
    pub btree_type: u8,
    pub key_compare_type: u8,
    pub attributes: u32,
}

impl HeaderRecord {
    pub const SIZE: usize = 106;

    pub fn parse(b: &[u8]) -> Self {
        Self {
            tree_depth: be_u16(b, 0),
            root_node: be_u32(b, 2),
            leaf_records: be_u32(b, 6),
            first_leaf_node: be_u32(b, 10),
            last_leaf_node: be_u32(b, 14),
            node_size: be_u16(b, 18),
            max_key_length: be_u16(b, 20),
            total_nodes: be_u32(b, 22),
            free_nodes: be_u32(b, 26),
            clump_size: be_u32(b, 32),
            btree_type: b.get(36).copied().unwrap_or(0),
            key_compare_type: b.get(37).copied().unwrap_or(0),
            attributes: be_u32(b, 38),
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put_be_u16(&mut b, 0, self.tree_depth);
        put_be_u32(&mut b, 2, self.root_node);
        put_be_u32(&mut b, 6, self.leaf_records);
        put_be_u32(&mut b, 10, self.first_leaf_node);
        put_be_u32(&mut b, 14, self.last_leaf_node);
        put_be_u16(&mut b, 18, self.node_size);
        put_be_u16(&mut b, 20, self.max_key_length);
        put_be_u32(&mut b, 22, self.total_nodes);
        put_be_u32(&mut b, 26, self.free_nodes);
        put_be_u32(&mut b, 32, self.clump_size);
        b[36] = self.btree_type;
        b[37] = self.key_compare_type;
        put_be_u32(&mut b, 38, self.attributes);
        b
    }
}
