//! Backends de lectura de salud. `auto_backend()` elige el adecuado para la plataforma:
//!
//! | Plataforma | Enumeración | Lectura |
//! |---|---|---|
//! | Linux | sysfs | nativa (`SG_IO` ATA / ioctl NVMe); si no se admite, `smartctl` si está instalado |
//! | macOS | `smartctl --scan-open` | `smartctl` |
//! | Windows | `smartctl --scan-open` | `smartctl` (lectura nativa pendiente) |
//!
//! `BLOCKSMITH_HEALTH_REPLAY=<archivo.json>` sustituye todo por el backend de reproducción.

#[cfg(target_os = "linux")]
pub mod linux;
pub mod replay;
pub mod smartctl;

use crate::{DeviceInfo, ReadOutcome};

pub trait HealthBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn devices(&self) -> Result<Vec<DeviceInfo>, String>;
    fn read(&self, dev: &DeviceInfo) -> ReadOutcome;
}

pub const REPLAY_ENV: &str = "BLOCKSMITH_HEALTH_REPLAY";

/// Backend combinado: el principal y, si no admite un disco, `smartctl`.
pub struct Auto {
    primary: Box<dyn HealthBackend>,
    fallback: Option<smartctl::Smartctl>,
}

impl HealthBackend for Auto {
    fn name(&self) -> &'static str {
        self.primary.name()
    }
    fn devices(&self) -> Result<Vec<DeviceInfo>, String> {
        self.primary.devices()
    }
    fn read(&self, dev: &DeviceInfo) -> ReadOutcome {
        let first = self.primary.read(dev);
        match (&first, &self.fallback) {
            (ReadOutcome::Unsupported(_), Some(sc)) => match sc.read(dev) {
                ok @ ReadOutcome::Read(_) => ok,
                _ => first,
            },
            _ => first,
        }
    }
}

/// Backend recomendado para esta plataforma (o el de reproducción si está configurado).
pub fn auto_backend() -> Box<dyn HealthBackend> {
    if let Ok(path) = std::env::var(REPLAY_ENV) {
        if !path.is_empty() {
            return Box::new(replay::Replay::from_file(&path));
        }
    }
    let sc = smartctl::Smartctl::locate();
    #[cfg(target_os = "linux")]
    {
        Box::new(Auto { primary: Box::new(linux::LinuxNative::default()), fallback: sc })
    }
    #[cfg(not(target_os = "linux"))]
    {
        match sc {
            Some(sc) => Box::new(sc),
            None => Box::new(smartctl::Missing),
        }
    }
}
