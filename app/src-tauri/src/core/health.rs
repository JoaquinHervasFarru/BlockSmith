//! Salud de los discos en la app: lectura bajo demanda o al iniciar, discos, histórico,
//! alertas y reglas. Comparte base de datos y lógica con el agente (`blocksmith-health`).

use blocksmith_fs_core::Progress;
use blocksmith_health::collector::{last_poll, NewAlert};
use blocksmith_health::repo::{self, AlertRow, DiskRow, PollStatus, StoredSnapshot};
use blocksmith_health::{collect, CollectReport, Rule};
use blocksmith_smart::auto_backend;
use serde::Serialize;

use super::{Core, CoreError, CoreResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskDetailDto {
    pub disk: DiskRow,
    pub history: Vec<StoredSnapshot>,
    pub alerts: Vec<AlertRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthStatusDto {
    pub last_poll: Option<PollStatus>,
    /// Backend que usaría la app en esta plataforma.
    pub backend: String,
    pub poll_interval_min: i64,
    pub open_alerts: i64,
}

impl Core {
    /// Lee la salud de todos los discos y guarda el resultado.
    pub fn health_collect(&self, progress: &mut dyn Progress) -> CoreResult<CollectReport> {
        progress.phase("Leyendo la salud de los discos");
        let backend = auto_backend();
        let report = collect(backend.as_ref(), self.db(), "app", &|| progress.is_cancelled())?;
        if progress.is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        Ok(report)
    }

    /// ¿Toca leer al iniciar? Sí si nunca se leyó o la última lectura supera el intervalo.
    pub fn health_due(&self) -> CoreResult<bool> {
        if !self.settings()?.health_check_on_start {
            return Ok(false);
        }
        let interval = repo::poll_interval_min(&self.db().conn())?;
        Ok(match last_poll(self.db())? {
            None => true,
            Some(p) => chrono::DateTime::parse_from_rfc3339(&p.at)
                .map(|t| chrono::Utc::now().signed_duration_since(t).num_minutes() >= interval)
                .unwrap_or(true),
        })
    }

    pub fn health_disks(&self) -> CoreResult<Vec<DiskRow>> {
        Ok(repo::list_disks(&self.db().conn())?)
    }

    pub fn health_disk(&self, id: i64) -> CoreResult<DiskDetailDto> {
        let conn = self.db().conn();
        let disk =
            repo::get_disk(&conn, id)?.ok_or_else(|| CoreError::NotFound(format!("disco {id} no encontrado")))?;
        Ok(DiskDetailDto {
            history: repo::history(&conn, id, 90)?,
            alerts: repo::alerts(&conn, false, Some(id))?,
            disk,
        })
    }

    pub fn health_status(&self) -> CoreResult<HealthStatusDto> {
        let conn = self.db().conn();
        let poll_interval_min = repo::poll_interval_min(&conn)?;
        let open_alerts = repo::alerts(&conn, true, None)?.len() as i64;
        drop(conn);
        Ok(HealthStatusDto {
            last_poll: last_poll(self.db())?,
            backend: auto_backend().name().into(),
            poll_interval_min,
            open_alerts,
        })
    }

    pub fn alerts(&self, open_only: bool) -> CoreResult<Vec<AlertRow>> {
        Ok(repo::alerts(&self.db().conn(), open_only, None)?)
    }

    pub fn acknowledge_alert(&self, id: i64) -> CoreResult<()> {
        if !repo::acknowledge(&self.db().conn(), id)? {
            return Err(CoreError::NotFound(format!("la alerta {id} no existe o ya estaba confirmada")));
        }
        Ok(())
    }

    pub fn alert_rules(&self) -> CoreResult<Vec<Rule>> {
        Ok(repo::rules(&self.db().conn())?)
    }

    pub fn set_alert_rule(&self, rule: &Rule) -> CoreResult<Vec<Rule>> {
        if !rule.threshold.is_object() {
            return Err(CoreError::Invalid("el umbral debe ser un objeto JSON".into()));
        }
        if rule.threshold.as_object().is_some_and(|o| o.values().any(|v| v.as_i64().is_some_and(|n| n < 0))) {
            return Err(CoreError::Invalid("los umbrales no pueden ser negativos".into()));
        }
        if !repo::update_rule(&self.db().conn(), rule)? {
            return Err(CoreError::NotFound(format!("regla {} no encontrada", rule.id)));
        }
        self.alert_rules()
    }
}

/// Texto de una notificación de escritorio para un conjunto de alertas nuevas.
pub fn notification_text(alerts: &[NewAlert]) -> Option<(String, String)> {
    let first = alerts.first()?;
    if alerts.len() == 1 {
        Some((format!("{} · {}", first.title, first.disk), first.message.clone()))
    } else {
        Some((
            format!("{} alertas nuevas de salud de disco", alerts.len()),
            format!("{} · {}: {}", first.disk, first.title, first.message),
        ))
    }
}
