//! Detección de volúmenes dentro de una imagen: esquema de particiones y tipo de
//! sistema de archivos por firma. Solo HFS+/HFSX se analiza en esta fase; el resto
//! se registra para mostrarlo con su estado.

use blocksmith_fs_hfsplus::volume::volume_name;
use blocksmith_fs_hfsplus::Volume;
use blocksmith_io::endian::le_u16;
use blocksmith_io::{BlockDevice, IoResult, Slice};
use blocksmith_partition::{detect, Hint, Scheme};

#[derive(Debug, Clone)]
pub struct ProbedVolume {
    pub partition_index: Option<u32>,
    pub offset: u64,
    pub length: u64,
    pub fs_type: &'static str,
    pub name: Option<String>,
    pub block_size: u32,
    pub total_blocks: u64,
    pub free_blocks: u64,
    pub file_count: Option<u32>,
    pub folder_count: Option<u32>,
    pub case_sensitive: bool,
    pub journaled: bool,
}

pub fn scheme_label(s: Scheme) -> &'static str {
    match s {
        Scheme::Gpt => "gpt",
        Scheme::Apm => "apm",
        Scheme::Mbr => "mbr",
        Scheme::None => "none",
    }
}

fn identify(dev: &dyn BlockDevice) -> IoResult<Option<&'static str>> {
    if blocksmith_partition::apfs_container::is_apfs_container(dev)? {
        return Ok(Some("apfs"));
    }
    if dev.len() >= 1024 + 64 {
        let boot = dev.read_vec(0, 16)?;
        if &boot[3..11] == b"NTFS    " {
            return Ok(Some("ntfs"));
        }
        let sb = dev.read_vec(1024, 64)?;
        if le_u16(&sb, 56) == 0xEF53 {
            return Ok(Some("ext4"));
        }
    }
    Ok(None)
}

/// Lista los volúmenes reconocibles del dispositivo.
pub fn probe(dev: &dyn BlockDevice) -> IoResult<(Scheme, Vec<ProbedVolume>)> {
    let table = detect(dev)?;
    let mut ranges: Vec<(Option<u32>, u64, u64, Option<String>)> = Vec::new();
    if table.scheme == Scheme::None {
        ranges.push((None, 0, dev.len(), None));
    }
    for p in &table.partitions {
        if !matches!(p.hint, Hint::Free | Hint::Reserved | Hint::EfiSystem) {
            ranges.push((Some(p.index), p.start, p.len, (!p.name.is_empty()).then(|| p.name.clone())));
        }
    }
    let mut out = Vec::new();
    for (idx, start, len, part_name) in ranges {
        let slice = Slice::new(dev, start, len)?;
        if let Some(vol) = Volume::open(&slice)? {
            let vh = &vol.vh;
            out.push(ProbedVolume {
                partition_index: idx,
                offset: start,
                length: len,
                fs_type: if vh.is_hfsx() { "hfsx" } else { "hfsplus" },
                name: volume_name(&vol).or(part_name),
                block_size: vh.block_size,
                total_blocks: vh.total_blocks as u64,
                free_blocks: vh.free_blocks as u64,
                file_count: Some(vh.file_count),
                folder_count: Some(vh.folder_count),
                case_sensitive: vh.is_hfsx(),
                journaled: vh.is_journaled(),
            });
        } else if let Some(fs) = identify(&slice)? {
            out.push(ProbedVolume {
                partition_index: idx,
                offset: start,
                length: len,
                fs_type: fs,
                name: part_name,
                block_size: 0,
                total_blocks: 0,
                free_blocks: 0,
                file_count: None,
                folder_count: None,
                case_sensitive: false,
                journaled: false,
            });
        }
    }
    Ok((table.scheme, out))
}
