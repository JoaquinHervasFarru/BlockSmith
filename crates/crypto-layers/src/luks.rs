//! LUKS1/LUKS2 (F6b). Cabecera `LUKS\xBA\xBE`; LUKS2 guarda dos copias de metadatos JSON.

pub const MAGIC: &[u8; 6] = b"LUKS\xba\xbe";
/// Magic de la cabecera secundaria de LUKS2.
pub const MAGIC_SECONDARY: &[u8; 6] = b"SKUL\xba\xbe";
