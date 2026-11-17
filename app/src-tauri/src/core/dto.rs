//! Objetos que viajan a la interfaz (camelCase, como los tipos de `ui/src/types`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesDto {
    pub scan: &'static str,
    pub recover: &'static str,
    pub rebuild: &'static str,
    pub write_in_place: &'static str,
    /// Motivo legible cuando algo no está disponible.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeDto {
    pub id: i64,
    pub source_id: i64,
    pub partition_index: Option<i64>,
    pub offset_bytes: i64,
    pub length_bytes: i64,
    pub fs_type: String,
    pub name: Option<String>,
    pub block_size: i64,
    pub total_blocks: i64,
    pub free_blocks: i64,
    pub file_count: Option<i64>,
    pub folder_count: Option<i64>,
    pub case_sensitive: bool,
    pub journaled: bool,
    pub journal_state: String,
    pub encrypted: bool,
    pub capabilities: CapabilitiesDto,
    pub last_session: Option<SessionSummaryDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDto {
    pub id: i64,
    pub kind: String,
    pub path: String,
    pub size_bytes: i64,
    pub label: Option<String>,
    pub added_at: String,
    pub partition_scheme: String,
    pub exists: bool,
    pub volumes: Vec<VolumeDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeverityCounts {
    pub critical: i64,
    pub minor: i64,
    pub cosmetic: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryDto {
    pub id: i64,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub counts: SeverityCounts,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: i64,
    pub volume_id: i64,
    pub volume_name: Option<String>,
    pub fs_type: String,
    pub source_path: String,
    pub source_label: Option<String>,
    pub mode: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub catalog_nodes_total: Option<i64>,
    pub catalog_nodes_damaged: Option<i64>,
    pub fragmentation_pct: Option<f64>,
    pub file_count: Option<i64>,
    pub folder_count: Option<i64>,
    pub error: Option<String>,
    pub counts: SeverityCounts,
    pub latest_rebuild_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingDto {
    pub id: i64,
    pub session_id: i64,
    pub code: String,
    pub severity: String,
    pub object_type: String,
    pub cnid: Option<i64>,
    pub node_number: Option<i64>,
    pub path: Option<String>,
    pub message: String,
    pub fixed_in_rebuild: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeCount {
    pub change: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RebuildDto {
    pub id: i64,
    pub session_id: i64,
    pub status: String,
    pub target: String,
    pub experimental: bool,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub nodes_before: i64,
    pub nodes_after: i64,
    pub recovered_count: i64,
    pub lost_count: i64,
    pub output_path: Option<String>,
    pub verified: bool,
    pub bytes_to_write: i64,
    pub changes: Vec<ChangeCount>,
    pub volume_name: Option<String>,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffDto {
    pub id: i64,
    pub rebuild_id: i64,
    pub cnid: i64,
    pub change: String,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub item_type: String,
    pub size_bytes: Option<i64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNodeDto {
    pub id: u64,
    pub name: String,
    pub is_folder: bool,
    pub size_bytes: u64,
    pub child_count: u32,
    pub change: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResultDto {
    pub rebuild_id: i64,
    pub output_path: String,
    pub verified: bool,
    pub remaining_findings: Vec<FindingDto>,
    pub bytes_copied: i64,
    pub bytes_written: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportDto {
    pub id: i64,
    pub session_id: i64,
    pub format: String,
    pub path: String,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub output_dir: String,
    pub theme: String,
    pub log_level: String,
    pub export_encrypted_by_default: bool,
    /// Leer la salud de los discos al abrir la app si la última lectura es antigua.
    #[serde(default = "default_true")]
    pub health_check_on_start: bool,
    /// Carpeta de los registros de deshacer (vacío = carpeta de datos de la app).
    #[serde(default)]
    pub undo_dir: String,
}

fn default_true() -> bool {
    true
}
