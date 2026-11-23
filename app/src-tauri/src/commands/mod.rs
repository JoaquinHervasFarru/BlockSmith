//! Comandos IPC. Las consultas son asíncronas para no bloquear la ventana; las
//! operaciones largas devuelven un id de trabajo y avisan por eventos.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, State};

use crate::core::dto::*;
use crate::core::Core;
use crate::db::DbStatus;
use crate::jobs::JobManager;

type Cmd<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: &'static str,
    pub platform: &'static str,
    pub phase: &'static str,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo { version: env!("CARGO_PKG_VERSION"), platform: std::env::consts::OS, phase: "F3" }
}

#[tauri::command]
pub async fn db_status(core: State<'_, Arc<Core>>) -> Cmd<DbStatus> {
    core.db().status().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_image(core: State<'_, Arc<Core>>, path: String) -> Cmd<SourceDto> {
    Ok(core.open_image(&PathBuf::from(path))?)
}

#[tauri::command]
pub async fn list_sources(core: State<'_, Arc<Core>>) -> Cmd<Vec<SourceDto>> {
    Ok(core.list_sources()?)
}

#[tauri::command]
pub async fn remove_source(core: State<'_, Arc<Core>>, id: i64) -> Cmd<()> {
    Ok(core.remove_source(id)?)
}

#[tauri::command]
pub async fn get_volume(core: State<'_, Arc<Core>>, id: i64) -> Cmd<VolumeDto> {
    Ok(core.get_volume(id)?)
}

#[tauri::command]
pub async fn list_sessions(core: State<'_, Arc<Core>>) -> Cmd<Vec<SessionDto>> {
    Ok(core.list_sessions()?)
}

#[tauri::command]
pub async fn get_session(core: State<'_, Arc<Core>>, id: i64) -> Cmd<SessionDto> {
    Ok(core.get_session(id)?)
}

#[tauri::command]
pub async fn get_findings(core: State<'_, Arc<Core>>, session_id: i64) -> Cmd<Vec<FindingDto>> {
    Ok(core.findings(session_id)?)
}

#[tauri::command]
pub async fn get_rebuild(core: State<'_, Arc<Core>>, id: i64) -> Cmd<RebuildDto> {
    Ok(core.get_rebuild(id)?)
}

#[tauri::command]
pub async fn get_diff(core: State<'_, Arc<Core>>, rebuild_id: i64) -> Cmd<Vec<DiffDto>> {
    Ok(core.diff(rebuild_id)?)
}

#[tauri::command]
pub async fn tree_children(
    core: State<'_, Arc<Core>>,
    side: String,
    id: i64,
    parent: Option<u64>,
) -> Cmd<Vec<TreeNodeDto>> {
    Ok(core.tree_children(&side, id, parent)?)
}

#[tauri::command]
pub async fn start_scan(
    app: AppHandle,
    core: State<'_, Arc<Core>>,
    jobs: State<'_, JobManager>,
    volume_id: i64,
) -> Cmd<u64> {
    let core = core.inner().clone();
    Ok(jobs.spawn(app, "scan", move |p| Ok(json!({ "sessionId": core.scan(volume_id, p)? }))))
}

#[tauri::command]
pub async fn start_plan(
    app: AppHandle,
    core: State<'_, Arc<Core>>,
    jobs: State<'_, JobManager>,
    session_id: i64,
) -> Cmd<u64> {
    let core = core.inner().clone();
    Ok(jobs.spawn(app, "plan", move |p| Ok(json!({ "rebuildId": core.plan(session_id, p)? }))))
}

#[tauri::command]
pub async fn start_export(
    app: AppHandle,
    core: State<'_, Arc<Core>>,
    jobs: State<'_, JobManager>,
    rebuild_id: i64,
    out_path: String,
) -> Cmd<u64> {
    let core = core.inner().clone();
    Ok(jobs.spawn(app, "export", move |p| {
        let r = core.export(rebuild_id, &PathBuf::from(out_path), p)?;
        Ok(serde_json::to_value(r).unwrap_or_default())
    }))
}

#[tauri::command]
pub fn cancel_job(jobs: State<'_, JobManager>, job_id: u64) -> bool {
    jobs.cancel(job_id)
}

#[tauri::command]
pub async fn suggest_export_path(core: State<'_, Arc<Core>>, rebuild_id: i64) -> Cmd<String> {
    let r = core.get_rebuild(rebuild_id)?;
    let dir = PathBuf::from(core.settings()?.output_dir);
    let stem = PathBuf::from(&r.source_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "volumen".into());
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{stem}-reparada.img") } else { format!("{stem}-reparada-{n}.img") };
        let p = dir.join(name);
        if !p.exists() {
            return Ok(p.to_string_lossy().to_string());
        }
        n += 1;
    }
}

#[tauri::command]
pub async fn write_report(
    core: State<'_, Arc<Core>>,
    session_id: i64,
    format: String,
    path: Option<String>,
) -> Cmd<ReportDto> {
    Ok(core.write_report(session_id, &format, path.as_deref().map(std::path::Path::new))?)
}

#[tauri::command]
pub async fn list_reports(core: State<'_, Arc<Core>>, session_id: i64) -> Cmd<Vec<ReportDto>> {
    Ok(core.reports(session_id)?)
}

#[tauri::command]
pub async fn get_settings(core: State<'_, Arc<Core>>) -> Cmd<SettingsDto> {
    Ok(core.settings()?)
}

#[tauri::command]
pub async fn set_settings(core: State<'_, Arc<Core>>, settings: SettingsDto) -> Cmd<SettingsDto> {
    if !settings.output_dir.is_empty() {
        std::fs::create_dir_all(&settings.output_dir)
            .map_err(|e| format!("no se puede crear la carpeta de salida: {e}"))?;
    }
    Ok(core.set_settings(&settings)?)
}

// ── Salud de los discos ───────────────────────────────────────────────────

use blocksmith_health::repo::{AlertRow, DiskRow};
use blocksmith_health::Rule;

use crate::core::health::{notification_text, DiskDetailDto, HealthStatusDto};

/// Muestra una notificación de escritorio si hay alertas nuevas.
pub fn notify_alerts(app: &AppHandle, alerts: &[blocksmith_health::collector::NewAlert]) {
    use tauri_plugin_notification::NotificationExt;
    if let Some((title, body)) = notification_text(alerts) {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

#[tauri::command]
pub async fn health_disks(core: State<'_, Arc<Core>>) -> Cmd<Vec<DiskRow>> {
    Ok(core.health_disks()?)
}

#[tauri::command]
pub async fn health_disk(core: State<'_, Arc<Core>>, id: i64) -> Cmd<DiskDetailDto> {
    Ok(core.health_disk(id)?)
}

#[tauri::command]
pub async fn health_status(core: State<'_, Arc<Core>>) -> Cmd<HealthStatusDto> {
    Ok(core.health_status()?)
}

#[tauri::command]
pub async fn start_health_read(app: AppHandle, core: State<'_, Arc<Core>>, jobs: State<'_, JobManager>) -> Cmd<u64> {
    let core = core.inner().clone();
    let handle = app.clone();
    Ok(jobs.spawn(app, "health", move |p| {
        let r = core.health_collect(p)?;
        notify_alerts(&handle, &r.new_alerts);
        Ok(serde_json::to_value(r).unwrap_or_default())
    }))
}

#[tauri::command]
pub async fn list_alerts(core: State<'_, Arc<Core>>, open_only: bool) -> Cmd<Vec<AlertRow>> {
    Ok(core.alerts(open_only)?)
}

#[tauri::command]
pub async fn ack_alert(core: State<'_, Arc<Core>>, id: i64) -> Cmd<()> {
    Ok(core.acknowledge_alert(id)?)
}

#[tauri::command]
pub async fn get_alert_rules(core: State<'_, Arc<Core>>) -> Cmd<Vec<Rule>> {
    Ok(core.alert_rules()?)
}

#[tauri::command]
pub async fn set_alert_rule(core: State<'_, Arc<Core>>, rule: Rule) -> Cmd<Vec<Rule>> {
    Ok(core.set_alert_rule(&rule)?)
}

// ── Escritura directa (F5) ────────────────────────────────────────────────

use crate::core::repair::{PreflightDto, TxDto};

#[tauri::command]
pub async fn preflight(core: State<'_, Arc<Core>>, rebuild_id: i64) -> Cmd<PreflightDto> {
    Ok(core.preflight(rebuild_id)?)
}

#[tauri::command]
pub async fn start_repair(
    app: AppHandle,
    core: State<'_, Arc<Core>>,
    jobs: State<'_, JobManager>,
    rebuild_id: i64,
    confirm: String,
) -> Cmd<u64> {
    let core = core.inner().clone();
    Ok(jobs.spawn(app, "repair", move |p| {
        let r = core.repair_in_place(rebuild_id, &confirm, p)?;
        let mut v = serde_json::to_value(r).unwrap_or_default();
        v["rebuildId"] = json!(rebuild_id);
        Ok(v)
    }))
}

#[tauri::command]
pub async fn list_transactions(core: State<'_, Arc<Core>>) -> Cmd<Vec<TxDto>> {
    Ok(core.transactions()?)
}

#[tauri::command]
pub async fn start_tx_rollback(
    app: AppHandle,
    core: State<'_, Arc<Core>>,
    jobs: State<'_, JobManager>,
    tx_id: i64,
    force: bool,
) -> Cmd<u64> {
    let core = core.inner().clone();
    Ok(jobs.spawn(app, "rollback", move |p| {
        Ok(serde_json::to_value(core.rollback_tx(tx_id, force, p)?).unwrap_or_default())
    }))
}

#[tauri::command]
pub async fn start_tx_complete(
    app: AppHandle,
    core: State<'_, Arc<Core>>,
    jobs: State<'_, JobManager>,
    tx_id: i64,
) -> Cmd<u64> {
    let core = core.inner().clone();
    Ok(jobs.spawn(app, "complete", move |p| Ok(serde_json::to_value(core.complete_tx(tx_id, p)?).unwrap_or_default())))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceOption {
    pub path: String,
    pub model: Option<String>,
    pub size_bytes: u64,
    pub bus: String,
    pub is_boot: bool,
    pub readable: bool,
}

/// Discos físicos que se pueden abrir como origen.
#[tauri::command]
pub async fn list_open_devices() -> Cmd<Vec<DeviceOption>> {
    #[cfg(target_os = "linux")]
    {
        use blocksmith_smart::HealthBackend;
        let devs = blocksmith_smart::backends::linux::LinuxNative::default().devices()?;
        Ok(devs
            .into_iter()
            .map(|d| DeviceOption {
                readable: std::fs::File::open(&d.path).is_ok(),
                path: d.path,
                model: d.model,
                size_bytes: d.capacity_bytes,
                bus: format!("{:?}", d.bus).to_lowercase(),
                is_boot: d.is_boot_disk,
            })
            .collect())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("la lista de discos aún no está disponible en este sistema; abre el dispositivo por su ruta (p. ej. /dev/disk4) o crea una imagen".into())
    }
}
