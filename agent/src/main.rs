//! blocksmith-agent: monitorización de la salud de los discos en segundo plano.
//!
//! Lee S.M.A.R.T./NVMe periódicamente, guarda el histórico en la misma base de datos
//! que la app de escritorio y registra alertas según las reglas configuradas en ella.
//! Pensado para ejecutarse como servicio del sistema (las lecturas requieren permisos).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use blocksmith_health::{collect, repo};
use blocksmith_smart::auto_backend;
use blocksmith_store::{default_db_path, Database};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "blocksmith-agent", version, about = "Monitorización de la salud de los discos de BlockSmith")]
struct Cli {
    /// Base de datos (por defecto, la de la app de escritorio del usuario actual).
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Una pasada de lectura y termina.
    Once {
        #[arg(long)]
        json: bool,
    },
    /// Lee periódicamente. El intervalo sale de las reglas de la app si no se indica.
    Run {
        /// Minutos entre lecturas.
        #[arg(long)]
        interval: Option<u64>,
    },
    /// Estado: última lectura, discos y alertas abiertas.
    Status,
    /// Imprime la definición de servicio para instalar el agente.
    PrintService {
        #[arg(long, value_enum, default_value_t = default_kind())]
        kind: ServiceKind,
        /// Ruta del ejecutable instalado.
        #[arg(long, default_value = "/usr/local/bin/blocksmith-agent")]
        bin: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ServiceKind {
    Systemd,
    Launchd,
    Windows,
}

fn default_kind() -> ServiceKind {
    if cfg!(target_os = "macos") {
        ServiceKind::Launchd
    } else if cfg!(windows) {
        ServiceKind::Windows
    } else {
        ServiceKind::Systemd
    }
}

fn db_path(cli: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
    match cli {
        Some(p) => Ok(p.clone()),
        None => default_db_path().context("no se pudo determinar la carpeta de datos; usa --db"),
    }
}

/// Si el agente corre como root sobre la base de datos de un usuario, devuelve los
/// archivos de SQLite (incluidos -wal y -shm) al propietario de la carpeta para que
/// la app pueda seguir escribiendo en ellos.
#[cfg(unix)]
fn fix_ownership(db: &Path) {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid no tiene precondiciones.
    if unsafe { libc::geteuid() } != 0 {
        return;
    }
    let Some(dir) = db.parent() else { return };
    let Ok(meta) = std::fs::metadata(dir) else { return };
    if meta.uid() == 0 {
        return;
    }
    for suffix in ["", "-wal", "-shm"] {
        let mut p = db.as_os_str().to_owned();
        p.push(suffix);
        let p = PathBuf::from(p);
        if p.exists() {
            let _ = std::os::unix::fs::chown(&p, Some(meta.uid()), Some(meta.gid()));
        }
    }
}

#[cfg(not(unix))]
fn fix_ownership(_: &Path) {}

fn pass(db: &Database, path: &Path) -> anyhow::Result<blocksmith_health::CollectReport> {
    let backend = auto_backend();
    let report = collect(backend.as_ref(), db, "agente", &|| false)?;
    fix_ownership(path);
    Ok(report)
}

fn summary(r: &blocksmith_health::CollectReport) -> String {
    let mut s = format!(
        "{} discos · {} leídos · {} sin S.M.A.R.T. · {} sin permiso · {} con error · {} alertas nuevas",
        r.disks, r.read, r.unsupported, r.permission_denied, r.failed, r.new_alerts.len()
    );
    if let Some(e) = &r.error {
        s.push_str(&format!(" · error: {e}"));
    }
    s
}

fn service_definition(kind: ServiceKind, bin: &str, db: &Path) -> String {
    let db = db.display();
    match kind {
        ServiceKind::Systemd => format!(
            "# /etc/systemd/system/blocksmith-agent.service
[Unit]
Description=BlockSmith · monitorización de la salud de los discos
After=local-fs.target

[Service]
Type=simple
ExecStart={bin} --db \"{db}\" run
Restart=on-failure
RestartSec=60
Nice=10
IOSchedulingClass=idle
NoNewPrivileges=yes
PrivateTmp=yes

[Install]
WantedBy=multi-user.target

# Activar con:  sudo systemctl daemon-reload && sudo systemctl enable --now blocksmith-agent
"
        ),
        ServiceKind::Launchd => format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- /Library/LaunchDaemons/dev.blocksmith.agent.plist -->
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.blocksmith.agent</string>
  <key>ProgramArguments</key>
  <array><string>{bin}</string><string>--db</string><string>{db}</string><string>run</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>LowPriorityIO</key><true/>
  <key>Nice</key><integer>10</integer>
</dict>
</plist>
<!-- Activar con:  sudo launchctl bootstrap system /Library/LaunchDaemons/dev.blocksmith.agent.plist -->
"#
        ),
        ServiceKind::Windows => format!(
            "REM Requiere smartmontools instalado. Ejecutar en una consola de administrador:
sc.exe create BlockSmithAgent binPath= \"\\\"{bin}\\\" --db \\\"{db}\\\" run\" start= auto DisplayName= \"BlockSmith · salud de discos\"
sc.exe start BlockSmithAgent
"
        ),
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let path = db_path(&cli.db)?;
    if let Cmd::PrintService { kind, bin } = &cli.cmd {
        print!("{}", service_definition(*kind, bin, &path));
        return Ok(());
    }
    let db = Database::open(&path).with_context(|| format!("no se pudo abrir la base de datos {}", path.display()))?;
    fix_ownership(&path);
    match cli.cmd {
        Cmd::Once { json } => {
            let r = pass(&db, &path)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                println!("{}", summary(&r));
                for a in &r.new_alerts {
                    println!("  [{}] {} · {}: {}", a.severity, a.disk, a.title, a.message);
                }
            }
        }
        Cmd::Run { interval } => {
            if interval == Some(0) {
                bail!("el intervalo debe ser de al menos 1 minuto");
            }
            eprintln!("blocksmith-agent {} · base de datos {}", env!("CARGO_PKG_VERSION"), path.display());
            loop {
                let started = Instant::now();
                match pass(&db, &path) {
                    Ok(r) => {
                        eprintln!("{}", summary(&r));
                        for a in &r.new_alerts {
                            eprintln!("  alerta [{}] {} · {}: {}", a.severity, a.disk, a.title, a.message);
                        }
                    }
                    Err(e) => eprintln!("error en la lectura: {e:#}"),
                }
                // Se relee en cada vuelta: la app puede haber cambiado las reglas.
                let minutes = match interval {
                    Some(m) => m,
                    None => repo::poll_interval_min(&db.conn()).unwrap_or(360) as u64,
                };
                let wait = Duration::from_secs(minutes * 60).saturating_sub(started.elapsed());
                std::thread::sleep(wait);
            }
        }
        Cmd::Status => {
            match blocksmith_health::collector::last_poll(&db)? {
                Some(p) => println!("Última lectura: {} (por {}, backend {}, {} de {} discos leídos{})", p.at, p.by, p.backend, p.read, p.disks,
                    p.error.map(|e| format!(", error: {e}")).unwrap_or_default()),
                None => println!("Todavía no se ha leído la salud de ningún disco."),
            }
            let conn = db.conn();
            for d in repo::list_disks(&conn)? {
                let state = d.latest.as_ref().map(|s| s.overall.clone()).unwrap_or_else(|| d.read_status.clone().unwrap_or_default());
                println!("  {} · {} · {} · {} alertas abiertas", d.path.unwrap_or_default(), d.model.unwrap_or_default(), state, d.open_alerts);
            }
        }
        Cmd::PrintService { .. } => unreachable!(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_files_reference_binary_and_db() {
        let db = Path::new("/home/ana/.local/share/dev.blocksmith.desktop/blocksmith.sqlite3");
        let s = service_definition(ServiceKind::Systemd, "/usr/local/bin/blocksmith-agent", db);
        assert!(s.contains("ExecStart=/usr/local/bin/blocksmith-agent --db \"/home/ana/.local/share/dev.blocksmith.desktop/blocksmith.sqlite3\" run"));
        let p = service_definition(ServiceKind::Launchd, "/usr/local/bin/blocksmith-agent", db);
        assert!(p.contains("<string>dev.blocksmith.agent</string>") && p.contains("<string>run</string>"));
        assert!(service_definition(ServiceKind::Windows, "C:\\BlockSmith\\agent.exe", db).contains("sc.exe create"));
    }
}
