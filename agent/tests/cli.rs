//! El agente de punta a punta con el backend de reproducción.

use std::process::Command;

const REPLAY: &str = r#"{ "devices": [
  { "info": { "path": "/dev/sdb", "model": "WDC WD40EFRX-68N32N0", "serial": "WX42D81F7KLP", "firmware": "82.00A82", "bus": "sata",
              "capacityBytes": 4000787030016, "removable": false, "rotational": true, "isBootDisk": false },
    "sequence": [ { "kind": "read", "detail": { "protocol": "ata", "overall": "unknown", "temperatureC": 41, "powerOnHours": 31870,
        "percentageUsed": null, "nvme": null, "source": "", "reasons": [],
        "attributes": [ { "id": 5, "name": "Reallocated_Sector_Ct", "flags": 51, "value": 7, "worst": 7, "threshold": 10, "raw": 3960 } ] } } ] }
] }"#;

#[test]
fn once_then_status() {
    let dir = tempfile::tempdir().unwrap();
    let replay = dir.path().join("replay.json");
    std::fs::write(&replay, REPLAY).unwrap();
    let db = dir.path().join("datos").join("blocksmith.sqlite3");
    let bin = env!("CARGO_BIN_EXE_blocksmith-agent");

    let out = Command::new(bin)
        .env("BLOCKSMITH_HEALTH_REPLAY", &replay)
        .arg("--db")
        .arg(&db)
        .args(["once", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["read"], 1);
    assert_eq!(r["newAlerts"][0]["title"], "El disco predice un fallo");

    let out = Command::new(bin).arg("--db").arg(&db).arg("status").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("por agente, backend reproducción, 1 de 1 discos"), "{text}");
    assert!(text.contains("/dev/sdb · WDC WD40EFRX-68N32N0 · failing · 1 alertas abiertas"), "{text}");
}
