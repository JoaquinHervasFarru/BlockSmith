//! dirforge-helper: proceso privilegiado con superficie mínima.
//! Solo acepta peticiones autenticadas de la app para bloquear/desmontar un
//! volumen y aplicar un `RebuildPlan` mediante `dirforge-txwriter` (F5).

fn main() {
    eprintln!("dirforge-helper {}: pendiente (fase F5)", env!("CARGO_PKG_VERSION"));
}
