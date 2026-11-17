//! Muestra la evaluación de seguridad de un destino: `cargo run --example assess -- /dev/sdb`.
fn main() {
    for p in std::env::args().skip(1) {
        let path = std::path::Path::new(&p);
        println!("{p}: {}", serde_json::to_string(&blocksmith_txwriter::device::assess(path)).unwrap());
        if let Err(e) = blocksmith_txwriter::FileTarget::open(path) {
            println!("  apertura para escribir: {e}");
        }
    }
}
