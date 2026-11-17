//! Lista los discos y su estado de salud con el backend automático.
use blocksmith_smart::{auto_backend, ReadOutcome};

fn main() {
    let b = auto_backend();
    println!("backend: {}", b.name());
    match b.devices() {
        Err(e) => println!("error: {e}"),
        Ok(devs) => {
            for d in devs {
                let r = b.read(&d);
                let s = match &r {
                    ReadOutcome::Read(s) => format!("{:?} · {:?} °C · {:?}", s.overall, s.temperature_c, s.reasons),
                    other => format!("{other:?}"),
                };
                println!("{} {:?} {:?} {} B arranque={} → {s}", d.path, d.bus, d.model, d.capacity_bytes, d.is_boot_disk);
            }
        }
    }
}
