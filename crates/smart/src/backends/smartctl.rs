//! `smartctl --json` (smartmontools) como **binario externo**: se invoca como
//! proceso y se interpreta su JSON. No se enlaza ni se copia código GPL.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::{ata, evaluate_ata, evaluate_nvme, Attribute, Bus, DeviceInfo, HealthSnapshot, NvmeHealth, Protocol, ReadOutcome};

use super::HealthBackend;

const CANDIDATES: &[&str] = &[
    "/usr/sbin/smartctl",
    "/usr/bin/smartctl",
    "/usr/local/sbin/smartctl",
    "/usr/local/bin/smartctl",
    "/opt/homebrew/bin/smartctl",
    "/opt/homebrew/sbin/smartctl",
    "C:\\Program Files\\smartmontools\\bin\\smartctl.exe",
];

pub struct Smartctl {
    bin: PathBuf,
}

/// Marcador cuando no hay ningún backend disponible en la plataforma.
pub struct Missing;

impl HealthBackend for Missing {
    fn name(&self) -> &'static str {
        "ninguno"
    }
    fn devices(&self) -> Result<Vec<DeviceInfo>, String> {
        Err("instala smartmontools (smartctl) para leer la salud de los discos en esta plataforma".into())
    }
    fn read(&self, _: &DeviceInfo) -> ReadOutcome {
        ReadOutcome::Unsupported("smartctl no está instalado".into())
    }
}

impl Smartctl {
    pub fn locate() -> Option<Self> {
        let from_path = std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p).map(|d| d.join(if cfg!(windows) { "smartctl.exe" } else { "smartctl" })).find(|p| p.is_file())
        });
        from_path.or_else(|| CANDIDATES.iter().map(PathBuf::from).find(|p| p.is_file())).map(|bin| Self { bin })
    }

    pub fn with_binary(bin: impl AsRef<Path>) -> Self {
        Self { bin: bin.as_ref().to_path_buf() }
    }

    fn run(&self, args: &[&str]) -> Result<Value, String> {
        let out = Command::new(&self.bin).args(args).output().map_err(|e| format!("no se pudo ejecutar smartctl: {e}"))?;
        serde_json::from_slice(&out.stdout).map_err(|e| format!("salida de smartctl no válida: {e}"))
    }
}

impl HealthBackend for Smartctl {
    fn name(&self) -> &'static str {
        "smartctl"
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, String> {
        let scan = self.run(&["--json=c", "--scan-open"])?;
        let mut out = Vec::new();
        for d in scan["devices"].as_array().into_iter().flatten() {
            let Some(name) = d["name"].as_str() else { continue };
            let ty = d["type"].as_str().unwrap_or("auto");
            match self.run(&["--json=c", "-i", "-d", ty, name]) {
                Ok(info) => out.push(device_from_json(name, &info)),
                Err(_) => out.push(DeviceInfo {
                    path: name.into(), model: None, serial: None, firmware: None, bus: Bus::Other, capacity_bytes: 0,
                    removable: false, rotational: None, is_boot_disk: false,
                }),
            }
        }
        Ok(out)
    }

    fn read(&self, dev: &DeviceInfo) -> ReadOutcome {
        match self.run(&["--json=c", "-a", &dev.path]) {
            Ok(v) => outcome_from_json(&v),
            Err(e) => ReadOutcome::Failed(e),
        }
    }
}

fn str_field(v: &Value, k: &str) -> Option<String> {
    v[k].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn device_from_json(path: &str, v: &Value) -> DeviceInfo {
    let protocol = v["device"]["protocol"].as_str().unwrap_or("");
    let ty = v["device"]["type"].as_str().unwrap_or("");
    let bus = if protocol.eq_ignore_ascii_case("nvme") {
        Bus::Nvme
    } else if ty.starts_with("usb") || ty == "sntjmicron" || ty.starts_with("sntasm") {
        Bus::Usb
    } else if protocol.eq_ignore_ascii_case("ata") {
        Bus::Sata
    } else {
        Bus::Other
    };
    DeviceInfo {
        path: path.to_string(),
        model: str_field(v, "model_name"),
        serial: str_field(v, "serial_number"),
        firmware: str_field(v, "firmware_version"),
        bus,
        capacity_bytes: v["user_capacity"]["bytes"].as_u64().or_else(|| v["nvme_total_capacity"].as_u64()).unwrap_or(0),
        removable: false,
        rotational: v["rotation_rate"].as_u64().map(|r| r > 0),
        is_boot_disk: false,
    }
}

/// Interpreta la salida de `smartctl --json -a`.
pub fn outcome_from_json(v: &Value) -> ReadOutcome {
    let messages: Vec<String> = v["smartctl"]["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["string"].as_str().map(str::to_string))
        .collect();
    let joined = messages.join("; ");
    if joined.contains("Permission denied") || joined.contains("Operation not permitted") {
        return ReadOutcome::PermissionDenied("smartctl necesita permisos de administrador para leer este disco".into());
    }
    if let Some(log) = v.get("nvme_smart_health_information_log").filter(|l| l.is_object()) {
        let n = |k: &str| log[k].as_u64().unwrap_or(0);
        let health = NvmeHealth {
            critical_warning: n("critical_warning") as u8,
            temperature_k: log["temperature"].as_u64().map(|c| (c + 273) as u16).unwrap_or(0),
            available_spare: n("available_spare") as u8,
            spare_threshold: n("available_spare_threshold") as u8,
            percentage_used: n("percentage_used") as u8,
            data_units_read: n("data_units_read"),
            data_units_written: n("data_units_written"),
            power_cycles: n("power_cycles"),
            power_on_hours: n("power_on_hours"),
            unsafe_shutdowns: n("unsafe_shutdowns"),
            media_errors: n("media_errors"),
            error_log_entries: n("num_err_log_entries"),
        };
        let (overall, reasons) = evaluate_nvme(&health);
        return ReadOutcome::Read(HealthSnapshot {
            protocol: Protocol::Nvme,
            overall,
            temperature_c: crate::nvme::temperature_c(&health),
            power_on_hours: Some(health.power_on_hours),
            percentage_used: Some(health.percentage_used),
            attributes: vec![],
            nvme: Some(health),
            source: "smartctl".into(),
            reasons,
        });
    }
    let table = v["ata_smart_attributes"]["table"].as_array();
    let Some(table) = table.filter(|t| !t.is_empty()) else {
        if v["smart_support"]["available"].as_bool() == Some(false) {
            return ReadOutcome::Unsupported("el dispositivo no admite S.M.A.R.T.".into());
        }
        return ReadOutcome::Unsupported(if joined.is_empty() {
            "smartctl no devolvió atributos (el puente USB puede no transmitirlos)".into()
        } else {
            joined
        });
    };
    let attrs: Vec<Attribute> = table
        .iter()
        .filter_map(|a| {
            let id = a["id"].as_u64()? as u8;
            Some(Attribute {
                id,
                name: a["name"].as_str().map(str::to_string).unwrap_or_else(|| ata::attribute_name(id).into()),
                flags: a["flags"]["value"].as_u64().unwrap_or(0) as u16,
                value: a["value"].as_u64().unwrap_or(0) as u8,
                worst: a["worst"].as_u64().unwrap_or(0) as u8,
                threshold: a["thresh"].as_u64().unwrap_or(0) as u8,
                raw: a["raw"]["value"].as_u64().unwrap_or(0),
            })
        })
        .collect();
    let (mut overall, mut reasons) = evaluate_ata(&attrs);
    if v["smart_status"]["passed"].as_bool() == Some(false) {
        overall = crate::Overall::Failing;
        reasons.insert(0, "el propio disco informa de un fallo inminente".into());
    }
    ReadOutcome::Read(HealthSnapshot {
        protocol: Protocol::Ata,
        overall,
        temperature_c: v["temperature"]["current"].as_i64().map(|t| t as i16).or_else(|| ata::temperature(&attrs)),
        power_on_hours: v["power_on_time"]["hours"].as_u64().or_else(|| ata::power_on_hours(&attrs)),
        percentage_used: None,
        attributes: attrs,
        nvme: None,
        source: "smartctl".into(),
        reasons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Overall;
    use serde_json::json;

    #[test]
    fn ata_output() {
        let v = json!({
            "smartctl": { "exit_status": 0, "messages": [] },
            "device": { "name": "/dev/sdb", "type": "sat", "protocol": "ATA" },
            "model_name": "WDC WD40EFRX-68N32N0", "serial_number": "WX42D81F7KLP", "firmware_version": "82.00A82",
            "user_capacity": { "blocks": 7814037168u64, "bytes": 4000787030016u64 },
            "rotation_rate": 5400,
            "smart_status": { "passed": true },
            "ata_smart_attributes": { "revision": 16, "table": [
                { "id": 5, "name": "Reallocated_Sector_Ct", "value": 199, "worst": 199, "thresh": 140, "flags": { "value": 51 }, "raw": { "value": 24, "string": "24" } },
                { "id": 9, "name": "Power_On_Hours", "value": 57, "worst": 57, "thresh": 0, "flags": { "value": 50 }, "raw": { "value": 31870, "string": "31870" } },
                { "id": 194, "name": "Temperature_Celsius", "value": 110, "worst": 101, "thresh": 0, "flags": { "value": 34 }, "raw": { "value": 40, "string": "40" } }
            ]},
            "power_on_time": { "hours": 31870 },
            "temperature": { "current": 40 }
        });
        let dev = device_from_json("/dev/sdb", &v);
        assert_eq!((dev.bus, dev.capacity_bytes, dev.rotational), (Bus::Sata, 4_000_787_030_016, Some(true)));
        let ReadOutcome::Read(s) = outcome_from_json(&v) else { panic!() };
        assert_eq!(s.overall, Overall::Warning);
        assert_eq!(s.reasons, vec!["24 sectores reasignados"]);
        assert_eq!((s.temperature_c, s.power_on_hours), (Some(40), Some(31_870)));
    }

    #[test]
    fn nvme_output() {
        let v = json!({
            "device": { "name": "/dev/nvme0", "type": "nvme", "protocol": "NVMe" },
            "model_name": "Samsung SSD 980 PRO 1TB", "nvme_total_capacity": 1000204886016u64,
            "smart_status": { "passed": true },
            "nvme_smart_health_information_log": {
                "critical_warning": 0, "temperature": 38, "available_spare": 100, "available_spare_threshold": 10,
                "percentage_used": 4, "data_units_read": 9000000, "data_units_written": 12345678, "power_cycles": 1204,
                "power_on_hours": 6120, "unsafe_shutdowns": 31, "media_errors": 0, "num_err_log_entries": 2
            }
        });
        assert_eq!(device_from_json("/dev/nvme0", &v).bus, Bus::Nvme);
        let ReadOutcome::Read(s) = outcome_from_json(&v) else { panic!() };
        assert_eq!((s.overall, s.temperature_c, s.percentage_used), (Overall::Passed, Some(38), Some(4)));
        assert_eq!(s.nvme.unwrap().power_on_hours, 6120);
    }

    #[test]
    fn failing_permission_and_usb() {
        let failing = json!({ "smart_status": { "passed": false }, "ata_smart_attributes": { "table": [
            { "id": 9, "value": 75, "worst": 75, "thresh": 0, "raw": { "value": 100 } } ] } });
        let ReadOutcome::Read(s) = outcome_from_json(&failing) else { panic!() };
        assert_eq!(s.overall, Overall::Failing);

        let denied = json!({ "smartctl": { "messages": [{ "string": "Smartctl open device: /dev/sda failed: Permission denied", "severity": "error" }] } });
        assert!(matches!(outcome_from_json(&denied), ReadOutcome::PermissionDenied(_)));

        // Salida real de smartctl 7.4 con un disco virtio (capturada en el entorno de desarrollo).
        let real: Value = serde_json::from_str(r#"{"json_format_version":[1,0],"smartctl":{"version":[7,4],"pre_release":false,"svn_revision":"5530","platform_info":"x86_64-linux-6.18.44-fc-v553","build_info":"(local build)","argv":["smartctl","--json=c","-a","/dev/vda"],"messages":[{"string":"/dev/vda: Unable to detect device type","severity":"error"}],"exit_status":1},"local_time":{"time_t":1794943880,"asctime":"Tue Nov 17 19:31:20 2026 UTC"}}"#).unwrap();
        assert!(matches!(outcome_from_json(&real), ReadOutcome::Unsupported(m) if m.contains("Unable to detect device type")));

        let usb = json!({ "smartctl": { "messages": [{ "string": "/dev/sdc: Unknown USB bridge [0x152d:0x0578]", "severity": "error" }] } });
        assert!(matches!(outcome_from_json(&usb), ReadOutcome::Unsupported(m) if m.contains("USB bridge")));
    }
}
