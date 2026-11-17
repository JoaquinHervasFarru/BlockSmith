//! Resumen de los cambios de un plan, para la CLI y los informes.

use std::collections::BTreeMap;

use blocksmith_fs_core::{DiffChange, RebuildPlan};
use serde::Serialize;

#[derive(Debug, Default, Clone, Serialize)]
pub struct DiffSummary {
    pub by_change: BTreeMap<String, usize>,
    pub nodes_before: u64,
    pub nodes_after: u64,
    pub recovered: u64,
    pub lost: u64,
    pub bytes_to_write: u64,
}

pub fn change_label(c: DiffChange) -> &'static str {
    match c {
        DiffChange::Recovered => "Recuperados",
        DiffChange::MovedToRecovered => "Movidos a «Recovered Items»",
        DiffChange::Renamed => "Renombrados",
        DiffChange::ValenceFixed => "Recuentos de carpeta corregidos",
        DiffChange::Dropped => "Descartados",
        DiffChange::DataTruncated => "Datos truncados",
        DiffChange::DataCloned => "Datos duplicados (bloques compartidos)",
        DiffChange::UnchangedFlagged => "Para revisar",
    }
}

pub fn summarize(plan: &RebuildPlan) -> DiffSummary {
    let mut s = DiffSummary {
        nodes_before: plan.nodes_before,
        nodes_after: plan.nodes_after,
        recovered: plan.recovered_count,
        lost: plan.lost_count,
        bytes_to_write: plan.bytes_to_write(),
        ..Default::default()
    };
    for d in &plan.diff {
        *s.by_change.entry(change_label(d.change).to_string()).or_default() += 1;
    }
    s
}
