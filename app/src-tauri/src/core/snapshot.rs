//! Árboles de directorio guardados en disco (MessagePack + zstd) para las vistas
//! de comparación, con índice de hijos en memoria.

use std::collections::HashMap;
use std::path::Path;

use blocksmith_fs_core::TreeEntry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct TreeSnapshot {
    pub root_id: u64,
    pub entries: Vec<TreeEntry>,
}

impl TreeSnapshot {
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let bytes = rmp_serde::to_vec(self).map_err(std::io::Error::other)?;
        std::fs::write(path, zstd::encode_all(&bytes[..], 3)?)
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let raw = zstd::decode_all(&std::fs::read(path)?[..])?;
        rmp_serde::from_slice(&raw).map_err(std::io::Error::other)
    }
}

/// Árbol con índice padre → hijos.
pub struct TreeIndex {
    pub snapshot: TreeSnapshot,
    children: HashMap<u64, Vec<usize>>,
}

impl TreeIndex {
    pub fn new(snapshot: TreeSnapshot) -> Self {
        let mut children: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, e) in snapshot.entries.iter().enumerate() {
            if e.id != snapshot.root_id {
                children.entry(e.parent).or_default().push(i);
            }
        }
        for list in children.values_mut() {
            list.sort_by(|a, b| {
                let (x, y) = (&snapshot.entries[*a], &snapshot.entries[*b]);
                y.is_folder.cmp(&x.is_folder).then_with(|| x.name.to_lowercase().cmp(&y.name.to_lowercase()))
            });
        }
        Self { snapshot, children }
    }

    pub fn root(&self) -> Option<&TreeEntry> {
        self.snapshot.entries.iter().find(|e| e.id == self.snapshot.root_id)
    }

    pub fn children(&self, parent: u64) -> impl Iterator<Item = &TreeEntry> {
        self.children.get(&parent).into_iter().flatten().map(|i| &self.snapshot.entries[*i])
    }

    pub fn child_count(&self, id: u64) -> u32 {
        self.children.get(&id).map(|v| v.len() as u32).unwrap_or(0)
    }
}
