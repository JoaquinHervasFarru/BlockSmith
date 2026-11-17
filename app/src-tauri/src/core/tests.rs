//! Flujo completo del backend con volúmenes generados por las fixtures.

use std::path::PathBuf;

use blocksmith_fs_core::Progress;
use blocksmith_fs_hfsplus::fixtures::{apply_damage, build_volume, sample_tree, BuildOptions, Damage};

use super::*;
use crate::db::Database;

struct Env {
    core: Core,
    dir: tempfile::TempDir,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::new(Database::in_memory().unwrap(), dir.path().join("data"), dir.path().join("salida"));
    Env { core, dir }
}

fn image(env: &Env, name: &str, damage: Option<Damage>) -> PathBuf {
    let mut img = build_volume(&BuildOptions { max_records_per_leaf: Some(6), ..Default::default() }, &sample_tree()).unwrap();
    if let Some(d) = damage {
        apply_damage(&mut img, d).unwrap();
    }
    let p = env.dir.path().join(name);
    std::fs::write(&p, img).unwrap();
    p
}

struct CancelAfter(u32);
impl Progress for CancelAfter {
    fn phase(&mut self, _: &str) {
        self.0 = self.0.saturating_sub(1);
    }
    fn advance(&mut self, _: u64, _: u64) {}
    fn is_cancelled(&self) -> bool {
        self.0 == 0
    }
}

#[test]
fn open_scan_plan_export_report() {
    let e = env();
    let path = image(&e, "dañada.img", Some(Damage::FolderAndThreadLost));
    let src = e.core.open_image(&path).unwrap();
    assert_eq!(src.kind, "raw_image");
    assert_eq!(src.volumes.len(), 1);
    let vol = &src.volumes[0];
    assert_eq!(vol.fs_type, "hfsplus");
    assert_eq!(vol.name.as_deref(), Some("BlockSmith Test"));
    assert_eq!(vol.capabilities.rebuild, "yes");

    // Abrir dos veces devuelve el mismo origen.
    assert_eq!(e.core.open_image(&path).unwrap().id, src.id);
    assert_eq!(e.core.list_sources().unwrap().len(), 1);

    let sid = e.core.scan(vol.id, &mut NullProgress).unwrap();
    let s = e.core.get_session(sid).unwrap();
    assert_eq!(s.status, "scanned");
    assert!(s.counts.critical > 0);
    assert_eq!(e.core.findings(sid).unwrap().len() as i64, s.counts.critical + s.counts.minor + s.counts.cosmetic);
    assert!(e.core.get_volume(vol.id).unwrap().last_session.is_some());

    // Árbol original: la raíz y sus hijos, carpetas primero.
    let root = e.core.tree_children("original", sid, None).unwrap();
    assert_eq!(root[0].name, "BlockSmith Test");
    let kids = e.core.tree_children("original", sid, Some(root[0].id)).unwrap();
    assert!(kids[0].is_folder && !kids.last().unwrap().is_folder);

    let rid = e.core.plan(sid, &mut NullProgress).unwrap();
    let r = e.core.get_rebuild(rid).unwrap();
    assert_eq!(r.status, "planned");
    assert!(r.nodes_after < r.nodes_before);
    assert!(r.changes.iter().any(|c| c.change == "moved_to_recovered" && c.count == 24));
    assert_eq!(e.core.get_session(sid).unwrap().latest_rebuild_id, Some(rid));

    // Árbol reconstruido: «Recovered Items» en la raíz, marcado como recuperado.
    let rroot = e.core.tree_children("rebuilt", rid, None).unwrap();
    let rkids = e.core.tree_children("rebuilt", rid, Some(rroot[0].id)).unwrap();
    let rec = rkids.iter().find(|n| n.name == "Recovered Items").expect("Recovered Items");
    assert_eq!(rec.child_count, 24);
    let moved = e.core.tree_children("rebuilt", rid, Some(rec.id)).unwrap();
    assert!(moved.iter().all(|n| n.change.as_deref() == Some("moved_to_recovered")));

    // La carpeta de destino no existe todavía: se crea al exportar.
    let out = e.dir.path().join("nueva-carpeta").join("reparada.img");
    let res = e.core.export(rid, &out, &mut NullProgress).unwrap();
    assert!(res.verified, "{:?}", res.remaining_findings);
    let r = e.core.get_rebuild(rid).unwrap();
    assert_eq!(r.status, "done");
    assert!(r.verified);
    assert_eq!(e.core.get_session(sid).unwrap().status, "exported");
    assert!(e.core.findings(sid).unwrap().iter().all(|f| f.fixed_in_rebuild));
    // Nunca sobrescribe.
    assert!(e.core.export(rid, &out, &mut NullProgress).is_err());

    // La imagen reparada se puede abrir y analizar como cualquier otra.
    let src2 = e.core.open_image(&out).unwrap();
    let sid2 = e.core.scan(src2.volumes[0].id, &mut NullProgress).unwrap();
    let s2 = e.core.get_session(sid2).unwrap();
    assert_eq!((s2.counts.critical, s2.counts.minor), (0, 0));

    for fmt in ["html", "json"] {
        let rep = e.core.write_report(sid, fmt, None).unwrap();
        let text = std::fs::read_to_string(&rep.path).unwrap();
        assert!(text.contains("BlockSmith"));
        assert!(text.contains("Recovered Items") || fmt == "json");
    }
    assert_eq!(e.core.reports(sid).unwrap().len(), 2);
    assert_eq!(e.core.list_sessions().unwrap().len(), 2);
}

#[test]
fn export_after_restart_recomputes_plan() {
    let e = env();
    let path = image(&e, "v.img", Some(Damage::ValenceWrong));
    let vol = e.core.open_image(&path).unwrap().volumes[0].id;
    let sid = e.core.scan(vol, &mut NullProgress).unwrap();
    let rid = e.core.plan(sid, &mut NullProgress).unwrap();
    e.core.plans.lock().unwrap().clear(); // como tras reiniciar la app
    let res = e.core.export(rid, &e.dir.path().join("out.img"), &mut NullProgress).unwrap();
    assert!(res.verified);
}

#[test]
fn cancelled_scan_is_recorded() {
    let e = env();
    let path = image(&e, "c.img", None);
    let vol = e.core.open_image(&path).unwrap().volumes[0].id;
    let err = e.core.scan(vol, &mut CancelAfter(2)).unwrap_err();
    assert!(err.to_string().contains("cancelada"), "{err}");
    let s = e.core.list_sessions().unwrap();
    assert_eq!(s[0].status, "cancelled");
}

#[test]
fn missing_image_and_unsupported_volumes() {
    let e = env();
    let path = image(&e, "m.img", None);
    let vol = e.core.open_image(&path).unwrap().volumes[0].id;
    std::fs::remove_file(&path).unwrap();
    assert!(!e.core.list_sources().unwrap()[0].exists);
    let err = e.core.scan(vol, &mut NullProgress).unwrap_err();
    assert!(err.to_string().contains("ya no está"), "{err}");
    assert_eq!(e.core.list_sessions().unwrap()[0].status, "failed");

    // Un archivo que no es una imagen reconocible se registra sin volúmenes.
    let junk = e.dir.path().join("basura.bin");
    std::fs::write(&junk, vec![0u8; 1 << 20]).unwrap();
    assert!(e.core.open_image(&junk).unwrap().volumes.is_empty());
    e.core.remove_source(e.core.list_sources().unwrap()[0].id).unwrap();
}

#[test]
fn settings_roundtrip_and_validation() {
    let e = env();
    let mut s = e.core.settings().unwrap();
    assert_eq!(s.theme, "system");
    s.theme = "dark".into();
    assert_eq!(e.core.set_settings(&s).unwrap().theme, "dark");
    s.theme = "verde".into();
    assert!(e.core.set_settings(&s).is_err());
}

#[test]
fn health_flow_with_replay_backend() {
    use blocksmith_health::repo;
    let e = env();
    let replay = e.dir.path().join("replay.json");
    std::fs::write(&replay, r#"{ "devices": [
      { "info": { "path": "/dev/nvme0n1", "model": "Samsung SSD 980 PRO 1TB", "serial": "S5GX", "firmware": "5B2QGXA7", "bus": "nvme",
                  "capacityBytes": 1000204886016, "removable": false, "rotational": false, "isBootDisk": true },
        "sequence": [ { "kind": "read", "detail": { "protocol": "nvme", "overall": "unknown", "temperatureC": 64, "powerOnHours": 6120,
            "percentageUsed": 4, "attributes": [], "source": "", "reasons": [],
            "nvme": { "criticalWarning": 0, "temperatureK": 337, "availableSpare": 100, "spareThreshold": 10, "percentageUsed": 4,
                      "dataUnitsRead": 1, "dataUnitsWritten": 2, "powerCycles": 3, "powerOnHours": 6120, "unsafeShutdowns": 0,
                      "mediaErrors": 0, "errorLogEntries": 0 } } } ] },
      { "info": { "path": "/dev/sdc", "model": "Caja USB", "serial": null, "firmware": null, "bus": "usb",
                  "capacityBytes": 1000000000000, "removable": false, "rotational": null, "isBootDisk": false },
        "sequence": [ { "kind": "unsupported", "detail": "el puente USB no transmite S.M.A.R.T." } ] }
    ] }"#).unwrap();
    std::env::set_var(blocksmith_smart::backends::REPLAY_ENV, &replay);
    assert!(e.core.health_due().unwrap());
    let r = e.core.health_collect(&mut NullProgress).unwrap();
    std::env::remove_var(blocksmith_smart::backends::REPLAY_ENV);
    assert_eq!((r.disks, r.read, r.unsupported), (2, 1, 1));
    assert_eq!(r.new_alerts.len(), 1, "temperatura alta");
    assert!(!e.core.health_due().unwrap(), "recién leído");

    let disks = e.core.health_disks().unwrap();
    let nvme = disks.iter().find(|d| d.bus == "nvme").unwrap();
    assert_eq!(nvme.latest.as_ref().unwrap().nvme.as_ref().unwrap().percentage_used, 4);
    let detail = e.core.health_disk(nvme.id).unwrap();
    assert_eq!((detail.history.len(), detail.alerts.len()), (1, 1));
    let st = e.core.health_status().unwrap();
    assert_eq!((st.open_alerts, st.last_poll.unwrap().by.as_str()), (1, "app"));

    e.core.acknowledge_alert(detail.alerts[0].id).unwrap();
    assert!(e.core.acknowledge_alert(detail.alerts[0].id).is_err());
    assert!(e.core.alerts(true).unwrap().is_empty());

    let mut rules = e.core.alert_rules().unwrap();
    let t = rules.iter_mut().find(|r| r.rule == "TEMP_HIGH").unwrap();
    t.threshold = serde_json::json!({ "celsius": 70 });
    assert_eq!(e.core.set_alert_rule(t).unwrap().iter().find(|r| r.rule == "TEMP_HIGH").unwrap().threshold["celsius"], 70);
    t.threshold = serde_json::json!({ "celsius": -5 });
    assert!(e.core.set_alert_rule(t).is_err());
    let _ = repo::POLL_KEY;

    // Ajustes antiguos sin el campo nuevo se siguen leyendo (con el valor por defecto).
    e.core.db().set_setting("app", r#"{"outputDir":"/x","theme":"dark","logLevel":"info","exportEncryptedByDefault":true}"#).unwrap();
    let s = e.core.settings().unwrap();
    assert_eq!((s.theme.as_str(), s.health_check_on_start), ("dark", true));
}
