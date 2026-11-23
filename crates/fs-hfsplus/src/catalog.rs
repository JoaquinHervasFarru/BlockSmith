//! Registros del catálogo: carpeta, archivo y threads (TN1150).

use std::cmp::Ordering;

use blocksmith_io::endian::{be_u16, be_u32, put_be_u16, put_be_u32};
use serde::Serialize;

use crate::unicode::{compare_binary, compare_folded};
use crate::volume_header::ForkData;

/// CNIDs reservados.
pub mod cnid {
    pub const ROOT_PARENT: u32 = 1;
    pub const ROOT_FOLDER: u32 = 2;
    pub const EXTENTS_FILE: u32 = 3;
    pub const CATALOG_FILE: u32 = 4;
    pub const BAD_BLOCKS: u32 = 5;
    pub const ALLOCATION_FILE: u32 = 6;
    pub const STARTUP_FILE: u32 = 7;
    pub const ATTRIBUTES_FILE: u32 = 8;
    pub const FIRST_USER: u32 = 16;
}

pub mod rtype {
    pub const FOLDER: u16 = 1;
    pub const FILE: u16 = 2;
    pub const FOLDER_THREAD: u16 = 3;
    pub const FILE_THREAD: u16 = 4;
}

/// Flag de archivo: existe su thread record.
pub const FILE_THREAD_EXISTS: u16 = 0x0002;
pub const MAX_NAME: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct CatalogKey {
    pub parent_id: u32,
    pub name: Vec<u16>,
}

impl CatalogKey {
    /// `b` es la clave sin el campo keyLength.
    pub fn parse(b: &[u8]) -> Option<Self> {
        if b.len() < 6 {
            return None;
        }
        let len = be_u16(b, 4) as usize;
        if len > MAX_NAME || 6 + 2 * len > b.len() {
            return None;
        }
        let name = (0..len).map(|i| be_u16(b, 6 + 2 * i)).collect();
        Some(Self { parent_id: be_u32(b, 0), name })
    }

    /// Bytes de la clave incluyendo keyLength.
    pub fn to_bytes(&self) -> Vec<u8> {
        let key_len = 6 + 2 * self.name.len();
        let mut b = vec![0u8; 2 + key_len];
        put_be_u16(&mut b, 0, key_len as u16);
        put_be_u32(&mut b, 2, self.parent_id);
        put_be_u16(&mut b, 6, self.name.len() as u16);
        for (i, u) in self.name.iter().enumerate() {
            put_be_u16(&mut b, 8 + 2 * i, *u);
        }
        b
    }

    pub fn compare(&self, other: &Self, binary: bool) -> Ordering {
        self.parent_id.cmp(&other.parent_id).then_with(|| {
            if binary {
                compare_binary(&self.name, &other.name)
            } else {
                compare_folded(&self.name, &other.name)
            }
        })
    }
}

/// Campos comunes de carpetas y archivos que BlockSmith conserva tal cual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommonInfo {
    pub flags: u16,
    pub create_date: u32,
    pub content_mod_date: u32,
    pub attribute_mod_date: u32,
    pub access_date: u32,
    pub backup_date: u32,
    pub permissions: [u8; 16],
    pub user_info: [u8; 16],
    pub finder_info: [u8; 16],
    pub text_encoding: u32,
}

impl CommonInfo {
    fn parse(b: &[u8], dates_at: usize) -> Self {
        let arr = |o: usize| -> [u8; 16] { b[o..o + 16].try_into().expect("16 bytes") };
        let p = dates_at + 20;
        Self {
            flags: be_u16(b, 2),
            create_date: be_u32(b, dates_at),
            content_mod_date: be_u32(b, dates_at + 4),
            attribute_mod_date: be_u32(b, dates_at + 8),
            access_date: be_u32(b, dates_at + 12),
            backup_date: be_u32(b, dates_at + 16),
            permissions: arr(p),
            user_info: arr(p + 16),
            finder_info: arr(p + 32),
            text_encoding: be_u32(b, p + 48),
        }
    }

    fn write(&self, b: &mut [u8], dates_at: usize) {
        put_be_u16(b, 2, self.flags);
        for (i, d) in
            [self.create_date, self.content_mod_date, self.attribute_mod_date, self.access_date, self.backup_date]
                .into_iter()
                .enumerate()
        {
            put_be_u32(b, dates_at + 4 * i, d);
        }
        let p = dates_at + 20;
        b[p..p + 16].copy_from_slice(&self.permissions);
        b[p + 16..p + 32].copy_from_slice(&self.user_info);
        b[p + 32..p + 48].copy_from_slice(&self.finder_info);
        put_be_u32(b, p + 48, self.text_encoding);
    }

    /// Modo POSIX (st_mode) guardado en los permisos BSD.
    pub fn file_mode(&self) -> u16 {
        be_u16(&self.permissions, 10)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderRecord {
    pub valence: u32,
    pub folder_id: u32,
    pub info: CommonInfo,
    /// Número de subcarpetas; solo es válido si `info.flags` tiene [`FOLDER_HAS_COUNT`].
    pub folder_count: u32,
}

/// Flag de carpeta: el campo `folder_count` está en uso.
pub const FOLDER_HAS_COUNT: u16 = 0x0010;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileRecord {
    pub file_id: u32,
    pub info: CommonInfo,
    pub data_fork: ForkData,
    pub rsrc_fork: ForkData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadRecord {
    pub parent_id: u32,
    pub name: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum CatalogRecord {
    Folder(FolderRecord),
    File(FileRecord),
    FolderThread(ThreadRecord),
    FileThread(ThreadRecord),
}

impl CatalogRecord {
    pub const FOLDER_SIZE: usize = 88;
    pub const FILE_SIZE: usize = 248;

    pub fn parse(b: &[u8]) -> Option<Self> {
        match be_u16(b, 0) {
            rtype::FOLDER if b.len() >= Self::FOLDER_SIZE => Some(Self::Folder(FolderRecord {
                valence: be_u32(b, 4),
                folder_id: be_u32(b, 8),
                info: CommonInfo::parse(b, 12),
                folder_count: be_u32(b, 84),
            })),
            rtype::FILE if b.len() >= Self::FILE_SIZE => Some(Self::File(FileRecord {
                file_id: be_u32(b, 8),
                info: CommonInfo::parse(b, 12),
                data_fork: ForkData::parse(&b[88..168]),
                rsrc_fork: ForkData::parse(&b[168..248]),
            })),
            t @ (rtype::FOLDER_THREAD | rtype::FILE_THREAD) => {
                let len = be_u16(b, 8) as usize;
                if len > MAX_NAME || b.len() < 10 + 2 * len {
                    return None;
                }
                let thread =
                    ThreadRecord { parent_id: be_u32(b, 4), name: (0..len).map(|i| be_u16(b, 10 + 2 * i)).collect() };
                Some(if t == rtype::FOLDER_THREAD { Self::FolderThread(thread) } else { Self::FileThread(thread) })
            }
            _ => None,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Folder(f) => {
                let mut b = vec![0u8; Self::FOLDER_SIZE];
                f.info.write(&mut b, 12);
                put_be_u16(&mut b, 0, rtype::FOLDER);
                put_be_u32(&mut b, 4, f.valence);
                put_be_u32(&mut b, 8, f.folder_id);
                put_be_u32(&mut b, 84, f.folder_count);
                b
            }
            Self::File(f) => {
                let mut b = vec![0u8; Self::FILE_SIZE];
                f.info.write(&mut b, 12);
                put_be_u16(&mut b, 0, rtype::FILE);
                put_be_u32(&mut b, 8, f.file_id);
                f.data_fork.write(&mut b[88..168]);
                f.rsrc_fork.write(&mut b[168..248]);
                b
            }
            Self::FolderThread(t) | Self::FileThread(t) => {
                let mut b = vec![0u8; 10 + 2 * t.name.len()];
                let ty = if matches!(self, Self::FolderThread(_)) { rtype::FOLDER_THREAD } else { rtype::FILE_THREAD };
                put_be_u16(&mut b, 0, ty);
                put_be_u32(&mut b, 4, t.parent_id);
                put_be_u16(&mut b, 8, t.name.len() as u16);
                for (i, u) in t.name.iter().enumerate() {
                    put_be_u16(&mut b, 10 + 2 * i, *u);
                }
                b
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unicode::to_hfs_name;

    #[test]
    fn key_and_records_roundtrip() {
        let key = CatalogKey { parent_id: 2, name: to_hfs_name("Fotos") };
        let bytes = key.to_bytes();
        assert_eq!(be_u16(&bytes, 0), 6 + 10);
        assert_eq!(CatalogKey::parse(&bytes[2..]).unwrap(), key);

        let info = CommonInfo {
            flags: 0,
            create_date: 1,
            content_mod_date: 2,
            attribute_mod_date: 3,
            access_date: 4,
            backup_date: 0,
            permissions: [0; 16],
            user_info: [0; 16],
            finder_info: [0; 16],
            text_encoding: 0,
        };
        let folder =
            CatalogRecord::Folder(FolderRecord { valence: 3, folder_id: 17, info: info.clone(), folder_count: 0 });
        assert_eq!(CatalogRecord::parse(&folder.to_bytes()).unwrap(), folder);
        let thread = CatalogRecord::FileThread(ThreadRecord { parent_id: 17, name: to_hfs_name("a.txt") });
        assert_eq!(CatalogRecord::parse(&thread.to_bytes()).unwrap(), thread);
        let data_fork = ForkData { logical_size: 10, ..Default::default() };
        let file = CatalogRecord::File(FileRecord { file_id: 18, info, data_fork, rsrc_fork: ForkData::default() });
        assert_eq!(CatalogRecord::parse(&file.to_bytes()).unwrap(), file);
    }
}
