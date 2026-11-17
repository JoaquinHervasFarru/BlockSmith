//! Una pasada de recolección: enumera los discos, lee su salud, guarda la lectura y
//! evalúa las reglas contra la lectura anterior.

use blocksmith_smart::{HealthBackend, ReadOutcome};
use blocksmith_store::Database;
use serde::Serialize;

use crate::repo::{self, PollStatus, POLL_KEY};
use crate::rules::evaluate;
use crate::{now, Result};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAlert {
    pub id: i64,
    pub disk: String,
    pub severity: &'static str,
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectReport {
    pub disks: usize,
    pub read: usize,
    pub unsupported: usize,
    pub permission_denied: usize,
    pub failed: usize,
    pub new_alerts: Vec<NewAlert>,
    pub error: Option<String>,
}

/// Ejecuta una pasada y registra su estado. `by` identifica quién la lanza ("app" o "agente").
pub fn collect(backend: &dyn HealthBackend, db: &Database, by: &str, cancelled: &dyn Fn() -> bool) -> Result<CollectReport> {
    let mut report = CollectReport::default();
    let devices = match backend.devices() {
        Ok(d) => d,
        Err(e) => {
            report.error = Some(e);
            save_status(db, by, backend.name(), &report)?;
            return Ok(report);
        }
    };
    report.disks = devices.len();
    for dev in devices {
        if cancelled() {
            break;
        }
        // La lectura puede tardar: se hace sin bloquear la base de datos.
        let outcome = backend.read(&dev);
        let conn = db.conn();
        let disk_id = repo::upsert_disk(&conn, &dev)?;
        let prev = repo::latest(&conn, disk_id)?.map(|s| repo::to_health(&s));
        repo::record_outcome(&conn, disk_id, &outcome)?;
        match &outcome {
            ReadOutcome::Read(cur) => {
                report.read += 1;
                let rules = repo::rules(&conn)?;
                for cand in evaluate(prev.as_ref(), cur, &rules) {
                    if let Some(id) = repo::raise(&conn, disk_id, &cand)? {
                        report.new_alerts.push(NewAlert {
                            id,
                            disk: dev.model.clone().unwrap_or_else(|| dev.path.clone()),
                            severity: cand.severity,
                            title: crate::rules::label(&cand.rule).into(),
                            message: cand.message,
                        });
                    }
                }
            }
            ReadOutcome::Unsupported(_) => report.unsupported += 1,
            ReadOutcome::PermissionDenied(_) => report.permission_denied += 1,
            ReadOutcome::Failed(_) => report.failed += 1,
        }
    }
    save_status(db, by, backend.name(), &report)?;
    Ok(report)
}

fn save_status(db: &Database, by: &str, backend: &str, r: &CollectReport) -> Result<()> {
    let status = PollStatus { at: now(), by: by.into(), backend: backend.into(), disks: r.disks, read: r.read, error: r.error.clone() };
    db.set_setting(POLL_KEY, &serde_json::to_string(&status).unwrap_or_default())
}

pub fn last_poll(db: &Database) -> Result<Option<PollStatus>> {
    Ok(db.setting(POLL_KEY)?.and_then(|s| serde_json::from_str(&s).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use blocksmith_smart::backends::replay::{Replay, ReplayDevice, ReplayFile};
    use blocksmith_smart::{ata, Attribute, Bus, DeviceInfo, HealthSnapshot, Overall, Protocol};

    fn dev(path: &str, serial: Option<&str>) -> DeviceInfo {
        DeviceInfo {
            path: path.into(), model: Some(format!("Disco {path}")), serial: serial.map(Into::into), firmware: None,
            bus: Bus::Sata, capacity_bytes: 4_000_000_000_000, removable: false, rotational: Some(true), is_boot_disk: false,
        }
    }

    fn reading(realloc: u64, pending: u64) -> ReadOutcome {
        let attrs = vec![
            Attribute { id: 5, name: ata::attribute_name(5).into(), flags: 0, value: 199, worst: 199, threshold: 140, raw: realloc },
            Attribute { id: 9, name: ata::attribute_name(9).into(), flags: 0, value: 57, worst: 57, threshold: 0, raw: 31_870 },
            Attribute { id: 197, name: ata::attribute_name(197).into(), flags: 0, value: 200, worst: 200, threshold: 0, raw: pending },
        ];
        ReadOutcome::Read(HealthSnapshot {
            protocol: Protocol::Ata, overall: Overall::Unknown, temperature_c: Some(41), power_on_hours: Some(31_870),
            percentage_used: None, attributes: attrs, nvme: None, source: String::new(), reasons: vec![],
        })
    }

    fn backend() -> Replay {
        Replay::new(ReplayFile {
            devices: vec![
                ReplayDevice { info: dev("/dev/sda", Some("WX42")), sequence: vec![reading(0, 0), reading(8, 0), reading(24, 3), reading(24, 3)] },
                ReplayDevice { info: dev("/dev/sdc", None), sequence: vec![ReadOutcome::Unsupported("puente USB".into())] },
            ],
        })
    }

    #[test]
    fn trend_raises_deduplicated_alerts() {
        let db = Database::in_memory().unwrap();
        let b = backend();
        let never = || false;

        let r1 = collect(&b, &db, "agente", &never).unwrap();
        assert_eq!((r1.disks, r1.read, r1.unsupported), (2, 1, 1));
        assert!(r1.new_alerts.is_empty());

        let r2 = collect(&b, &db, "agente", &never).unwrap();
        assert_eq!(r2.new_alerts.len(), 1);
        assert_eq!(r2.new_alerts[0].message, "Sectores reasignados: 0 → 8 desde la lectura anterior.");

        // Tercera lectura: crece otra vez (la alerta abierta se actualiza, no se duplica) y aparecen pendientes.
        let r3 = collect(&b, &db, "agente", &never).unwrap();
        let titles: Vec<&str> = r3.new_alerts.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, vec!["Sectores pendientes de reasignación"]);
        let conn = db.conn();
        let open = repo::alerts(&conn, true, None).unwrap();
        assert_eq!(open.len(), 2);
        assert!(open.iter().any(|a| a.message == "Sectores reasignados: 8 → 24 desde la lectura anterior."));

        let disks = repo::list_disks(&conn).unwrap();
        assert_eq!(disks.len(), 2);
        let sda = disks.iter().find(|d| d.path.as_deref() == Some("/dev/sda")).unwrap();
        assert_eq!(sda.open_alerts, 2);
        let latest = sda.latest.as_ref().unwrap();
        assert_eq!((latest.overall.as_str(), latest.attributes.len()), ("warning", 3));
        assert_eq!(latest.reasons, vec!["24 sectores reasignados", "3 sectores pendientes de reasignación"]);
        let sdc = disks.iter().find(|d| d.path.as_deref() == Some("/dev/sdc")).unwrap();
        assert_eq!((sdc.read_status.as_deref(), sdc.read_note.as_deref()), (Some("unsupported"), Some("puente USB")));
        assert!(sdc.latest.is_none());
        assert_eq!(repo::history(&conn, sda.id, 10).unwrap().len(), 3);

        // Al confirmar, una nueva subida vuelve a avisar.
        for a in &open {
            assert!(repo::acknowledge(&conn, a.id).unwrap());
        }
        assert!(repo::alerts(&conn, true, None).unwrap().is_empty());
        drop(conn);
        let status = last_poll(&db).unwrap().unwrap();
        assert_eq!((status.by.as_str(), status.backend.as_str(), status.read), ("agente", "reproducción", 1));
    }

    #[test]
    fn rules_can_be_tuned() {
        let db = Database::in_memory().unwrap();
        let conn = db.conn();
        let mut rs = repo::rules(&conn).unwrap();
        assert_eq!(rs.len(), 6);
        let temp = rs.iter_mut().find(|r| r.rule == "TEMP_HIGH").unwrap();
        temp.threshold = serde_json::json!({ "celsius": 50 });
        temp.poll_interval_min = 5; // se ajusta al mínimo de 15
        assert!(repo::update_rule(&conn, temp).unwrap());
        assert_eq!(repo::poll_interval_min(&conn).unwrap(), 15);
        let again = repo::rules(&conn).unwrap();
        assert_eq!(again.iter().find(|r| r.rule == "TEMP_HIGH").unwrap().threshold["celsius"], 50);
    }

    #[test]
    fn backend_errors_are_recorded() {
        let db = Database::in_memory().unwrap();
        let r = collect(&Replay::from_file("/no/existe.json"), &db, "app", &|| false).unwrap();
        assert!(r.error.unwrap().contains("no se pudo leer"));
        assert!(last_poll(&db).unwrap().unwrap().error.is_some());
    }
}
