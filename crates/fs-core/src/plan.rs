use serde::{Deserialize, Serialize};

/// Escritura de bloque propuesta. El helper la aplica de forma transaccional.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockWrite {
    pub offset: u64,
    pub data: Vec<u8>,
    /// Las escrituras con `commit_point` se aplican al final y de forma atómica
    /// (cabecera de volumen, superbloque, checkpoint).
    pub commit_point: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffChange {
    Recovered,
    MovedToRecovered,
    Renamed,
    ValenceFixed,
    Dropped,
    UnchangedFlagged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffEntry {
    pub object_id: u64,
    pub change: DiffChange,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub is_folder: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RebuildPlan {
    pub writes: Vec<BlockWrite>,
    pub diff: Vec<DiffEntry>,
    pub nodes_before: u64,
    pub nodes_after: u64,
    pub experimental: bool,
}

impl RebuildPlan {
    /// Devuelve las escrituras en orden seguro: datos primero, puntos de commit al final.
    pub fn ordered_writes(&self) -> impl Iterator<Item = &BlockWrite> {
        self.writes
            .iter()
            .filter(|w| !w.commit_point)
            .chain(self.writes.iter().filter(|w| w.commit_point))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_points_go_last() {
        let w = |o, c| BlockWrite { offset: o, data: vec![], commit_point: c };
        let plan = RebuildPlan { writes: vec![w(0, true), w(10, false), w(20, false)], ..Default::default() };
        let order: Vec<u64> = plan.ordered_writes().map(|w| w.offset).collect();
        assert_eq!(order, vec![10, 20, 0]);
    }
}
