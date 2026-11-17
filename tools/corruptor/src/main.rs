//! blocksmith-corruptor: genera fixtures HFS+ y les aplica daños deterministas.
//! Solo escribe archivos de imagen nuevos; nunca opera sobre dispositivos.

use std::path::PathBuf;

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use blocksmith_fs_hfsplus::fixtures::{apply_damage, build_volume, sample_tree, BuildOptions, Damage};

#[derive(Parser)]
#[command(name = "blocksmith-corruptor", version, about = "Fixtures HFS+ con daños controlados")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Genera el juego completo: volumen sano, variante HFSX y una imagen por cada daño.
    Gen {
        #[arg(long, default_value = "fixtures/hfsplus")]
        out: PathBuf,
    },
    /// Aplica un daño a una copia de una imagen generada por `gen`.
    Damage {
        input: PathBuf,
        output: PathBuf,
        /// Uno de: key-order, broken-link, node-unreadable, valence, file-count,
        /// next-cnid, extent-overlap, extent-out-of-range, bitmap-leak, bitmap-underalloc
        #[arg(long)]
        kind: String,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Gen { out } => {
            std::fs::create_dir_all(&out)?;
            let multi = BuildOptions { max_records_per_leaf: Some(6), ..Default::default() };
            let clean = build_volume(&multi, &sample_tree()).map_err(anyhow::Error::msg)?;
            std::fs::write(out.join("sano.img"), &clean)?;
            let hfsx = BuildOptions { case_sensitive: true, ..multi.clone() };
            std::fs::write(out.join("sano-hfsx.img"), build_volume(&hfsx, &sample_tree()).map_err(anyhow::Error::msg)?)?;
            for d in Damage::ALL {
                let mut img = clean.clone();
                apply_damage(&mut img, d).map_err(anyhow::Error::msg)?;
                std::fs::write(out.join(format!("dañado-{}.img", d.slug())), img)?;
            }
            println!("{} imágenes generadas en {}", Damage::ALL.len() + 2, out.display());
        }
        Cmd::Damage { input, output, kind } => {
            let Some(d) = Damage::ALL.into_iter().find(|d| d.slug() == kind) else {
                bail!("daño desconocido: {kind}");
            };
            if input == output {
                bail!("la salida debe ser un archivo distinto de la entrada");
            }
            let mut img = std::fs::read(&input).with_context(|| format!("no se pudo leer {}", input.display()))?;
            apply_damage(&mut img, d).map_err(anyhow::Error::msg)?;
            std::fs::write(&output, img)?;
            println!("{} → {} ({kind})", input.display(), output.display());
        }
    }
    Ok(())
}
