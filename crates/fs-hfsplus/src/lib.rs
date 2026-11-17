//! Parser y reconstrucción de HFS+/HFSX (basado en Apple TN1150).

pub mod volume_header;
pub mod btree;
pub mod catalog;
pub mod extents;
pub mod bitmap;
pub mod unicode;

/// Firmas del Volume Header (TN1150).
pub const SIGNATURE_HFSPLUS: [u8; 2] = *b"H+";
pub const SIGNATURE_HFSX: [u8; 2] = *b"HX";
/// El Volume Header está a 1024 bytes del inicio del volumen.
pub const VOLUME_HEADER_OFFSET: u64 = 1024;
