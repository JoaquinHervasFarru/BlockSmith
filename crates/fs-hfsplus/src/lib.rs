//! HFS+/HFSX: lectura, verificación y (desde F2) reconstrucción del catálogo.
//!
//! Implementación propia basada en Apple Technical Note TN1150.

pub mod bitmap;
pub mod btree;
pub mod catalog;
pub mod check;
pub mod extents;
pub mod fixtures;
pub mod fork;
pub mod unicode;
pub mod volume;
pub mod volume_header;

use std::path::Path;

use blocksmith_fs_core::{
    Capabilities, FileSystem, FsError, FsInfo, FsKind, FsResult, RebuildPlan, RecoverResult, ScanContext, ScanResult,
    Support,
};
use blocksmith_io::BlockDevice;

pub use check::{scan_volume, CatalogSnapshot, HfsScan};
pub use volume::{locate, Volume, VOLUME_HEADER_OFFSET};

/// Firmas del Volume Header (TN1150).
pub const SIGNATURE_HFSPLUS: [u8; 2] = *b"H+";
pub const SIGNATURE_HFSX: [u8; 2] = *b"HX";

#[derive(Debug, Default, Clone, Copy)]
pub struct HfsPlus;

impl FileSystem for HfsPlus {
    fn kind(&self) -> FsKind {
        FsKind::HfsPlus
    }

    fn probe(&self, dev: &dyn BlockDevice) -> FsResult<Option<FsInfo>> {
        let Some(vol) = Volume::open(dev)? else { return Ok(None) };
        let vh = &vol.vh;
        Ok(Some(FsInfo {
            kind: if vh.is_hfsx() { FsKind::Hfsx } else { FsKind::HfsPlus },
            name: None,
            block_size: vh.block_size,
            total_blocks: vh.total_blocks as u64,
            free_blocks: vh.free_blocks as u64,
            journaled: vh.is_journaled(),
            case_sensitive: false,
        }))
    }

    fn capabilities(&self) -> Capabilities {
        // F1: solo escaneo. Recuperación y reconstrucción llegan en F2.
        Capabilities { scan: Support::Yes, recover: Support::No, rebuild: Support::No, write_in_place: Support::No }
    }

    fn scan(&self, ctx: &mut ScanContext<'_>) -> FsResult<ScanResult> {
        let vol = Volume::open(ctx.device)?.ok_or_else(|| FsError::Invalid("no es un volumen HFS+/HFSX".into()))?;
        Ok(scan_volume(&vol, ctx.progress)?.result)
    }

    fn recover(&self, _ctx: &mut ScanContext<'_>, _dest: &Path) -> FsResult<RecoverResult> {
        Err(FsError::Unsupported("recuperación de archivos (fase F2)"))
    }

    fn plan_rebuild(&self, _ctx: &mut ScanContext<'_>) -> FsResult<RebuildPlan> {
        Err(FsError::Unsupported("reconstrucción (fase F2)"))
    }
}
