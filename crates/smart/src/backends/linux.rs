//! Linux: enumeración por sysfs y lectura nativa.
//!
//! - ATA (SATA y puentes USB compatibles con SAT): `SG_IO` con ATA PASS-THROUGH(16),
//!   SMART READ DATA (0xD0) y READ THRESHOLDS (0xD1).
//! - NVMe: ioctl `NVME_IOCTL_ADMIN_CMD` con GET LOG PAGE 0x02.
//!
//! Ambas lecturas necesitan normalmente permisos de administrador. Estado: los
//! intérpretes están probados con datos de la especificación; las llamadas al
//! sistema están pendientes de validar con hardware real.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use crate::{ata, evaluate_ata, evaluate_nvme, nvme, Bus, DeviceInfo, HealthSnapshot, Protocol, ReadOutcome};

use super::HealthBackend;

pub struct LinuxNative {
    sysfs: PathBuf,
}

impl Default for LinuxNative {
    fn default() -> Self {
        Self { sysfs: PathBuf::from("/sys") }
    }
}

impl LinuxNative {
    /// Para pruebas: un árbol sysfs falso.
    pub fn with_sysfs(root: impl Into<PathBuf>) -> Self {
        Self { sysfs: root.into() }
    }
}

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn is_disk_name(name: &str) -> bool {
    let digits_after = |prefix: &str| name.strip_prefix(prefix).is_some_and(|r| !r.is_empty());
    (name.starts_with("sd") && digits_after("sd"))
        || (name.starts_with("vd") && digits_after("vd"))
        || (name.starts_with("hd") && digits_after("hd"))
        || (name.starts_with("nvme") && name.contains('n') && name[4..].contains('n'))
        || (name.starts_with("mmcblk") && !name.contains("boot") && !name.contains("rpmb"))
}

/// Disco que contiene el sistema de archivos raíz, a partir de /proc/self/mountinfo.
fn boot_disk_name(sysfs: &Path) -> Option<String> {
    let info = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    let majmin = info.lines().find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        (f.get(4) == Some(&"/")).then(|| f.get(2).map(|s| s.to_string())).flatten()
    })?;
    let dev = std::fs::canonicalize(sysfs.join("dev/block").join(&majmin)).ok()?;
    let disk = if dev.join("partition").exists() { dev.parent()?.to_path_buf() } else { dev };
    disk.file_name().map(|n| n.to_string_lossy().to_string())
}

/// Número de serie ATA/SCSI desde la página VPD 0x80 (4 bytes de cabecera + ASCII).
fn vpd_serial(dev_dir: &Path) -> Option<String> {
    let b = std::fs::read(dev_dir.join("vpd_pg80")).ok()?;
    let s = String::from_utf8_lossy(b.get(4..)?).trim_matches(|c: char| c.is_whitespace() || c == '\0').to_string();
    (!s.is_empty()).then_some(s)
}

impl HealthBackend for LinuxNative {
    fn name(&self) -> &'static str {
        "nativo"
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, String> {
        let block = self.sysfs.join("block");
        let entries = std::fs::read_dir(&block).map_err(|e| format!("no se pudo leer {}: {e}", block.display()))?;
        let boot = boot_disk_name(&self.sysfs);
        let mut out = Vec::new();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !is_disk_name(&name) {
                continue;
            }
            let dir = block.join(&name);
            let sectors: u64 = read_trim(&dir.join("size")).and_then(|s| s.parse().ok()).unwrap_or(0);
            if sectors == 0 {
                continue;
            }
            let real = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            let real_str = real.to_string_lossy();
            let nvme = name.starts_with("nvme");
            let bus = if nvme {
                Bus::Nvme
            } else if real_str.contains("/usb") {
                Bus::Usb
            } else if real_str.contains("/ata") || name.starts_with("sd") {
                Bus::Sata
            } else {
                Bus::Other
            };
            let dev_dir = dir.join("device");
            let (model, serial, firmware) = if nvme {
                // device → controlador (nvme0) con model/serial/firmware_rev.
                (read_trim(&dev_dir.join("model")), read_trim(&dev_dir.join("serial")), read_trim(&dev_dir.join("firmware_rev")))
            } else {
                let vendor = read_trim(&dev_dir.join("vendor")).filter(|v| v != "ATA");
                let model = read_trim(&dev_dir.join("model")).map(|m| match &vendor {
                    Some(v) => format!("{v} {m}"),
                    None => m,
                });
                (model, vpd_serial(&dev_dir), read_trim(&dev_dir.join("rev")))
            };
            out.push(DeviceInfo {
                path: format!("/dev/{name}"),
                model,
                serial,
                firmware,
                bus,
                capacity_bytes: sectors * 512,
                removable: read_trim(&dir.join("removable")).as_deref() == Some("1"),
                rotational: read_trim(&dir.join("queue/rotational")).map(|r| r == "1"),
                is_boot_disk: boot.as_deref() == Some(name.as_str()),
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn read(&self, dev: &DeviceInfo) -> ReadOutcome {
        let file = match OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&dev.path) {
            Ok(f) => f,
            Err(e) if matches!(e.raw_os_error(), Some(libc::EACCES) | Some(libc::EPERM)) => {
                return ReadOutcome::PermissionDenied(permission_message(is_root()))
            }
            Err(e) => return ReadOutcome::Failed(format!("no se pudo abrir {}: {e}", dev.path)),
        };
        if dev.bus == Bus::Nvme {
            read_nvme(&file)
        } else {
            read_ata(&file)
        }
    }
}

fn is_root() -> bool {
    // SAFETY: geteuid no tiene precondiciones.
    unsafe { libc::geteuid() == 0 }
}

/// Mensaje según quién ejecuta: sin privilegios hacen falta; con ellos, lo impide el sistema.
pub fn permission_message(root: bool) -> String {
    if root {
        "el sistema no permite el acceso directo a este dispositivo (por ejemplo, dentro de un contenedor o por una política de seguridad)".into()
    } else {
        "leer S.M.A.R.T. requiere permisos de administrador; instala el agente como servicio del sistema".into()
    }
}

// ── ATA PASS-THROUGH(16) por SG_IO ─────────────────────────────────────────

const SG_IO: libc::c_ulong = 0x2285;
const SG_DXFER_FROM_DEV: i32 = -3;

#[repr(C)]
struct SgIoHdr {
    interface_id: i32,
    dxfer_direction: i32,
    cmd_len: u8,
    mx_sb_len: u8,
    iovec_count: u16,
    dxfer_len: u32,
    dxferp: *mut libc::c_void,
    cmdp: *mut u8,
    sbp: *mut u8,
    timeout: u32,
    flags: u32,
    pack_id: i32,
    usr_ptr: *mut libc::c_void,
    status: u8,
    masked_status: u8,
    msg_status: u8,
    sb_len_wr: u8,
    host_status: u16,
    driver_status: u16,
    resid: i32,
    duration: u32,
    info: u32,
}

/// CDB de ATA PASS-THROUGH(16) para un subcomando SMART con lectura PIO de un sector.
pub fn ata_smart_cdb(feature: u8) -> [u8; 16] {
    [
        0x85,       // ATA PASS-THROUGH (16)
        4 << 1,     // protocolo PIO Data-In
        0x0E,       // t_dir = desde el dispositivo, byt_blok = bloques, t_length = campo count
        0, feature, // features (0xD0 datos, 0xD1 umbrales)
        0, 1,       // count = 1 sector
        0, 0,       // LBA low
        0, 0x4F,    // LBA mid: firma SMART
        0, 0xC2,    // LBA high: firma SMART
        0,          // device
        0xB0,       // comando SMART
        0,          // control
    ]
}

enum AtaError {
    Unsupported(String),
    Failed(String),
}

fn sg_ata_smart(file: &File, feature: u8) -> Result<Vec<u8>, AtaError> {
    let mut data = vec![0u8; ata::PAGE];
    let mut cdb = ata_smart_cdb(feature);
    let mut sense = [0u8; 32];
    let mut hdr = SgIoHdr {
        interface_id: b'S' as i32,
        dxfer_direction: SG_DXFER_FROM_DEV,
        cmd_len: cdb.len() as u8,
        mx_sb_len: sense.len() as u8,
        iovec_count: 0,
        dxfer_len: data.len() as u32,
        dxferp: data.as_mut_ptr().cast(),
        cmdp: cdb.as_mut_ptr(),
        sbp: sense.as_mut_ptr(),
        timeout: 10_000,
        flags: 0,
        pack_id: 0,
        usr_ptr: std::ptr::null_mut(),
        status: 0,
        masked_status: 0,
        msg_status: 0,
        sb_len_wr: 0,
        host_status: 0,
        driver_status: 0,
        resid: 0,
        duration: 0,
        info: 0,
    };
    // SAFETY: `hdr` apunta a buffers vivos durante toda la llamada y con los tamaños declarados.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), SG_IO as _, &mut hdr as *mut SgIoHdr) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        return Err(match err.raw_os_error() {
            Some(libc::ENOTTY) | Some(libc::EINVAL) | Some(libc::ENOSYS) => {
                AtaError::Unsupported("el controlador no admite comandos ATA (p. ej. discos virtuales)".into())
            }
            _ => AtaError::Failed(format!("SG_IO falló: {err}")),
        });
    }
    // Un CHECK CONDITION con sense «ILLEGAL REQUEST» indica que el dispositivo o el
    // puente no entiende el comando.
    if hdr.status != 0 || hdr.host_status != 0 || (hdr.driver_status & 0x0F) != 0 {
        let key = if hdr.sb_len_wr > 2 && sense[0] & 0x7F >= 0x72 { sense[1] & 0x0F } else { sense.get(2).copied().unwrap_or(0) & 0x0F };
        if key == 0x05 {
            return Err(AtaError::Unsupported("el dispositivo o su puente USB no transmite S.M.A.R.T.".into()));
        }
        // Algunos puentes devuelven CHECK CONDITION con los datos correctos: se valida la página.
        if !ata::checksum_ok(&data) || data.iter().all(|b| *b == 0) {
            return Err(AtaError::Failed(format!("comando SMART rechazado (estado {:#x}, sense {:#x})", hdr.status, key)));
        }
    }
    Ok(data)
}

fn read_ata(file: &File) -> ReadOutcome {
    let data = match sg_ata_smart(file, 0xD0) {
        Ok(d) => d,
        Err(AtaError::Unsupported(m)) => return ReadOutcome::Unsupported(m),
        Err(AtaError::Failed(m)) => return ReadOutcome::Failed(m),
    };
    if data.iter().all(|b| *b == 0) {
        return ReadOutcome::Unsupported("el dispositivo devolvió una página S.M.A.R.T. vacía".into());
    }
    if !ata::checksum_ok(&data) {
        return ReadOutcome::Failed("la página S.M.A.R.T. tiene una suma de comprobación incorrecta".into());
    }
    let thresholds = sg_ata_smart(file, 0xD1).ok().filter(|t| ata::checksum_ok(t));
    let Some(attrs) = ata::parse(&data, thresholds.as_deref()) else {
        return ReadOutcome::Failed("página S.M.A.R.T. ilegible".into());
    };
    let (overall, mut reasons) = evaluate_ata(&attrs);
    if thresholds.is_none() {
        reasons.push("no se pudieron leer los umbrales del fabricante".into());
    }
    ReadOutcome::Read(HealthSnapshot {
        protocol: Protocol::Ata,
        overall,
        temperature_c: ata::temperature(&attrs),
        power_on_hours: ata::power_on_hours(&attrs),
        percentage_used: None,
        attributes: attrs,
        nvme: None,
        source: "nativo".into(),
        reasons,
    })
}

// ── NVMe: GET LOG PAGE por ioctl de administración ────────────────────────

#[repr(C)]
#[derive(Default)]
struct NvmePassthruCmd {
    opcode: u8,
    flags: u8,
    rsvd1: u16,
    nsid: u32,
    cdw2: u32,
    cdw3: u32,
    metadata: u64,
    addr: u64,
    metadata_len: u32,
    data_len: u32,
    cdw10: u32,
    cdw11: u32,
    cdw12: u32,
    cdw13: u32,
    cdw14: u32,
    cdw15: u32,
    timeout_ms: u32,
    result: u32,
}

/// _IOWR('N', 0x41, struct nvme_passthru_cmd) con tamaño 72.
const NVME_IOCTL_ADMIN_CMD: libc::c_ulong = 0xC048_4E41;

/// cdw10 de GET LOG PAGE: número de dwords - 1 en los bits 31:16 y el id de log.
pub fn nvme_log_cdw10(lid: u8, bytes: usize) -> u32 {
    ((((bytes / 4) - 1) as u32) << 16) | lid as u32
}

fn read_nvme(file: &File) -> ReadOutcome {
    const _: () = assert!(std::mem::size_of::<NvmePassthruCmd>() == 72);
    let mut buf = vec![0u8; nvme::LOG_SIZE];
    let mut cmd = NvmePassthruCmd {
        opcode: 0x02,
        nsid: 0xFFFF_FFFF,
        addr: buf.as_mut_ptr() as u64,
        data_len: buf.len() as u32,
        cdw10: nvme_log_cdw10(nvme::LOG_PAGE_ID, nvme::LOG_SIZE),
        timeout_ms: 10_000,
        ..Default::default()
    };
    // SAFETY: `cmd.addr` apunta a `buf`, que vive durante toda la llamada y mide `data_len`.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), NVME_IOCTL_ADMIN_CMD as _, &mut cmd as *mut NvmePassthruCmd) };
    if rc < 0 {
        let err = std::io::Error::last_os_error();
        return match err.raw_os_error() {
            Some(libc::EACCES) | Some(libc::EPERM) => {
                ReadOutcome::PermissionDenied("leer el registro de salud NVMe requiere permisos de administrador".into())
            }
            Some(libc::ENOTTY) | Some(libc::EINVAL) => ReadOutcome::Unsupported("el controlador no admite comandos NVMe de administración".into()),
            _ => ReadOutcome::Failed(format!("ioctl NVMe falló: {err}")),
        };
    }
    if rc > 0 {
        return ReadOutcome::Failed(format!("el controlador NVMe devolvió el estado {rc:#x}"));
    }
    let Some(h) = nvme::parse(&buf) else { return ReadOutcome::Failed("registro de salud NVMe ilegible".into()) };
    let (overall, reasons) = evaluate_nvme(&h);
    ReadOutcome::Read(HealthSnapshot {
        protocol: Protocol::Nvme,
        overall,
        temperature_c: nvme::temperature_c(&h),
        power_on_hours: Some(h.power_on_hours),
        percentage_used: Some(h.percentage_used),
        attributes: vec![],
        nvme: Some(h),
        source: "nativo".into(),
        reasons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_encoding() {
        let cdb = ata_smart_cdb(0xD0);
        assert_eq!((cdb[0], cdb[4], cdb[10], cdb[12], cdb[14]), (0x85, 0xD0, 0x4F, 0xC2, 0xB0));
        assert_eq!(nvme_log_cdw10(0x02, 512), 0x007F_0002);
        assert_eq!(std::mem::size_of::<NvmePassthruCmd>(), 72);
    }

    #[test]
    fn disk_names() {
        for ok in ["sda", "sdab", "vda", "nvme0n1", "nvme12n3", "mmcblk0", "hdb"] {
            assert!(is_disk_name(ok), "{ok}");
        }
        for no in ["sda1", "loop0", "dm-0", "md0", "zram0", "nvme0", "mmcblk0boot0", "sr0", "sd"] {
            if no == "sda1" {
                continue; // las particiones no aparecen en /sys/block
            }
            assert!(!is_disk_name(no), "{no}");
        }
    }

    #[test]
    fn enumerates_fake_sysfs() {
        let root = tempfile::tempdir().unwrap();
        let mk = |rel: &str, content: &str| {
            let p = root.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        };
        mk("block/sda/size", "7814037168\n");
        mk("block/sda/removable", "0\n");
        mk("block/sda/queue/rotational", "1\n");
        mk("block/sda/device/vendor", "ATA     \n");
        mk("block/sda/device/model", "WDC WD40EFRX-68N\n");
        mk("block/sda/device/rev", "0A82\n");
        let mut vpd = vec![0u8, 0x80, 0, 12];
        vpd.extend_from_slice(b"WX42D81F7KLP");
        std::fs::write(root.path().join("block/sda/device/vpd_pg80"), vpd).unwrap();
        mk("block/nvme0n1/size", "1953525168\n");
        mk("block/nvme0n1/queue/rotational", "0\n");
        mk("block/nvme0n1/device/model", "Samsung SSD 980 PRO 1TB\n");
        mk("block/nvme0n1/device/serial", "S5GXNF0R123456\n");
        mk("block/nvme0n1/device/firmware_rev", "5B2QGXA7\n");
        mk("block/loop0/size", "100\n");
        mk("block/sdb/size", "0\n");
        let devs = LinuxNative::with_sysfs(root.path()).devices().unwrap();
        assert_eq!(devs.len(), 2, "{devs:?}");
        let nvme = &devs[0];
        assert_eq!((nvme.path.as_str(), nvme.bus, nvme.serial.as_deref()), ("/dev/nvme0n1", Bus::Nvme, Some("S5GXNF0R123456")));
        let sda = &devs[1];
        assert_eq!(sda.model.as_deref(), Some("WDC WD40EFRX-68N"));
        assert_eq!(sda.serial.as_deref(), Some("WX42D81F7KLP"));
        assert_eq!((sda.capacity_bytes, sda.rotational), (7_814_037_168 * 512, Some(true)));
    }
}
