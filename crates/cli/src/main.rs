//! blocksmith-cli: herramienta de desarrollo y diagnóstico del motor.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context};
use blocksmith_fs_core::{Progress, Severity};
use blocksmith_fs_hfsplus::rebuild::plan_rebuild;
use blocksmith_fs_hfsplus::{scan_volume, Volume};
use blocksmith_io::{open_image, BlockDevice, Slice};
use blocksmith_partition::{detect, Hint, Scheme};
use blocksmith_rebuilder::summary::change_label;
use blocksmith_rebuilder::{export_image, summarize};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "blocksmith-cli", version, about = "Motor de BlockSmith desde la terminal (solo lectura)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Identifica formato de imagen, particiones y sistemas de archivos.
    Probe { path: PathBuf },
    /// Escanea un volumen HFS+/HFSX y lista los hallazgos.
    Scan {
        path: PathBuf,
        /// Índice de partición (por defecto, el primer volumen HFS+ encontrado).
        #[arg(long)]
        partition: Option<u32>,
        /// Salida en JSON.
        #[arg(long)]
        json: bool,
        /// Muestra también el árbol de carpetas reconstruido a partir del catálogo.
        #[arg(long)]
        tree: bool,
    },
    /// Reconstruye el directorio y lo exporta a una imagen NUEVA (el origen no se modifica).
    Rebuild {
        path: PathBuf,
        /// Archivo de salida; no debe existir.
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        partition: Option<u32>,
        /// Solo muestra los cambios previstos, sin escribir nada.
        #[arg(long)]
        dry_run: bool,
        /// Muestra todos los cambios (por defecto, los 40 primeros).
        #[arg(long)]
        all: bool,
    },
}

/// Busca el primer volumen HFS+/HFSX (o el de la partición pedida).
fn choose_volume(dev: &dyn BlockDevice, partition: Option<u32>) -> anyhow::Result<(u64, u64, String)> {
    let (_, vols, _) = candidate_volumes(dev)?;
    for (idx, start, len, label) in vols {
        if partition.is_some_and(|p| p != idx) {
            continue;
        }
        let slice = Slice::new(dev, start, len)?;
        if Volume::open(&slice)?.is_some() {
            return Ok((start, len, label));
        }
    }
    bail!("no se encontró ningún volumen HFS+/HFSX")
}

struct CliProgress {
    quiet: bool,
}

impl Progress for CliProgress {
    fn phase(&mut self, name: &str) {
        if !self.quiet {
            eprintln!("  · {name}");
        }
    }
    fn advance(&mut self, _done: u64, _total: u64) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// (índice de partición, offset, longitud, etiqueta)
type Candidate = (u32, u64, u64, String);

/// Rangos donde buscar volúmenes.
fn candidate_volumes(dev: &dyn BlockDevice) -> anyhow::Result<(Scheme, Vec<Candidate>, Vec<String>)> {
    let table = detect(dev)?;
    let mut out = Vec::new();
    if table.scheme == Scheme::None {
        out.push((0, 0, dev.len(), "dispositivo completo".to_string()));
    }
    for p in &table.partitions {
        if matches!(p.hint, Hint::Free | Hint::Reserved) {
            continue;
        }
        out.push((p.index, p.start, p.len, format!("{} «{}» ({})", p.index, p.name, p.type_id)));
    }
    Ok((table.scheme, out, table.warnings))
}

fn human(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < units.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    format!("{v:.1} {}", units[i])
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    match Cli::parse().cmd {
        Cmd::Probe { path } => {
            let dev = open_image(&path).with_context(|| format!("no se pudo abrir {}", path.display()))?;
            let udif = blocksmith_io::udif::has_koly_trailer(&blocksmith_io::RawImage::open(&path)?)?;
            println!("{}", path.display());
            println!("  formato:     {}", if udif { "UDIF (.dmg)" } else { "raw" });
            println!("  tamaño:      {} ({} bytes)", human(dev.len()), dev.len());
            let (scheme, vols, warnings) = candidate_volumes(dev.as_ref())?;
            println!("  particiones: {scheme:?}");
            for w in warnings {
                println!("  aviso:       {w}");
            }
            for (_, start, len, label) in vols {
                let slice = Slice::new(dev.as_ref(), start, len)?;
                let fs = match Volume::open(&slice)? {
                    Some(v) => format!(
                        "{}{} · bloques de {} B · {} bloques · {}",
                        if v.vh.is_hfsx() { "HFSX" } else { "HFS+" },
                        if v.wrapped { " (en wrapper HFS)" } else { "" },
                        v.vh.block_size,
                        v.vh.total_blocks,
                        if v.vh.is_journaled() { "con journal" } else { "sin journal" }
                    ),
                    None => "sistema de archivos no reconocido en esta fase".into(),
                };
                println!("    - {label}: {} → {fs}", human(len));
            }
        }
        Cmd::Scan { path, partition, json, tree } => {
            let dev = open_image(&path).with_context(|| format!("no se pudo abrir {}", path.display()))?;
            let (start, len, label) = choose_volume(dev.as_ref(), partition)?;
            let slice = Slice::new(dev.as_ref(), start, len)?;
            let vol = Volume::open(&slice)?.expect("comprobado arriba");
            if !json {
                eprintln!("Escaneando {label} (solo lectura)…");
            }
            let t0 = Instant::now();
            let scan = scan_volume(&vol, &mut CliProgress { quiet: json })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&scan.result)?);
                return Ok(());
            }
            let r = &scan.result;
            let s = &r.stats;
            println!();
            println!("Volumen «{}» · {:?}", r.info.name.as_deref().unwrap_or("?"), r.info.kind);
            println!("  {} archivos · {} carpetas · catálogo de {} nodos ({} dañados) · {:.1} % de espacio sin usar en hojas",
                s.files, s.folders, s.nodes_total, s.nodes_damaged, s.fragmentation_pct);
            let count = |sev| r.findings.iter().filter(|f| f.severity == sev).count();
            println!(
                "  Hallazgos: {} críticos · {} menores · {} cosméticos  ({:.2} s)",
                count(Severity::Critical),
                count(Severity::Minor),
                count(Severity::Cosmetic),
                t0.elapsed().as_secs_f32()
            );
            for f in &r.findings {
                let sev = match f.severity {
                    Severity::Critical => "CRÍTICO ",
                    Severity::Minor => "menor   ",
                    Severity::Cosmetic => "cosmético",
                };
                println!("  [{sev}] {:<24} {}", f.code, f.message);
                if let Some(p) = &f.path {
                    println!("             {:<24} {p}", "");
                }
            }
            if r.findings.is_empty() {
                println!("  ✓ No se encontraron problemas.");
            }
            if tree {
                println!();
                print_tree(&scan.catalog, 2, 0);
            }
        }
        Cmd::Rebuild { path, out, partition, dry_run, all } => {
            if out.is_none() && !dry_run {
                bail!("indica --out <archivo nuevo> o usa --dry-run");
            }
            let dev = open_image(&path).with_context(|| format!("no se pudo abrir {}", path.display()))?;
            let (start, len, label) = choose_volume(dev.as_ref(), partition)?;
            let slice = Slice::new(dev.as_ref(), start, len)?;
            let vol = Volume::open(&slice)?.expect("comprobado");
            eprintln!("Reconstruyendo {label} (el origen no se modifica)…");
            let mut progress = CliProgress { quiet: false };
            let scan = scan_volume(&vol, &mut progress)?;
            let before = scan.result.findings.iter().filter(|f| f.severity != Severity::Cosmetic).count();
            progress.phase("Plan de reconstrucción");
            let plan = plan_rebuild(&vol, &scan)?;
            let sum = summarize(&plan);
            println!();
            println!(
                "Directorio: {} nodos → {} nodos{}",
                sum.nodes_before,
                sum.nodes_after,
                if sum.nodes_after < sum.nodes_before {
                    format!(" ({:.0} % más compacto)", 100.0 * (1.0 - sum.nodes_after as f64 / sum.nodes_before as f64))
                } else {
                    String::new()
                }
            );
            println!(
                "Problemas antes: {before} · recuperados: {} · perdidos: {} · escritura: {}",
                sum.recovered,
                sum.lost,
                human(sum.bytes_to_write)
            );
            for (k, v) in &sum.by_change {
                println!("  {k}: {v}");
            }
            let limit = if all { usize::MAX } else { 40 };
            for d in plan.diff.iter().take(limit) {
                let path = d.new_path.as_deref().or(d.old_path.as_deref()).unwrap_or("—");
                println!("  · {:<36} {path}", change_label(d.change));
                if let Some(n) = &d.note {
                    println!("    {:<36} {n}", "");
                }
            }
            if plan.diff.len() > limit {
                println!("  … y {} cambios más (usa --all)", plan.diff.len() - limit);
            }
            if dry_run {
                println!("\nSimulación: no se ha escrito nada.");
                return Ok(());
            }
            let out = out.expect("comprobado arriba");
            let report = export_image(dev.as_ref(), start, len, &plan, &out, &mut progress)?;
            println!(
                "\nImagen reparada: {} ({} de datos copiados, {} escrituras, {})",
                out.display(),
                human(report.bytes_copied),
                report.writes_applied,
                human(report.bytes_written)
            );

            // Verificación independiente de la imagen nueva.
            let fixed = open_image(&out)?;
            let fslice = Slice::new(fixed.as_ref(), start, len)?;
            let fvol = Volume::open(&fslice)?.context("la imagen nueva no contiene un volumen HFS+")?;
            let verify = scan_volume(&fvol, &mut CliProgress { quiet: true })?;
            let remaining: Vec<_> =
                verify.result.findings.iter().filter(|f| f.severity != Severity::Cosmetic).collect();
            if remaining.is_empty() {
                println!(
                    "Verificación: ✓ la imagen reparada no tiene problemas ({} archivos, {} carpetas).",
                    verify.result.stats.files, verify.result.stats.folders
                );
            } else {
                println!("Verificación: ✗ quedan {} problemas:", remaining.len());
                for f in remaining {
                    println!("  [{:?}] {} {}", f.severity, f.code, f.message);
                }
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn print_tree(cat: &blocksmith_fs_hfsplus::CatalogSnapshot, id: u32, depth: usize) {
    use blocksmith_fs_hfsplus::unicode::from_hfs_name;
    if let Some((k, _)) = cat.folders.get(&id) {
        println!("{}{}/", "  ".repeat(depth), from_hfs_name(&k.name));
    }
    let mut entries: Vec<(String, Option<u32>, u64)> = Vec::new();
    for (cid, (k, _)) in &cat.folders {
        if k.parent_id == id && *cid != id {
            entries.push((from_hfs_name(&k.name), Some(*cid), 0));
        }
    }
    for (k, r) in cat.files.values() {
        if k.parent_id == id {
            entries.push((from_hfs_name(&k.name), None, r.data_fork.logical_size));
        }
    }
    entries.sort_by_key(|e| e.0.to_lowercase());
    for (name, folder, size) in entries {
        match folder {
            Some(cid) => print_tree(cat, cid, depth + 1),
            None => println!("{}{name}  ({})", "  ".repeat(depth + 1), human(size)),
        }
    }
}
