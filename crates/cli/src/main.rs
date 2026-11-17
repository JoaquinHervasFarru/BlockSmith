//! dirforge-cli: herramienta de desarrollo y diagnóstico del motor.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use dirforge_io::{udif, BlockDevice, RawImage};

#[derive(Parser)]
#[command(name = "dirforge-cli", version, about = "Motor de DirForge desde la terminal")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Identifica el formato de una imagen o dispositivo (solo lectura).
    Probe { path: PathBuf },
    /// Escanea un volumen (F1).
    Scan { path: PathBuf },
    /// Calcula una reconstrucción y la exporta a una imagen nueva (F2).
    Rebuild { path: PathBuf, #[arg(long)] out: PathBuf },
    /// Lee la salud del disco (F4).
    Smart { device: String },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::from_default_env(),
    ).init();
    match Cli::parse().cmd {
        Cmd::Probe { path } => {
            let img = RawImage::open(&path)?;
            println!("{}: {} bytes", path.display(), img.len());
            println!("UDIF (.dmg): {}", if udif::has_koly_trailer(&img)? { "sí" } else { "no" });
        }
        Cmd::Scan { .. } | Cmd::Rebuild { .. } | Cmd::Smart { .. } => {
            anyhow::bail!("comando aún no implementado en esta fase");
        }
    }
    Ok(())
}
