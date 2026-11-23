//! Reglas de alerta. Cada una se evalúa sobre la lectura actual y, si existe, la anterior.

use blocksmith_smart::{ata, nvme, HealthSnapshot, Overall};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: i64,
    pub rule: String,
    pub enabled: bool,
    pub threshold: Value,
    pub poll_interval_min: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertCandidate {
    pub rule: String,
    /// critical | minor | cosmetic
    pub severity: &'static str,
    pub message: String,
}

pub fn label(rule: &str) -> &'static str {
    match rule {
        "SMART_FAILING" => "El disco predice un fallo",
        "REALLOCATED_INCREASE" => "Aumentan los sectores reasignados",
        "PENDING_SECTORS" => "Sectores pendientes de reasignación",
        "NVME_CRITICAL_WARNING" => "Aviso crítico NVMe",
        "NVME_WEAR" => "Desgaste NVMe",
        "TEMP_HIGH" => "Temperatura alta",
        _ => "Regla desconocida",
    }
}

fn num(v: &Value, k: &str, default: u64) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(default)
}

fn ata_raw(s: &HealthSnapshot, id: u8) -> Option<u64> {
    s.attributes.iter().find(|a| a.id == id).map(ata::counter)
}

/// Alertas que disparan `cur` (y `prev`, si existe) con las reglas activas.
pub fn evaluate(prev: Option<&HealthSnapshot>, cur: &HealthSnapshot, rules: &[Rule]) -> Vec<AlertCandidate> {
    let mut out = Vec::new();
    let mut push = |rule: &str, severity: &'static str, message: String| {
        out.push(AlertCandidate { rule: rule.into(), severity, message })
    };
    for r in rules.iter().filter(|r| r.enabled) {
        let t = &r.threshold;
        match r.rule.as_str() {
            "SMART_FAILING" if cur.overall == Overall::Failing => push(
                &r.rule,
                "critical",
                format!("{}. Copia o clona los datos antes de cualquier reparación.", cur.reasons.join("; ")),
            ),
            "REALLOCATED_INCREASE" => {
                if let (Some(before), Some(now)) = (prev.and_then(|p| ata_raw(p, 5)), ata_raw(cur, 5)) {
                    let delta = num(t, "delta", 1);
                    if now >= before + delta.max(1) {
                        let sev = if now - before >= 50 { "critical" } else { "minor" };
                        push(
                            &r.rule,
                            sev,
                            format!("Sectores reasignados: {before} → {now} desde la lectura anterior."),
                        );
                    }
                }
            }
            "PENDING_SECTORS" => {
                if let Some(n) = ata_raw(cur, 197).filter(|n| *n >= num(t, "min", 1).max(1)) {
                    let sev = if n >= 100 { "critical" } else { "minor" };
                    push(&r.rule, sev, format!("{n} sectores pendientes de reasignación."));
                }
            }
            "NVME_CRITICAL_WARNING" => {
                if let Some(n) = &cur.nvme {
                    let bits = n.critical_warning & (num(t, "mask", 0xFF) as u8);
                    if bits != 0 {
                        push(
                            &r.rule,
                            "critical",
                            format!("Aviso crítico: {}.", nvme::warning_reasons(bits).join(", ")),
                        );
                    }
                }
            }
            "NVME_WEAR" => {
                if let Some(p) = cur.percentage_used.filter(|p| *p as u64 >= num(t, "percentage_used", 90)) {
                    push(
                        &r.rule,
                        if p >= 100 { "critical" } else { "minor" },
                        format!("Vida útil estimada consumida: {p} %."),
                    );
                }
            }
            "TEMP_HIGH" => {
                if let Some(c) = cur.temperature_c.filter(|c| *c as u64 >= num(t, "celsius", 60)) {
                    push(&r.rule, "minor", format!("Temperatura de {c} °C."));
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use blocksmith_smart::{evaluate_ata, Attribute, NvmeHealth, Protocol};
    use serde_json::json;

    fn rules() -> Vec<Rule> {
        [
            ("SMART_FAILING", json!({})),
            ("REALLOCATED_INCREASE", json!({ "delta": 1 })),
            ("PENDING_SECTORS", json!({ "min": 1 })),
            ("NVME_CRITICAL_WARNING", json!({ "mask": 255 })),
            ("NVME_WEAR", json!({ "percentage_used": 90 })),
            ("TEMP_HIGH", json!({ "celsius": 60 })),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (r, t))| Rule { id: i as i64, rule: r.into(), enabled: true, threshold: t, poll_interval_min: 360 })
        .collect()
    }

    fn ata(realloc: u64, pending: u64, temp: i16, value5: u8) -> HealthSnapshot {
        let attrs = vec![
            Attribute {
                id: 5,
                name: "Reallocated_Sector_Ct".into(),
                flags: 0,
                value: value5,
                worst: value5,
                threshold: 10,
                raw: realloc,
            },
            Attribute {
                id: 197,
                name: "Current_Pending_Sector".into(),
                flags: 0,
                value: 100,
                worst: 100,
                threshold: 0,
                raw: pending,
            },
        ];
        let (overall, reasons) = evaluate_ata(&attrs);
        HealthSnapshot {
            protocol: Protocol::Ata,
            overall,
            temperature_c: Some(temp),
            power_on_hours: None,
            percentage_used: None,
            attributes: attrs,
            nvme: None,
            source: "prueba".into(),
            reasons,
        }
    }

    fn codes(v: &[AlertCandidate]) -> Vec<&str> {
        v.iter().map(|a| a.rule.as_str()).collect()
    }

    #[test]
    fn healthy_disk_raises_nothing() {
        assert!(evaluate(None, &ata(0, 0, 38, 100), &rules()).is_empty());
    }

    #[test]
    fn reallocated_needs_previous_reading_and_growth() {
        assert!(evaluate(None, &ata(24, 0, 38, 100), &rules()).iter().all(|a| a.rule != "REALLOCATED_INCREASE"));
        let a = evaluate(Some(&ata(10, 0, 38, 100)), &ata(24, 0, 38, 100), &rules());
        let r = a.iter().find(|a| a.rule == "REALLOCATED_INCREASE").unwrap();
        assert_eq!(
            (r.severity, r.message.as_str()),
            ("minor", "Sectores reasignados: 10 → 24 desde la lectura anterior.")
        );
        assert!(evaluate(Some(&ata(24, 0, 38, 100)), &ata(24, 0, 38, 100), &rules())
            .iter()
            .all(|a| a.rule != "REALLOCATED_INCREASE"));
    }

    #[test]
    fn failing_pending_and_temperature() {
        let a = evaluate(None, &ata(3_960, 304, 61, 7), &rules());
        assert_eq!(codes(&a), vec!["SMART_FAILING", "PENDING_SECTORS", "TEMP_HIGH"]);
        assert_eq!(a[1].severity, "critical");
        // Reglas desactivadas o con umbral más alto no disparan.
        let mut rs = rules();
        rs[5].threshold = json!({ "celsius": 70 });
        rs[0].enabled = false;
        assert_eq!(codes(&evaluate(None, &ata(3_960, 304, 61, 7), &rs)), vec!["PENDING_SECTORS"]);
    }

    #[test]
    fn nvme_rules() {
        let n = NvmeHealth {
            critical_warning: 0b10,
            available_spare: 100,
            spare_threshold: 10,
            percentage_used: 93,
            ..Default::default()
        };
        let s = HealthSnapshot {
            protocol: Protocol::Nvme,
            overall: Overall::Failing,
            temperature_c: Some(45),
            power_on_hours: None,
            percentage_used: Some(93),
            attributes: vec![],
            nvme: Some(n),
            source: "prueba".into(),
            reasons: vec!["temperatura fuera de rango".into()],
        };
        let a = evaluate(None, &s, &rules());
        assert_eq!(codes(&a), vec!["SMART_FAILING", "NVME_CRITICAL_WARNING", "NVME_WEAR"]);
        assert!(a[1].message.contains("temperatura fuera de rango"));
    }
}
