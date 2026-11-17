//! Destinos en disco (imágenes y dispositivos de bloque) y comprobaciones de seguridad
//! previas a escribir: montajes, disco de arranque, imágenes conectadas a un loop,
//! acceso exclusivo y ubicación del registro de deshacer.
//!
//! Linux se comprueba con /proc/self/mountinfo y sysfs (funciones con rutas
//! inyectables para las pruebas). En macOS se usa la salida de `mount`. En Windows
//! la escritura en dispositivos aún no está disponible.

use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::target::{pread, pwrite, Target};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Image,
    BlockDevice,
}

/// Imagen o dispositivo abierto para escribir.
pub struct FileTarget {
    file: File,
    path: PathBuf,
    len: u64,
    pub kind: TargetKind,
}

impl FileTarget {
    /// Abre para escritura. Imágenes: bloqueo exclusivo para que otro proceso no
    /// escriba a la vez. Dispositivos: apertura exclusiva (en Linux falla si el
    /// dispositivo o alguna partición está montada o en uso).
    pub fn open(path: &Path) -> io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        let kind = kind_of(&meta).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "solo se puede escribir en imágenes o dispositivos de bloque"))?;
        let mut opts = OpenOptions::new();
        opts.read(true).write(true);
        #[cfg(target_os = "linux")]
        if kind == TargetKind::BlockDevice {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(libc::O_EXCL);
        }
        #[cfg(windows)]
        if kind == TargetKind::BlockDevice {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "la escritura directa en dispositivos aún no está disponible en Windows"));
        }
        let mut file = opts.open(path).map_err(|e| match e.raw_os_error() {
            #[cfg(unix)]
            Some(libc::EBUSY) => io::Error::new(io::ErrorKind::ResourceBusy, "el dispositivo está en uso o montado; desmóntalo antes de reparar"),
            _ => e,
        })?;
        #[cfg(unix)]
        if kind == TargetKind::Image {
            use std::os::unix::io::AsRawFd;
            // SAFETY: descriptor válido mientras vive `file`.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(io::Error::new(io::ErrorKind::ResourceBusy, "otra aplicación está usando la imagen"));
            }
        }
        let len = file.seek(SeekFrom::End(0))?;
        Ok(Self { file, path: path.to_path_buf(), len, kind })
    }
}

impl Target for FileTarget {
    fn len(&self) -> u64 {
        self.len
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        pread(&self.file, offset, buf)
    }
    fn write_at(&mut self, offset: u64, data: &[u8]) -> io::Result<()> {
        pwrite(&self.file, offset, data)
    }
    fn sync(&mut self) -> io::Result<()> {
        self.file.sync_all()
    }
    fn describe(&self) -> String {
        self.path.display().to_string()
    }
}

fn kind_of(meta: &std::fs::Metadata) -> Option<TargetKind> {
    if meta.is_file() {
        return Some(TargetKind::Image);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if meta.file_type().is_block_device() || meta.file_type().is_char_device() {
            return Some(TargetKind::BlockDevice);
        }
    }
    None
}

/// Resultado de evaluar un destino antes de escribir.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    pub kind: Option<TargetKind>,
    pub exists: bool,
    pub writable: bool,
    /// Puntos de montaje del destino (o de sus particiones, o del loop que lo usa).
    pub mounted_at: Vec<String>,
    /// El destino contiene el sistema de archivos raíz.
    pub is_boot: bool,
    /// Imagen conectada a un dispositivo loop.
    pub loop_device: Option<String>,
}

/// Número major:minor de un dispositivo (st_rdev) o del sistema de archivos que
/// contiene un archivo (st_dev).
#[cfg(unix)]
fn dev_numbers(meta: &std::fs::Metadata, rdev: bool) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let d = if rdev { meta.rdev() } else { meta.dev() };
    (libc::major(d) as u64, libc::minor(d) as u64)
}

/// Montajes de /proc/self/mountinfo: (major:minor, punto de montaje).
pub fn parse_mountinfo(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            Some((f.get(2)?.to_string(), f.get(4)?.replace("\\040", " ")))
        })
        .collect()
}

/// Números major:minor de un disco y sus particiones según sysfs.
pub fn device_family(sysfs: &Path, majmin: &str) -> Vec<String> {
    let mut out = vec![majmin.to_string()];
    let Ok(dir) = std::fs::canonicalize(sysfs.join("dev/block").join(majmin)) else { return out };
    // Si es una partición, la familia es la del disco completo.
    let disk = if dir.join("partition").exists() { dir.parent().map(Path::to_path_buf).unwrap_or(dir) } else { dir };
    if let Ok(d) = std::fs::read_to_string(disk.join("dev")) {
        out.push(d.trim().to_string());
    }
    if let Ok(entries) = std::fs::read_dir(&disk) {
        for e in entries.flatten() {
            if e.path().join("partition").exists() {
                if let Ok(d) = std::fs::read_to_string(e.path().join("dev")) {
                    out.push(d.trim().to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Dispositivo loop que tiene `image` como archivo de respaldo.
pub fn loop_for_image(sysfs: &Path, image: &Path) -> Option<(String, String)> {
    let canon = std::fs::canonicalize(image).ok()?;
    for e in std::fs::read_dir(sysfs.join("block")).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("loop") {
            continue;
        }
        let backing = std::fs::read_to_string(e.path().join("loop/backing_file")).unwrap_or_default();
        if Path::new(backing.trim()) == canon {
            let dev = std::fs::read_to_string(e.path().join("dev")).ok()?.trim().to_string();
            return Some((format!("/dev/{name}"), dev));
        }
    }
    None
}

/// Evalúa un destino. En Linux usa /proc y /sys; en otros sistemas, lo que puede.
pub fn assess(path: &Path) -> Assessment {
    assess_with(path, Path::new("/sys"), std::fs::read_to_string("/proc/self/mountinfo").ok().as_deref())
}

pub fn assess_with(path: &Path, sysfs: &Path, mountinfo: Option<&str>) -> Assessment {
    let meta = std::fs::metadata(path);
    let kind = meta.as_ref().ok().and_then(kind_of);
    let mut a = Assessment {
        kind,
        exists: meta.is_ok(),
        writable: OpenOptions::new().write(true).open(path).is_ok(),
        mounted_at: vec![],
        is_boot: false,
        loop_device: None,
    };
    #[cfg(unix)]
    if let (Ok(meta), Some(info)) = (&meta, mountinfo) {
        let mounts = parse_mountinfo(info);
        let root = mounts.iter().find(|(_, mp)| mp == "/").map(|(d, _)| d.clone());
        let family = match kind {
            Some(TargetKind::BlockDevice) => {
                let (ma, mi) = dev_numbers(meta, true);
                device_family(sysfs, &format!("{ma}:{mi}"))
            }
            Some(TargetKind::Image) => match loop_for_image(sysfs, path) {
                Some((dev, majmin)) => {
                    a.loop_device = Some(dev);
                    device_family(sysfs, &majmin)
                }
                None => vec![],
            },
            None => vec![],
        };
        a.mounted_at = mounts.iter().filter(|(d, _)| family.contains(d)).map(|(_, mp)| mp.clone()).collect();
        a.is_boot = root.is_some_and(|r| family.contains(&r));
    }
    #[cfg(not(target_os = "linux"))]
    if kind == Some(TargetKind::BlockDevice) {
        // macOS y otros: el dispositivo (o /dev/diskNsM) aparece en la salida de `mount`.
        if let Ok(out) = std::process::Command::new("mount").output() {
            let dev = path.display().to_string().replace("/dev/rdisk", "/dev/disk");
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if line.starts_with(&dev) {
                    if let Some(mp) = line.split(" on ").nth(1).and_then(|r| r.split(" (").next()) {
                        a.mounted_at.push(mp.to_string());
                        a.is_boot |= mp == "/";
                    }
                }
            }
        }
    }
    let _ = sysfs;
    a
}

/// ¿Está `dir` (donde irá el registro de deshacer) en el mismo dispositivo físico que
/// el destino? Para dispositivos de bloque se compara con el disco y sus particiones.
pub fn undo_on_target_device(dir: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    {
        let (Ok(dmeta), Ok(tmeta)) = (std::fs::metadata(dir), std::fs::metadata(target)) else { return false };
        let (ma, mi) = dev_numbers(&dmeta, false);
        let dir_dev = format!("{ma}:{mi}");
        match kind_of(&tmeta) {
            Some(TargetKind::BlockDevice) => {
                let (ta, ti) = dev_numbers(&tmeta, true);
                device_family(Path::new("/sys"), &format!("{ta}:{ti}")).contains(&dir_dev)
            }
            // Para imágenes, el mismo sistema de archivos se admite (se avisa en la app).
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (dir, target);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
22 1 254:0 / / rw,relatime shared:1 - ext4 /dev/vda rw
30 22 8:17 / /media/ana/Archivo\\040Fotos rw - hfsplus /dev/sdb1 rw
31 22 7:0 / /mnt/imagen rw - hfsplus /dev/loop0 rw
";

    fn fake_sysfs() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let mk = |rel: &str, content: &str| {
            let p = root.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        };
        mk("devices/sdb/dev", "8:16\n");
        mk("devices/sdb/sdb1/dev", "8:17\n");
        mk("devices/sdb/sdb1/partition", "1\n");
        mk("devices/sdb/sdb2/dev", "8:18\n");
        mk("devices/sdb/sdb2/partition", "2\n");
        std::fs::create_dir_all(root.path().join("dev/block")).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.path().join("devices/sdb"), root.path().join("dev/block/8:16")).unwrap();
            std::os::unix::fs::symlink(root.path().join("devices/sdb/sdb2"), root.path().join("dev/block/8:18")).unwrap();
        }
        root
    }

    #[test]
    fn mountinfo_and_device_families() {
        let m = parse_mountinfo(MOUNTINFO);
        assert_eq!(m[1], ("8:17".into(), "/media/ana/Archivo Fotos".into()));
        let sys = fake_sysfs();
        // Desde el disco o desde cualquier partición se obtiene toda la familia.
        assert_eq!(device_family(sys.path(), "8:16"), vec!["8:16", "8:17", "8:18"]);
        assert_eq!(device_family(sys.path(), "8:18"), vec!["8:16", "8:17", "8:18"]);
        assert_eq!(device_family(sys.path(), "9:9"), vec!["9:9"]);
    }

    #[test]
    fn image_attached_to_loop_is_detected() {
        let sys = fake_sysfs();
        let img_dir = tempfile::tempdir().unwrap();
        let img = img_dir.path().join("disco.img");
        std::fs::write(&img, vec![0u8; 4096]).unwrap();
        let canon = std::fs::canonicalize(&img).unwrap();
        std::fs::create_dir_all(sys.path().join("block/loop0/loop")).unwrap();
        std::fs::write(sys.path().join("block/loop0/loop/backing_file"), format!("{}\n", canon.display())).unwrap();
        std::fs::write(sys.path().join("block/loop0/dev"), "7:0\n").unwrap();
        let a = assess_with(&img, sys.path(), Some(MOUNTINFO));
        assert_eq!(a.kind, Some(TargetKind::Image));
        assert_eq!(a.loop_device.as_deref(), Some("/dev/loop0"));
        assert_eq!(a.mounted_at, vec!["/mnt/imagen"]);
        assert!(!a.is_boot && a.writable);

        let other = img_dir.path().join("otra.img");
        std::fs::write(&other, vec![0u8; 4096]).unwrap();
        let b = assess_with(&other, sys.path(), Some(MOUNTINFO));
        assert!(b.mounted_at.is_empty() && b.loop_device.is_none());
    }

    #[test]
    fn file_target_locks_images() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("v.img");
        std::fs::write(&img, vec![7u8; 8192]).unwrap();
        let mut t = FileTarget::open(&img).unwrap();
        assert_eq!((t.len(), t.kind), (8192, TargetKind::Image));
        #[cfg(unix)]
        assert!(FileTarget::open(&img).is_err(), "una segunda apertura debe fallar mientras dura la primera");
        t.write_at(10, &[1, 2, 3]).unwrap();
        t.sync().unwrap();
        drop(t);
        assert_eq!(&std::fs::read(&img).unwrap()[10..13], &[1, 2, 3]);
        assert!(FileTarget::open(dir.path()).is_err(), "un directorio no es un destino");
    }
}
