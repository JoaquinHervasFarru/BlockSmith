//! Acceso a datos de salud. Las funciones reciben una conexión ya bloqueada.

use blocksmith_smart::{Attribute, Bus, DeviceInfo, HealthSnapshot, NvmeHealth, Overall, Protocol, ReadOutcome};
use blocksmith_store::rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::rules::{label, AlertCandidate, Rule};
use crate::{now, Result};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSnapshot {
    pub id: i64,
    pub disk_id: i64,
    pub taken_at: String,
    pub overall: String,
    pub temperature_c: Option<i64>,
    pub power_on_hours: Option<i64>,
    pub percentage_used: Option<i64>,
    pub protocol: Option<Protocol>,
    pub attributes: Vec<Attribute>,
    pub nvme: Option<NvmeHealth>,
    pub reasons: Vec<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskRow {
    pub id: i64,
    pub path: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub firmware: Option<String>,
    pub bus: String,
    pub capacity_bytes: i64,
    pub is_boot_disk: bool,
    pub removable: bool,
    pub rotational: Option<bool>,
    /// ok | unsupported | permission | failed
    pub read_status: Option<String>,
    pub read_note: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
    pub latest: Option<StoredSnapshot>,
    pub open_alerts: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRow {
    pub id: i64,
    pub disk_id: i64,
    pub disk_model: Option<String>,
    pub disk_path: Option<String>,
    pub rule: String,
    pub rule_label: String,
    pub severity: String,
    pub message: String,
    pub created_at: String,
    pub acknowledged_at: Option<String>,
}

/// Estado de la última pasada de recolección (de la app o del agente).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollStatus {
    pub at: String,
    /// "app" | "agente"
    pub by: String,
    pub backend: String,
    pub disks: usize,
    pub read: usize,
    pub error: Option<String>,
}

pub const POLL_KEY: &str = "health:last_poll";

fn bus_str(b: Bus) -> &'static str {
    match b {
        Bus::Sata => "sata",
        Bus::Nvme => "nvme",
        Bus::Usb => "usb",
        Bus::Other => "other",
    }
}

pub fn upsert_disk(conn: &Connection, d: &DeviceInfo) -> Result<i64> {
    let key = d.stable_key();
    let ts = now();
    let existing: Option<i64> = conn.query_row("SELECT id FROM disks WHERE device_key = ?1", [&key], |r| r.get(0)).optional()?;
    match existing {
        Some(id) => {
            conn.execute(
                "UPDATE disks SET path = ?1, model = COALESCE(?2, model), serial = COALESCE(?3, serial), firmware = COALESCE(?4, firmware),
                    bus = ?5, capacity_bytes = ?6, is_boot_disk = ?7, removable = ?8, rotational = ?9, last_seen = ?10 WHERE id = ?11",
                params![d.path, d.model, d.serial, d.firmware, bus_str(d.bus), d.capacity_bytes as i64, d.is_boot_disk, d.removable,
                    d.rotational, ts, id],
            )?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO disks (serial, model, firmware, bus, capacity_bytes, is_boot_disk, smart_supported, first_seen, last_seen,
                    device_key, path, removable, rotational) VALUES (?1,?2,?3,?4,?5,?6,0,?7,?7,?8,?9,?10,?11)",
                params![d.serial, d.model, d.firmware, bus_str(d.bus), d.capacity_bytes as i64, d.is_boot_disk, ts, key, d.path,
                    d.removable, d.rotational],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// Guarda el resultado de una lectura. Devuelve el id de la instantánea si hubo datos.
pub fn record_outcome(conn: &Connection, disk_id: i64, outcome: &ReadOutcome) -> Result<Option<i64>> {
    let (status, note) = match outcome {
        ReadOutcome::Read(_) => ("ok", None),
        ReadOutcome::Unsupported(m) => ("unsupported", Some(m.as_str())),
        ReadOutcome::PermissionDenied(m) => ("permission", Some(m.as_str())),
        ReadOutcome::Failed(m) => ("failed", Some(m.as_str())),
    };
    conn.execute(
        "UPDATE disks SET read_status = ?1, read_note = ?2, smart_supported = CASE WHEN ?1 = 'ok' THEN 1 WHEN ?1 = 'unsupported' THEN 0 ELSE smart_supported END WHERE id = ?3",
        params![status, note, disk_id],
    )?;
    let ReadOutcome::Read(s) = outcome else { return Ok(None) };
    let raw = json!({ "protocol": s.protocol, "nvme": s.nvme, "reasons": s.reasons, "source": s.source });
    conn.execute(
        "INSERT INTO smart_snapshots (disk_id, taken_at, overall, temperature_c, power_on_hours, percentage_used, raw_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![disk_id, now(), s.overall.as_str(), s.temperature_c, s.power_on_hours.map(|v| v as i64),
            s.percentage_used, raw.to_string()],
    )?;
    let snap = conn.last_insert_rowid();
    let mut st = conn.prepare(
        "INSERT INTO smart_attributes (snapshot_id, attr_id, name, value, worst, threshold, raw_value, flags) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
    )?;
    for a in &s.attributes {
        st.execute(params![snap, a.id, a.name, a.value, a.worst, a.threshold, a.raw as i64, a.flags])?;
    }
    Ok(Some(snap))
}

fn snapshot_from_row(conn: &Connection, r: &Row<'_>) -> Result<StoredSnapshot> {
    let id: i64 = r.get(0)?;
    let raw: Option<String> = r.get(7)?;
    let raw: Value = raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    let mut st = conn.prepare_cached(
        "SELECT attr_id, name, flags, value, worst, threshold, raw_value FROM smart_attributes WHERE snapshot_id = ?1 ORDER BY attr_id",
    )?;
    let attributes = st
        .query_map([id], |a| {
            Ok(Attribute {
                id: a.get::<_, i64>(0)? as u8,
                name: a.get(1)?,
                flags: a.get::<_, Option<i64>>(2)?.unwrap_or(0) as u16,
                value: a.get::<_, Option<i64>>(3)?.unwrap_or(0) as u8,
                worst: a.get::<_, Option<i64>>(4)?.unwrap_or(0) as u8,
                threshold: a.get::<_, Option<i64>>(5)?.unwrap_or(0) as u8,
                raw: a.get::<_, Option<i64>>(6)?.unwrap_or(0) as u64,
            })
        })?
        .collect::<Result<_>>()?;
    Ok(StoredSnapshot {
        id,
        disk_id: r.get(1)?,
        taken_at: r.get(2)?,
        overall: r.get(3)?,
        temperature_c: r.get(4)?,
        power_on_hours: r.get(5)?,
        percentage_used: r.get(6)?,
        protocol: serde_json::from_value(raw["protocol"].clone()).ok(),
        attributes,
        nvme: serde_json::from_value(raw["nvme"].clone()).ok(),
        reasons: serde_json::from_value(raw["reasons"].clone()).unwrap_or_default(),
        source: raw["source"].as_str().map(str::to_string),
    })
}

const SNAP_COLS: &str = "id, disk_id, taken_at, overall, temperature_c, power_on_hours, percentage_used, raw_json";

pub fn latest(conn: &Connection, disk_id: i64) -> Result<Option<StoredSnapshot>> {
    let mut st = conn.prepare(&format!("SELECT {SNAP_COLS} FROM smart_snapshots WHERE disk_id = ?1 ORDER BY id DESC LIMIT 1"))?;
    let mut rows = st.query([disk_id])?;
    match rows.next()? {
        Some(r) => Ok(Some(snapshot_from_row(conn, r)?)),
        None => Ok(None),
    }
}

/// Últimas `limit` lecturas, de la más antigua a la más reciente.
pub fn history(conn: &Connection, disk_id: i64, limit: i64) -> Result<Vec<StoredSnapshot>> {
    let mut st = conn.prepare(&format!(
        "SELECT {SNAP_COLS} FROM (SELECT * FROM smart_snapshots WHERE disk_id = ?1 ORDER BY id DESC LIMIT ?2) ORDER BY id"
    ))?;
    let mut rows = st.query(params![disk_id, limit])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(snapshot_from_row(conn, r)?);
    }
    Ok(out)
}

/// Convierte una instantánea guardada de vuelta al modelo del motor (para las reglas).
pub fn to_health(s: &StoredSnapshot) -> HealthSnapshot {
    HealthSnapshot {
        protocol: s.protocol.unwrap_or(Protocol::Ata),
        overall: match s.overall.as_str() {
            "passed" => Overall::Passed,
            "warning" => Overall::Warning,
            "failing" => Overall::Failing,
            _ => Overall::Unknown,
        },
        temperature_c: s.temperature_c.map(|t| t as i16),
        power_on_hours: s.power_on_hours.map(|v| v as u64),
        percentage_used: s.percentage_used.map(|v| v as u8),
        attributes: s.attributes.clone(),
        nvme: s.nvme.clone(),
        source: s.source.clone().unwrap_or_default(),
        reasons: s.reasons.clone(),
    }
}

fn disk_from_row(r: &Row<'_>) -> Result<DiskRow> {
    Ok(DiskRow {
        id: r.get(0)?,
        path: r.get(1)?,
        model: r.get(2)?,
        serial: r.get(3)?,
        firmware: r.get(4)?,
        bus: r.get(5)?,
        capacity_bytes: r.get(6)?,
        is_boot_disk: r.get(7)?,
        removable: r.get(8)?,
        rotational: r.get(9)?,
        read_status: r.get(10)?,
        read_note: r.get(11)?,
        first_seen: r.get(12)?,
        last_seen: r.get(13)?,
        latest: None,
        open_alerts: r.get(14)?,
    })
}

const DISK_SELECT: &str = "SELECT d.id, d.path, d.model, d.serial, d.firmware, d.bus, d.capacity_bytes, d.is_boot_disk, d.removable,
    d.rotational, d.read_status, d.read_note, d.first_seen, d.last_seen,
    (SELECT COUNT(*) FROM alerts a WHERE a.disk_id = d.id AND a.acknowledged_at IS NULL) FROM disks d";

pub fn list_disks(conn: &Connection) -> Result<Vec<DiskRow>> {
    let mut st = conn.prepare(&format!("{DISK_SELECT} ORDER BY d.is_boot_disk DESC, d.path"))?;
    let mut disks: Vec<DiskRow> = st.query_map([], disk_from_row)?.collect::<Result<_>>()?;
    for d in &mut disks {
        d.latest = latest(conn, d.id)?;
    }
    Ok(disks)
}

pub fn get_disk(conn: &Connection, id: i64) -> Result<Option<DiskRow>> {
    let mut st = conn.prepare(&format!("{DISK_SELECT} WHERE d.id = ?1"))?;
    let Some(mut d) = st.query_row([id], disk_from_row).optional()? else { return Ok(None) };
    d.latest = latest(conn, id)?;
    Ok(Some(d))
}

/// Registra una alerta. Si ya hay una abierta de la misma regla para el disco, se
/// actualiza su mensaje y severidad en lugar de duplicarla. Devuelve el id si es nueva.
pub fn raise(conn: &Connection, disk_id: i64, c: &AlertCandidate) -> Result<Option<i64>> {
    let open: Option<i64> = conn
        .query_row(
            "SELECT id FROM alerts WHERE disk_id = ?1 AND rule = ?2 AND acknowledged_at IS NULL ORDER BY id DESC LIMIT 1",
            params![disk_id, c.rule],
            |r| r.get(0),
        )
        .optional()?;
    match open {
        Some(id) => {
            conn.execute("UPDATE alerts SET message = ?1, severity = ?2 WHERE id = ?3", params![c.message, c.severity, id])?;
            Ok(None)
        }
        None => {
            conn.execute(
                "INSERT INTO alerts (disk_id, rule, severity, message, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![disk_id, c.rule, c.severity, c.message, now()],
            )?;
            Ok(Some(conn.last_insert_rowid()))
        }
    }
}

pub fn alerts(conn: &Connection, open_only: bool, disk_id: Option<i64>) -> Result<Vec<AlertRow>> {
    let mut st = conn.prepare(
        "SELECT a.id, a.disk_id, d.model, d.path, a.rule, a.severity, a.message, a.created_at, a.acknowledged_at
         FROM alerts a JOIN disks d ON d.id = a.disk_id
         WHERE (?1 = 0 OR a.acknowledged_at IS NULL) AND (?2 IS NULL OR a.disk_id = ?2)
         ORDER BY a.acknowledged_at IS NOT NULL, CASE a.severity WHEN 'critical' THEN 0 WHEN 'minor' THEN 1 ELSE 2 END, a.id DESC",
    )?;
    let rows = st
        .query_map(params![open_only, disk_id], |r| {
            let rule: String = r.get(4)?;
            Ok(AlertRow {
                id: r.get(0)?,
                disk_id: r.get(1)?,
                disk_model: r.get(2)?,
                disk_path: r.get(3)?,
                rule_label: label(&rule).into(),
                rule,
                severity: r.get(5)?,
                message: r.get(6)?,
                created_at: r.get(7)?,
                acknowledged_at: r.get(8)?,
            })
        })?
        .collect::<Result<_>>()?;
    Ok(rows)
}

pub fn acknowledge(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("UPDATE alerts SET acknowledged_at = ?1 WHERE id = ?2 AND acknowledged_at IS NULL", params![now(), id])? > 0)
}

pub fn rules(conn: &Connection) -> Result<Vec<Rule>> {
    let mut st = conn.prepare("SELECT id, rule, enabled, threshold_json, poll_interval_min FROM alert_rules ORDER BY rule <> 'SMART_FAILING', id")?;
    let rows = st
        .query_map([], |r| {
            let t: Option<String> = r.get(3)?;
            Ok(Rule {
                id: r.get(0)?,
                rule: r.get(1)?,
                enabled: r.get(2)?,
                threshold: t.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({})),
                poll_interval_min: r.get(4)?,
            })
        })?
        .collect::<Result<_>>()?;
    Ok(rows)
}

pub fn update_rule(conn: &Connection, r: &Rule) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE alert_rules SET enabled = ?1, threshold_json = ?2, poll_interval_min = ?3 WHERE id = ?4",
        params![r.enabled, r.threshold.to_string(), r.poll_interval_min.clamp(15, 10_080), r.id],
    )? > 0)
}

/// Intervalo de sondeo efectivo: el menor de las reglas activas (mínimo 15 min).
pub fn poll_interval_min(conn: &Connection) -> Result<i64> {
    Ok(rules(conn)?.iter().filter(|r| r.enabled).map(|r| r.poll_interval_min).min().unwrap_or(360).max(15))
}
