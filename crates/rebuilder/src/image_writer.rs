//! Exporta un dispositivo o imagen a un archivo nuevo con el plan aplicado.

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use blocksmith_fs_core::{Progress, RebuildPlan};
use blocksmith_io::BlockDevice;
use thiserror::Error;

const CHUNK: usize = 4 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("el archivo de salida ya existe: {0}")]
    OutputExists(String),
    #[error("la escritura «{label}» (offset {offset}, {len} bytes) se sale del volumen")]
    OutOfRange { label: String, offset: u64, len: usize },
    #[error("operación cancelada")]
    Cancelled,
    #[error(transparent)]
    Device(#[from] blocksmith_io::IoError),
    #[error(transparent)]
    Os(#[from] std::io::Error),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ExportReport {
    pub bytes_total: u64,
    /// Bytes de datos copiados (los tramos de ceros se dejan como huecos).
    pub bytes_copied: u64,
    pub writes_applied: usize,
    pub bytes_written: u64,
}

fn check(plan: &RebuildPlan, vol_offset: u64, vol_len: u64, total: u64) -> Result<(), ExportError> {
    for w in &plan.writes {
        if w.offset + w.data.len() as u64 > vol_len || vol_offset + w.offset + w.data.len() as u64 > total {
            return Err(ExportError::OutOfRange { label: w.label.clone(), offset: w.offset, len: w.data.len() });
        }
    }
    Ok(())
}

/// Aplica el plan sobre una copia en memoria (tests y volúmenes pequeños).
pub fn apply_in_memory(img: &mut [u8], vol_offset: u64, vol_len: u64, plan: &RebuildPlan) -> Result<(), ExportError> {
    check(plan, vol_offset, vol_len, img.len() as u64)?;
    for w in plan.ordered_writes() {
        let o = (vol_offset + w.offset) as usize;
        img[o..o + w.data.len()].copy_from_slice(&w.data);
    }
    Ok(())
}

/// Copia `src` completo a `out` (archivo nuevo, disperso) y aplica el plan del
/// volumen que empieza en `vol_offset`. Nunca sobrescribe un archivo existente.
pub fn export_image(
    src: &dyn BlockDevice,
    vol_offset: u64,
    vol_len: u64,
    plan: &RebuildPlan,
    out: &Path,
    progress: &mut dyn Progress,
) -> Result<ExportReport, ExportError> {
    let total = src.len();
    check(plan, vol_offset, vol_len, total)?;
    let mut file: File = OpenOptions::new().write(true).create_new(true).open(out).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => ExportError::OutputExists(out.display().to_string()),
        _ => e.into(),
    })?;
    let result = (|| {
        let mut report = ExportReport { bytes_total: total, ..Default::default() };
        progress.phase("Copiando la imagen");
        file.set_len(total)?;
        let mut buf = vec![0u8; CHUNK];
        let mut off = 0u64;
        while off < total {
            if progress.is_cancelled() {
                return Err(ExportError::Cancelled);
            }
            let n = CHUNK.min((total - off) as usize);
            src.read_exact_at(off, &mut buf[..n])?;
            if buf[..n].iter().any(|&b| b != 0) {
                file.seek(SeekFrom::Start(off))?;
                file.write_all(&buf[..n])?;
                report.bytes_copied += n as u64;
            }
            off += n as u64;
            progress.advance(off, total);
        }
        progress.phase("Aplicando la reconstrucción");
        for w in plan.ordered_writes() {
            file.seek(SeekFrom::Start(vol_offset + w.offset))?;
            file.write_all(&w.data)?;
            report.writes_applied += 1;
            report.bytes_written += w.data.len() as u64;
        }
        file.sync_all()?;
        Ok(report)
    })();
    if result.is_err() {
        // No se deja una imagen a medias que pueda confundirse con una válida.
        drop(file);
        let _ = std::fs::remove_file(out);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use blocksmith_fs_core::{BlockWrite, WritePhase};
    use blocksmith_io::MemoryDevice;

    struct P;
    impl Progress for P {
        fn phase(&mut self, _: &str) {}
        fn advance(&mut self, _: u64, _: u64) {}
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    fn plan() -> RebuildPlan {
        let w = |offset, byte, phase| BlockWrite { offset, data: vec![byte; 4], phase, label: "t".into() };
        // Dos escrituras al mismo sitio: debe ganar la de la fase posterior.
        RebuildPlan { writes: vec![w(100, 2, WritePhase::Finalize), w(100, 1, WritePhase::Data)], ..Default::default() }
    }

    #[test]
    fn export_copies_and_applies_in_phase_order() {
        let mut data = vec![0u8; 10 * 1024 * 1024];
        data[5_000_000] = 7;
        let src = MemoryDevice::new(data.clone());
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.img");
        let r = export_image(&src, 1000, 2000, &plan(), &out, &mut P).unwrap();
        let got = std::fs::read(&out).unwrap();
        assert_eq!(got.len(), data.len());
        assert_eq!(got[5_000_000], 7);
        assert_eq!(&got[1100..1104], &[2, 2, 2, 2]);
        assert_eq!(r.writes_applied, 2);
        assert!(r.bytes_copied < data.len() as u64, "los tramos de ceros no se copian");
        // Nunca sobrescribe.
        assert!(matches!(export_image(&src, 1000, 2000, &plan(), &out, &mut P), Err(ExportError::OutputExists(_))));
    }

    #[test]
    fn rejects_writes_outside_the_volume() {
        let mut img = vec![0u8; 4096];
        assert!(apply_in_memory(&mut img, 0, 102, &plan()).is_err());
        apply_in_memory(&mut img, 0, 4096, &plan()).unwrap();
        assert_eq!(img[100], 2);
    }
}
