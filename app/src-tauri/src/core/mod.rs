//! Lógica de la aplicación, independiente de Tauri: registro de imágenes, análisis,
//! planes de reconstrucción, exportación, historial, informes y ajustes.
//!
//! Todas las operaciones son bloqueantes; los comandos Tauri las ejecutan en hilos
//! de trabajo. La base de datos solo se bloquea durante cada consulta.

pub mod dto;
pub mod health;
pub mod probe;
pub mod report;
pub mod snapshot;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use blocksmith_fs_core::{DiffChange, Finding, ObjectType, Progress, RebuildPlan, Severity};
use blocksmith_fs_hfsplus::rebuild::plan_rebuild;
use blocksmith_fs_hfsplus::{scan_volume, Volume};
use blocksmith_io::{open_image, BlockDevice, Slice};
use blocksmith_rebuilder::export_image;
use rusqlite::{params, OptionalExtension, Row};
use thiserror::Error;

use crate::db::Database;
use dto::*;
use snapshot::{TreeIndex, TreeSnapshot};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("operación cancelada")]
    Cancelled,
    #[error("base de datos: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("lectura: {0}")]
    Io(#[from] blocksmith_io::IoError),
    #[error("{0}")]
    Fs(#[from] blocksmith_fs_core::FsError),
    #[error("exportación: {0}")]
    Export(#[from] blocksmith_rebuilder::ExportError),
    #[error("{0}")]
    Os(#[from] std::io::Error),
}

impl From<CoreError> for String {
    fn from(e: CoreError) -> Self {
        e.to_string()
    }
}

pub type CoreResult<T> = Result<T, CoreError>;

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Critical => "critical",
        Severity::Minor => "minor",
        Severity::Cosmetic => "cosmetic",
    }
}

fn object_type_str(o: ObjectType) -> &'static str {
    match o {
        ObjectType::Node => "node",
        ObjectType::Folder => "folder",
        ObjectType::File => "file",
        ObjectType::Extent => "extent",
        ObjectType::Bitmap => "bitmap",
        ObjectType::Header => "header",
        ObjectType::Encryption => "encryption",
    }
}

pub fn change_str(c: DiffChange) -> &'static str {
    match c {
        DiffChange::Recovered => "recovered",
        DiffChange::MovedToRecovered => "moved_to_recovered",
        DiffChange::Renamed => "renamed",
        DiffChange::ValenceFixed => "valence_fixed",
        DiffChange::Dropped => "dropped",
        DiffChange::DataTruncated => "data_truncated",
        DiffChange::DataCloned => "data_cloned",
        DiffChange::UnchangedFlagged => "unchanged_flagged",
    }
}

fn capabilities(fs_type: &str) -> CapabilitiesDto {
    match fs_type {
        "hfsplus" | "hfsx" => CapabilitiesDto {
            scan: "yes",
            recover: "no",
            rebuild: "yes",
            write_in_place: "no",
            note: Some("La reconstrucción se exporta a una imagen nueva; la escritura directa llega en una fase posterior.".into()),
        },
        other => CapabilitiesDto {
            scan: "no",
            recover: "no",
            rebuild: "no",
            write_in_place: "no",
            note: Some(format!(
                "El análisis de {} llegará en una fase posterior.",
                match other {
                    "apfs" => "APFS",
                    "ntfs" => "NTFS",
                    "ext4" => "ext4",
                    _ => "este sistema de archivos",
                }
            )),
        },
    }
}

/// Progreso que además corta si se cancela (lo implementan los trabajos).
pub struct NullProgress;
impl Progress for NullProgress {
    fn phase(&mut self, _: &str) {}
    fn advance(&mut self, _: u64, _: u64) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

pub struct Core {
    db: Database,
    data_dir: PathBuf,
    default_output_dir: PathBuf,
    plans: Mutex<HashMap<i64, Arc<RebuildPlan>>>,
    trees: Mutex<HashMap<PathBuf, Arc<TreeIndex>>>,
}

/// Datos mínimos para reabrir un volumen.
struct VolumeLocation {
    path: String,
    offset: u64,
    length: u64,
    fs_type: String,
}

impl Core {
    pub fn new(db: Database, data_dir: PathBuf, default_output_dir: PathBuf) -> Self {
        Self { db, data_dir, default_output_dir, plans: Mutex::default(), trees: Mutex::default() }
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    fn session_tree_path(&self, id: i64) -> PathBuf {
        self.data_dir.join("snapshots").join(format!("session-{id}.tree.zst"))
    }

    fn rebuild_tree_path(&self, id: i64) -> PathBuf {
        self.data_dir.join("snapshots").join(format!("rebuild-{id}.tree.zst"))
    }

    // ── Orígenes ────────────────────────────────────────────────────────────

    /// Registra una imagen (o devuelve la ya registrada) y detecta sus volúmenes.
    pub fn open_image(&self, path: &Path) -> CoreResult<SourceDto> {
        let path = path.canonicalize().map_err(|e| CoreError::Invalid(format!("no se puede abrir {}: {e}", path.display())))?;
        let path_str = path.to_string_lossy().to_string();
        // Ojo: la conexión se libera antes de llamar a get_source (el Mutex no es reentrante).
        let existing = self.db.conn().query_row("SELECT id FROM sources WHERE path = ?1", [&path_str], |r| r.get::<_, i64>(0)).optional()?;
        if let Some(id) = existing {
            return self.get_source(id);
        }
        let dev = open_image(&path)?;
        let raw = blocksmith_io::RawImage::open(&path)?;
        let kind = if blocksmith_io::udif::has_koly_trailer(&raw)? { "udif" } else { "raw_image" };
        let (scheme, vols) = probe::probe(dev.as_ref())?;
        let label = path.file_name().map(|n| n.to_string_lossy().to_string());
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO sources (disk_id, kind, path, size_bytes, label, added_at) VALUES (NULL, ?1, ?2, ?3, ?4, ?5)",
            params![kind, path_str, dev.len() as i64, label, now()],
        )?;
        let source_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT OR REPLACE INTO settings (key, value_json) VALUES (?1, ?2)",
            params![format!("scheme:{source_id}"), serde_json::to_string(probe::scheme_label(scheme)).unwrap_or_default()],
        )?;
        for v in vols {
            tx.execute(
                "INSERT INTO volumes (source_id, partition_index, offset_bytes, length_bytes, fs_type, name, block_size,
                    total_blocks, free_blocks, file_count, folder_count, case_sensitive, journaled, journal_state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    source_id, v.partition_index, v.offset as i64, v.length as i64, v.fs_type, v.name, v.block_size,
                    v.total_blocks as i64, v.free_blocks as i64, v.file_count, v.folder_count, v.case_sensitive, v.journaled,
                    if v.journaled { "clean" } else { "none" }
                ],
            )?;
        }
        tx.commit()?;
        drop(conn);
        self.get_source(source_id)
    }

    pub fn list_sources(&self) -> CoreResult<Vec<SourceDto>> {
        let ids: Vec<i64> = {
            let conn = self.db.conn();
            let mut st = conn.prepare("SELECT id FROM sources ORDER BY added_at DESC, id DESC")?;
            let rows = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            rows
        };
        ids.into_iter().map(|id| self.get_source(id)).collect()
    }

    pub fn get_source(&self, id: i64) -> CoreResult<SourceDto> {
        let conn = self.db.conn();
        let mut src = conn
            .query_row("SELECT id, kind, path, size_bytes, label, added_at FROM sources WHERE id = ?1", [id], |r| {
                Ok(SourceDto {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    path: r.get(2)?,
                    size_bytes: r.get(3)?,
                    label: r.get(4)?,
                    added_at: r.get(5)?,
                    partition_scheme: String::new(),
                    exists: false,
                    volumes: vec![],
                })
            })
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("origen {id} no encontrado")))?;
        src.partition_scheme = conn
            .query_row("SELECT value_json FROM settings WHERE key = ?1", [format!("scheme:{id}")], |r| r.get::<_, String>(0))
            .optional()?
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_else(|| "none".into());
        src.exists = Path::new(&src.path).exists();
        let vol_ids: Vec<i64> = {
            let mut st = conn.prepare("SELECT id FROM volumes WHERE source_id = ?1 ORDER BY offset_bytes")?;
            let rows = st.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;
            rows
        };
        drop(conn);
        src.volumes = vol_ids.into_iter().map(|v| self.get_volume(v)).collect::<CoreResult<_>>()?;
        Ok(src)
    }

    /// Quita un origen de la biblioteca (no borra el archivo de imagen).
    pub fn remove_source(&self, id: i64) -> CoreResult<()> {
        let n = self.db.conn().execute("DELETE FROM sources WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("origen {id} no encontrado")));
        }
        self.db.conn().execute("DELETE FROM settings WHERE key = ?1", [format!("scheme:{id}")])?;
        Ok(())
    }

    pub fn get_volume(&self, id: i64) -> CoreResult<VolumeDto> {
        let mut v = self
            .db
            .conn()
            .query_row(
                "SELECT id, source_id, partition_index, offset_bytes, length_bytes, fs_type, name, block_size, total_blocks,
                        free_blocks, file_count, folder_count, case_sensitive, journaled, journal_state, encrypted
                 FROM volumes WHERE id = ?1",
                [id],
                |r| {
                    let fs: String = r.get(5)?;
                    Ok(VolumeDto {
                        id: r.get(0)?,
                        source_id: r.get(1)?,
                        partition_index: r.get(2)?,
                        offset_bytes: r.get(3)?,
                        length_bytes: r.get(4)?,
                        capabilities: capabilities(&fs),
                        fs_type: fs,
                        name: r.get(6)?,
                        block_size: r.get::<_, Option<i64>>(7)?.unwrap_or(0),
                        total_blocks: r.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        free_blocks: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        file_count: r.get(10)?,
                        folder_count: r.get(11)?,
                        case_sensitive: r.get(12)?,
                        journaled: r.get(13)?,
                        journal_state: r.get(14)?,
                        encrypted: r.get(15)?,
                        last_session: None,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("volumen {id} no encontrado")))?;
        let last: Option<i64> = self
            .db
            .conn()
            .query_row("SELECT id FROM sessions WHERE volume_id = ?1 ORDER BY id DESC LIMIT 1", [id], |r| r.get(0))
            .optional()?;
        if let Some(sid) = last {
            let s = self.get_session(sid)?;
            v.last_session = Some(SessionSummaryDto {
                id: s.id,
                status: s.status,
                started_at: s.started_at,
                finished_at: s.finished_at,
                counts: s.counts,
            });
        }
        Ok(v)
    }

    fn location(&self, volume_id: i64) -> CoreResult<VolumeLocation> {
        self.db
            .conn()
            .query_row(
                "SELECT s.path, v.offset_bytes, v.length_bytes, v.fs_type FROM volumes v JOIN sources s ON s.id = v.source_id WHERE v.id = ?1",
                [volume_id],
                |r| Ok(VolumeLocation { path: r.get(0)?, offset: r.get::<_, i64>(1)? as u64, length: r.get::<_, i64>(2)? as u64, fs_type: r.get(3)? }),
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("volumen {volume_id} no encontrado")))
    }

    fn open_location(loc: &VolumeLocation) -> CoreResult<Box<dyn BlockDevice>> {
        if !Path::new(&loc.path).exists() {
            return Err(CoreError::Invalid(format!("la imagen ya no está en {}", loc.path)));
        }
        Ok(open_image(&loc.path)?)
    }

    // ── Análisis ────────────────────────────────────────────────────────────

    /// Escanea un volumen y guarda la sesión. Devuelve el id de sesión.
    pub fn scan(&self, volume_id: i64, progress: &mut dyn Progress) -> CoreResult<i64> {
        let loc = self.location(volume_id)?;
        if !matches!(loc.fs_type.as_str(), "hfsplus" | "hfsx") {
            return Err(CoreError::Invalid(capabilities(&loc.fs_type).note.unwrap_or_default()));
        }
        let caps = serde_json::to_string(&capabilities(&loc.fs_type)).unwrap_or_default();
        self.db.conn().execute(
            "INSERT INTO sessions (volume_id, mode, status, capabilities_json, started_at, app_version) VALUES (?1, 'image', 'scanning', ?2, ?3, ?4)",
            params![volume_id, caps, now(), APP_VERSION],
        )?;
        let session_id = self.db.conn().last_insert_rowid();
        match self.scan_inner(volume_id, session_id, &loc, progress) {
            Ok(()) => Ok(session_id),
            Err(e) => {
                let status = if matches!(e, CoreError::Cancelled | CoreError::Fs(blocksmith_fs_core::FsError::Cancelled)) {
                    "cancelled"
                } else {
                    "failed"
                };
                self.db.conn().execute(
                    "UPDATE sessions SET status = ?1, finished_at = ?2, error = ?3 WHERE id = ?4",
                    params![status, now(), e.to_string(), session_id],
                )?;
                Err(e)
            }
        }
    }

    fn scan_inner(&self, volume_id: i64, session_id: i64, loc: &VolumeLocation, progress: &mut dyn Progress) -> CoreResult<()> {
        let dev = Self::open_location(loc)?;
        let slice = Slice::new(dev.as_ref(), loc.offset, loc.length)?;
        let vol = Volume::open(&slice)?.ok_or_else(|| CoreError::Invalid("ya no hay un volumen HFS+ en esa posición".into()))?;
        let scan = scan_volume(&vol, progress)?;
        let snap = TreeSnapshot { root_id: 2, entries: scan.catalog.entries() };
        let snap_path = self.session_tree_path(session_id);
        snap.save(&snap_path)?;
        let r = &scan.result;
        let pending = r.findings.iter().any(|f| f.code == "HFS_JOURNAL_PENDING");
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        insert_findings(&tx, session_id, &r.findings)?;
        tx.execute(
            "UPDATE sessions SET status = 'scanned', finished_at = ?1, catalog_nodes_total = ?2, catalog_nodes_damaged = ?3,
                 fragmentation_pct = ?4, snapshot_path = ?5 WHERE id = ?6",
            params![now(), r.stats.nodes_total as i64, r.stats.nodes_damaged as i64, r.stats.fragmentation_pct as f64,
                snap_path.to_string_lossy(), session_id],
        )?;
        tx.execute(
            "UPDATE volumes SET name = COALESCE(?1, name), file_count = ?2, folder_count = ?3, case_sensitive = ?4,
                 journal_state = CASE WHEN journaled = 1 THEN ?5 ELSE journal_state END WHERE id = ?6",
            params![r.info.name, r.stats.files as i64, r.stats.folders as i64, r.info.case_sensitive,
                if pending { "pending" } else { "clean" }, volume_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_sessions(&self) -> CoreResult<Vec<SessionDto>> {
        let ids: Vec<i64> = {
            let conn = self.db.conn();
            let mut st = conn.prepare("SELECT id FROM sessions ORDER BY id DESC")?;
            let rows = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            rows
        };
        ids.into_iter().map(|id| self.get_session(id)).collect()
    }

    pub fn get_session(&self, id: i64) -> CoreResult<SessionDto> {
        let conn = self.db.conn();
        let mut s = conn
            .query_row(
                "SELECT s.id, s.volume_id, v.name, v.fs_type, src.path, src.label, s.mode, s.status, s.started_at, s.finished_at,
                        s.catalog_nodes_total, s.catalog_nodes_damaged, s.fragmentation_pct, v.file_count, v.folder_count, s.error
                 FROM sessions s JOIN volumes v ON v.id = s.volume_id JOIN sources src ON src.id = v.source_id WHERE s.id = ?1",
                [id],
                |r| {
                    Ok(SessionDto {
                        id: r.get(0)?,
                        volume_id: r.get(1)?,
                        volume_name: r.get(2)?,
                        fs_type: r.get(3)?,
                        source_path: r.get(4)?,
                        source_label: r.get(5)?,
                        mode: r.get(6)?,
                        status: r.get(7)?,
                        started_at: r.get(8)?,
                        finished_at: r.get(9)?,
                        catalog_nodes_total: r.get(10)?,
                        catalog_nodes_damaged: r.get(11)?,
                        fragmentation_pct: r.get(12)?,
                        file_count: r.get(13)?,
                        folder_count: r.get(14)?,
                        error: r.get(15)?,
                        counts: SeverityCounts { critical: 0, minor: 0, cosmetic: 0 },
                        latest_rebuild_id: None,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("sesión {id} no encontrada")))?;
        let mut st = conn.prepare("SELECT severity, COUNT(*) FROM findings WHERE session_id = ?1 GROUP BY severity")?;
        for row in st.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (sev, n) = row?;
            match sev.as_str() {
                "critical" => s.counts.critical = n,
                "minor" => s.counts.minor = n,
                _ => s.counts.cosmetic = n,
            }
        }
        s.latest_rebuild_id =
            conn.query_row("SELECT id FROM rebuilds WHERE session_id = ?1 ORDER BY id DESC LIMIT 1", [id], |r| r.get(0)).optional()?;
        Ok(s)
    }

    pub fn findings(&self, session_id: i64) -> CoreResult<Vec<FindingDto>> {
        let conn = self.db.conn();
        let mut st = conn.prepare(
            "SELECT id, session_id, code, severity, object_type, cnid, node_number, path, message, fixed_in_rebuild
             FROM findings WHERE session_id = ?1
             ORDER BY CASE severity WHEN 'critical' THEN 0 WHEN 'minor' THEN 1 ELSE 2 END, id",
        )?;
        let rows = st.query_map([session_id], finding_from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    // ── Reconstrucción ──────────────────────────────────────────────────────

    fn compute_plan(&self, volume_id: i64, progress: &mut dyn Progress) -> CoreResult<RebuildPlan> {
        let loc = self.location(volume_id)?;
        let dev = Self::open_location(&loc)?;
        let slice = Slice::new(dev.as_ref(), loc.offset, loc.length)?;
        let vol = Volume::open(&slice)?.ok_or_else(|| CoreError::Invalid("ya no hay un volumen HFS+ en esa posición".into()))?;
        let scan = scan_volume(&vol, progress)?;
        progress.phase("Plan de reconstrucción");
        Ok(plan_rebuild(&vol, &scan)?)
    }

    /// Calcula el plan de reconstrucción de una sesión y lo guarda. Devuelve el id.
    pub fn plan(&self, session_id: i64, progress: &mut dyn Progress) -> CoreResult<i64> {
        let session = self.get_session(session_id)?;
        let plan = self.compute_plan(session.volume_id, progress)?;
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO rebuilds (session_id, status, target, experimental, started_at, nodes_before, nodes_after, recovered_count, lost_count)
             VALUES (?1, 'planned', 'new_image', ?2, ?3, ?4, ?5, ?6, ?7)",
            params![session_id, plan.experimental, now(), plan.nodes_before as i64, plan.nodes_after as i64,
                plan.recovered_count as i64, plan.lost_count as i64],
        )?;
        let rebuild_id = tx.last_insert_rowid();
        {
            let mut ins = tx.prepare(
                "INSERT INTO diff_entries (rebuild_id, cnid, change, old_path, new_path, item_type, size_bytes, note) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )?;
            for d in &plan.diff {
                ins.execute(params![rebuild_id, d.object_id as i64, change_str(d.change), d.old_path, d.new_path,
                    if d.is_folder { "folder" } else { "file" }, d.size_bytes as i64, d.note])?;
            }
        }
        tx.execute("UPDATE sessions SET status = 'rebuilt' WHERE id = ?1 AND status = 'scanned'", [session_id])?;
        tx.commit()?;
        drop(conn);
        TreeSnapshot { root_id: plan.root_id, entries: plan.tree_after.clone() }.save(&self.rebuild_tree_path(rebuild_id))?;
        self.plans.lock().expect("caché de planes").insert(rebuild_id, Arc::new(plan));
        Ok(rebuild_id)
    }

    pub fn get_rebuild(&self, id: i64) -> CoreResult<RebuildDto> {
        let conn = self.db.conn();
        let mut r = conn
            .query_row(
                "SELECT r.id, r.session_id, r.status, r.target, r.experimental, r.started_at, r.finished_at, r.nodes_before,
                        r.nodes_after, r.recovered_count, r.lost_count, r.output_path, r.verified, v.name, src.path
                 FROM rebuilds r JOIN sessions s ON s.id = r.session_id JOIN volumes v ON v.id = s.volume_id
                 JOIN sources src ON src.id = v.source_id WHERE r.id = ?1",
                [id],
                |r| {
                    Ok(RebuildDto {
                        id: r.get(0)?,
                        session_id: r.get(1)?,
                        status: r.get(2)?,
                        target: r.get(3)?,
                        experimental: r.get(4)?,
                        started_at: r.get(5)?,
                        finished_at: r.get(6)?,
                        nodes_before: r.get::<_, Option<i64>>(7)?.unwrap_or(0),
                        nodes_after: r.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        recovered_count: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        lost_count: r.get::<_, Option<i64>>(10)?.unwrap_or(0),
                        output_path: r.get(11)?,
                        verified: r.get(12)?,
                        bytes_to_write: 0,
                        changes: vec![],
                        volume_name: r.get(13)?,
                        source_path: r.get(14)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("reconstrucción {id} no encontrada")))?;
        let mut st = conn.prepare("SELECT change, COUNT(*) FROM diff_entries WHERE rebuild_id = ?1 GROUP BY change ORDER BY COUNT(*) DESC")?;
        r.changes = st
            .query_map([id], |row| Ok(ChangeCount { change: row.get(0)?, count: row.get(1)? }))?
            .collect::<Result<_, _>>()?;
        drop(st);
        drop(conn);
        if let Some(p) = self.plans.lock().expect("caché de planes").get(&id) {
            r.bytes_to_write = p.bytes_to_write() as i64;
        }
        Ok(r)
    }

    pub fn diff(&self, rebuild_id: i64) -> CoreResult<Vec<DiffDto>> {
        let conn = self.db.conn();
        let mut st = conn.prepare(
            "SELECT id, rebuild_id, cnid, change, old_path, new_path, item_type, size_bytes, note FROM diff_entries WHERE rebuild_id = ?1 ORDER BY id",
        )?;
        let rows = st
            .query_map([rebuild_id], |r| {
                Ok(DiffDto {
                    id: r.get(0)?,
                    rebuild_id: r.get(1)?,
                    cnid: r.get(2)?,
                    change: r.get(3)?,
                    old_path: r.get(4)?,
                    new_path: r.get(5)?,
                    item_type: r.get(6)?,
                    size_bytes: r.get(7)?,
                    note: r.get(8)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    fn tree(&self, path: PathBuf) -> CoreResult<Arc<TreeIndex>> {
        if let Some(t) = self.trees.lock().expect("caché de árboles").get(&path) {
            return Ok(t.clone());
        }
        let t = Arc::new(TreeIndex::new(TreeSnapshot::load(&path).map_err(|e| {
            CoreError::NotFound(format!("no se encontró el árbol guardado ({e}); vuelve a escanear el volumen"))
        })?));
        let mut cache = self.trees.lock().expect("caché de árboles");
        if cache.len() > 8 {
            cache.clear();
        }
        cache.insert(path, t.clone());
        Ok(t)
    }

    /// Hijos de `parent` (o la raíz si es `None`) en el árbol original de una sesión
    /// o en el reconstruido de un plan, con el tipo de cambio de cada elemento.
    pub fn tree_children(&self, side: &str, id: i64, parent: Option<u64>) -> CoreResult<Vec<TreeNodeDto>> {
        let (path, rebuild_id) = match side {
            "original" => {
                let rid = self.get_session(id)?.latest_rebuild_id;
                (self.session_tree_path(id), rid)
            }
            "rebuilt" => (self.rebuild_tree_path(id), Some(id)),
            _ => return Err(CoreError::Invalid(format!("lado de árbol desconocido: {side}"))),
        };
        let tree = self.tree(path)?;
        let changes: HashMap<u64, String> = match rebuild_id {
            Some(r) => self.diff(r)?.into_iter().map(|d| (d.cnid as u64, d.change)).collect(),
            None => HashMap::new(),
        };
        let node = |e: &blocksmith_fs_core::TreeEntry| TreeNodeDto {
            id: e.id,
            name: e.name.clone(),
            is_folder: e.is_folder,
            size_bytes: e.size_bytes,
            child_count: tree.child_count(e.id),
            change: changes.get(&e.id).cloned(),
        };
        Ok(match parent {
            None => tree.root().map(node).into_iter().collect(),
            Some(p) => tree.children(p).map(node).collect(),
        })
    }

    /// Exporta la imagen reparada y la verifica.
    pub fn export(&self, rebuild_id: i64, out: &Path, progress: &mut dyn Progress) -> CoreResult<ExportResultDto> {
        let rebuild = self.get_rebuild(rebuild_id)?;
        let session = self.get_session(rebuild.session_id)?;
        let loc = self.location(session.volume_id)?;
        // Como la salida no puede existir, tampoco puede ser la imagen de origen.
        if out.exists() {
            return Err(CoreError::Invalid(format!("{} ya existe; elige otro nombre", out.display())));
        }
        let cached = self.plans.lock().expect("caché de planes").get(&rebuild_id).cloned();
        let plan = match cached {
            Some(p) => p,
            None => {
                // La app se reinició desde el plan: se recalcula y se exige que coincida.
                let p = self.compute_plan(session.volume_id, progress)?;
                let stored = self.diff(rebuild_id)?.len();
                if p.diff.len() != stored {
                    return Err(CoreError::Invalid(
                        "el volumen cambió desde que se calculó el plan; vuelve a analizarlo".into(),
                    ));
                }
                Arc::new(p)
            }
        };
        if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        self.db.conn().execute("UPDATE rebuilds SET status = 'exporting' WHERE id = ?1", [rebuild_id])?;
        let dev = Self::open_location(&loc)?;
        let report = match export_image(dev.as_ref(), loc.offset, loc.length, &plan, out, progress) {
            Ok(r) => r,
            Err(e) => {
                let cancelled = matches!(e, blocksmith_rebuilder::ExportError::Cancelled);
                self.db.conn().execute(
                    "UPDATE rebuilds SET status = ?1 WHERE id = ?2",
                    params![if cancelled { "planned" } else { "failed" }, rebuild_id],
                )?;
                return Err(if cancelled { CoreError::Cancelled } else { e.into() });
            }
        };
        progress.phase("Verificando la imagen reparada");
        let fixed = open_image(out)?;
        let slice = Slice::new(fixed.as_ref(), loc.offset, loc.length)?;
        let vol = Volume::open(&slice)?.ok_or_else(|| CoreError::Invalid("la imagen exportada no contiene el volumen".into()))?;
        let verify = scan_volume(&vol, progress)?;
        let remaining: Vec<FindingDto> = verify
            .result
            .findings
            .iter()
            .filter(|f| f.severity != Severity::Cosmetic)
            .enumerate()
            .map(|(i, f)| finding_dto(i as i64, rebuild.session_id, f))
            .collect();
        let verified = remaining.is_empty();
        let out_str = out.to_string_lossy().to_string();
        self.db.conn().execute(
            "UPDATE rebuilds SET status = 'done', finished_at = ?1, output_path = ?2, verified = ?3 WHERE id = ?4",
            params![now(), out_str, verified, rebuild_id],
        )?;
        self.db.conn().execute("UPDATE sessions SET status = 'exported' WHERE id = ?1", [rebuild.session_id])?;
        if verified {
            self.db.conn().execute("UPDATE findings SET fixed_in_rebuild = 1 WHERE session_id = ?1", [rebuild.session_id])?;
        }
        Ok(ExportResultDto {
            rebuild_id,
            output_path: out_str,
            verified,
            remaining_findings: remaining,
            bytes_copied: report.bytes_copied as i64,
            bytes_written: report.bytes_written as i64,
        })
    }

    // ── Informes ────────────────────────────────────────────────────────────

    pub fn write_report(&self, session_id: i64, format: &str, path: Option<&Path>) -> CoreResult<ReportDto> {
        let session = self.get_session(session_id)?;
        let findings = self.findings(session_id)?;
        let rebuild = session.latest_rebuild_id.map(|r| self.get_rebuild(r)).transpose()?;
        let diff = session.latest_rebuild_id.map(|r| self.diff(r)).transpose()?.unwrap_or_default();
        let content = match format {
            "json" => report::json(&session, &findings, rebuild.as_ref(), &diff),
            "html" => report::html(&session, &findings, rebuild.as_ref(), &diff),
            other => return Err(CoreError::Invalid(format!("formato de informe desconocido: {other}"))),
        };
        let path = match path {
            Some(p) => p.to_path_buf(),
            None => {
                let dir = PathBuf::from(self.settings()?.output_dir).join("informes");
                std::fs::create_dir_all(&dir)?;
                dir.join(format!("blocksmith-sesion-{session_id}.{format}"))
            }
        };
        std::fs::write(&path, content)?;
        let path_str = path.to_string_lossy().to_string();
        let generated = now();
        self.db.conn().execute(
            "INSERT INTO reports (session_id, format, path, generated_at) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, format, path_str, generated],
        )?;
        Ok(ReportDto { id: self.db.conn().last_insert_rowid(), session_id, format: format.into(), path: path_str, generated_at: generated })
    }

    pub fn reports(&self, session_id: i64) -> CoreResult<Vec<ReportDto>> {
        let conn = self.db.conn();
        let mut st = conn.prepare("SELECT id, session_id, format, path, generated_at FROM reports WHERE session_id = ?1 ORDER BY id DESC")?;
        let rows = st
            .query_map([session_id], |r| {
                Ok(ReportDto { id: r.get(0)?, session_id: r.get(1)?, format: r.get(2)?, path: r.get(3)?, generated_at: r.get(4)? })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    // ── Ajustes ─────────────────────────────────────────────────────────────

    pub fn settings(&self) -> CoreResult<SettingsDto> {
        let stored: Option<String> = self
            .db
            .conn()
            .query_row("SELECT value_json FROM settings WHERE key = 'app'", [], |r| r.get(0))
            .optional()?;
        let defaults = SettingsDto {
            output_dir: self.default_output_dir.to_string_lossy().to_string(),
            theme: "system".into(),
            log_level: "info".into(),
            export_encrypted_by_default: true,
            health_check_on_start: true,
        };
        Ok(stored.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(defaults))
    }

    pub fn set_settings(&self, s: &SettingsDto) -> CoreResult<SettingsDto> {
        if !matches!(s.theme.as_str(), "system" | "light" | "dark") {
            return Err(CoreError::Invalid(format!("tema no válido: {}", s.theme)));
        }
        if !matches!(s.log_level.as_str(), "error" | "warn" | "info" | "debug") {
            return Err(CoreError::Invalid(format!("nivel de registro no válido: {}", s.log_level)));
        }
        if s.output_dir.trim().is_empty() {
            return Err(CoreError::Invalid("la carpeta de salida no puede estar vacía".into()));
        }
        self.db.conn().execute(
            "INSERT OR REPLACE INTO settings (key, value_json) VALUES ('app', ?1)",
            [serde_json::to_string(s).unwrap_or_default()],
        )?;
        self.settings()
    }
}

fn insert_findings(tx: &rusqlite::Transaction<'_>, session_id: i64, findings: &[Finding]) -> rusqlite::Result<()> {
    let mut st = tx.prepare(
        "INSERT INTO findings (session_id, code, severity, object_type, cnid, node_number, path, message, details_json, fixed_in_rebuild)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
    )?;
    for f in findings {
        st.execute(params![
            session_id,
            f.code,
            severity_str(f.severity),
            object_type_str(f.object_type),
            f.object_id.map(|v| v as i64),
            f.node_number.map(|v| v as i64),
            f.path,
            f.message,
            f.details.to_string()
        ])?;
    }
    Ok(())
}

fn finding_from_row(r: &Row<'_>) -> rusqlite::Result<FindingDto> {
    Ok(FindingDto {
        id: r.get(0)?,
        session_id: r.get(1)?,
        code: r.get(2)?,
        severity: r.get(3)?,
        object_type: r.get(4)?,
        cnid: r.get(5)?,
        node_number: r.get(6)?,
        path: r.get(7)?,
        message: r.get(8)?,
        fixed_in_rebuild: r.get(9)?,
    })
}

fn finding_dto(id: i64, session_id: i64, f: &Finding) -> FindingDto {
    FindingDto {
        id,
        session_id,
        code: f.code.clone(),
        severity: severity_str(f.severity).into(),
        object_type: object_type_str(f.object_type).into(),
        cnid: f.object_id.map(|v| v as i64),
        node_number: f.node_number.map(|v| v as i64),
        path: f.path.clone(),
        message: f.message.clone(),
        fixed_in_rebuild: false,
    }
}
