//! Backends por plataforma (F4): Linux (SG_IO / NVMe ioctl), Windows
//! (IOCTL_STORAGE_QUERY_PROPERTY) y macOS (`smartctl` como binario externo).

#[cfg(target_os = "linux")]
pub mod linux;
pub mod smartctl;
