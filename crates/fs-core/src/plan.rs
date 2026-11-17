use serde::{Deserialize, Serialize};

/// Fase de una escritura. El orden garantiza que una interrupción en cualquier
/// punto deja el volumen consistente con la versión anterior o con la nueva:
///
/// 1. `Data`: estructuras nuevas en espacio libre y bitmap con lo viejo y lo
///    nuevo marcados como usados (como mucho, una fuga de bloques).
/// 2. `Commit`: el puntero raíz (cabecera de volumen, superbloque, checkpoint)
///    pasa a apuntar a las estructuras nuevas.
/// 3. `Finalize`: limpieza posterior, como liberar en el bitmap los bloques viejos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WritePhase {
    Data,
    Commit,
    Finalize,
}

/// Escritura de bloque propuesta, con offset relativo al inicio del volumen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockWrite {
    pub offset: u64,
    pub data: Vec<u8>,
    pub phase: WritePhase,
    /// Descripción breve para registros y el undo log ("catálogo nuevo", "bitmap"…).
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffChange {
    /// Elemento que solo se encontró por barrido o que se recreó a partir de su thread.
    Recovered,
    /// Elemento sin carpeta padre localizable, movido a «Recovered Items».
    MovedToRecovered,
    /// Nombre cambiado para resolver un conflicto.
    Renamed,
    /// Recuento de elementos de la carpeta corregido.
    ValenceFixed,
    /// Registro descartado (ilegible o duplicado).
    Dropped,
    /// Extents fuera del volumen eliminados: el archivo pierde la parte inaccesible.
    DataTruncated,
    /// Bloques compartidos con otro archivo copiados a bloques propios.
    DataCloned,
    /// Sin cambios en el directorio, pero el usuario debería revisarlo.
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
    pub note: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RebuildPlan {
    pub writes: Vec<BlockWrite>,
    pub diff: Vec<DiffEntry>,
    pub nodes_before: u64,
    pub nodes_after: u64,
    pub recovered_count: u64,
    pub lost_count: u64,
    pub experimental: bool,
}

impl RebuildPlan {
    /// Escrituras en orden seguro: por fase y, dentro de cada fase, en el orden del plan.
    pub fn ordered_writes(&self) -> impl Iterator<Item = &BlockWrite> {
        [WritePhase::Data, WritePhase::Commit, WritePhase::Finalize]
            .into_iter()
            .flat_map(move |p| self.writes.iter().filter(move |w| w.phase == p))
    }

    pub fn bytes_to_write(&self) -> u64 {
        self.writes.iter().map(|w| w.data.len() as u64).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_are_ordered_by_phase() {
        let w = |o, phase| BlockWrite { offset: o, data: vec![], phase, label: String::new() };
        let plan = RebuildPlan {
            writes: vec![w(0, WritePhase::Commit), w(5, WritePhase::Finalize), w(10, WritePhase::Data), w(20, WritePhase::Data)],
            ..Default::default()
        };
        let order: Vec<u64> = plan.ordered_writes().map(|w| w.offset).collect();
        assert_eq!(order, vec![10, 20, 0, 5]);
    }
}
