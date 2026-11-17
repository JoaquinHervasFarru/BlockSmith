//! Tablas sintéticas construidas byte a byte siguiendo las especificaciones.

use dirforge_io::MemoryDevice;
use dirforge_partition::{detect, Hint, Scheme};

const MB: usize = 1024 * 1024;

fn guid_bytes(s: &str) -> [u8; 16] {
    let hex = s.replace('-', "");
    let h: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
    let mut out = [0u8; 16];
    // Orden mixto de GPT: los tres primeros campos en little-endian.
    out[0..4].copy_from_slice(&[h[3], h[2], h[1], h[0]]);
    out[4..6].copy_from_slice(&[h[5], h[4]]);
    out[6..8].copy_from_slice(&[h[7], h[6]]);
    out[8..16].copy_from_slice(&h[8..16]);
    out
}

fn gpt_image(corrupt_primary: bool) -> Vec<u8> {
    let mut img = vec![0u8; 8 * MB];
    let sectors = (img.len() / 512) as u64;
    // MBR protector
    img[446 + 4] = 0xEE;
    img[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    img[446 + 12..446 + 16].copy_from_slice(&((sectors - 1) as u32).to_le_bytes());
    img[510] = 0x55;
    img[511] = 0xAA;

    let mut entries = vec![0u8; 128 * 128];
    let mut put = |i: usize, ty: &str, first: u64, last: u64, name: &str| {
        let e = &mut entries[i * 128..(i + 1) * 128];
        e[0..16].copy_from_slice(&guid_bytes(ty));
        e[16] = i as u8 + 1;
        e[32..40].copy_from_slice(&first.to_le_bytes());
        e[40..48].copy_from_slice(&last.to_le_bytes());
        for (k, u) in name.encode_utf16().enumerate() {
            e[56 + k * 2..58 + k * 2].copy_from_slice(&u.to_le_bytes());
        }
    };
    put(0, "C12A7328-F81F-11D2-BA4B-00A0C93EC93B", 40, 2087, "EFI System Partition");
    put(1, "48465300-0000-11AA-AA11-00306543ECAC", 2088, 12287, "Archivo Fotos");
    put(2, "0FC63DAF-8483-4772-8E79-3D69D8477DE4", 12288, sectors - 40, "datos");
    let entries_crc = crc32fast::hash(&entries);

    let header = |my: u64, alt: u64, entries_lba: u64| {
        let mut h = vec![0u8; 92];
        h[0..8].copy_from_slice(b"EFI PART");
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        h[12..16].copy_from_slice(&92u32.to_le_bytes());
        h[24..32].copy_from_slice(&my.to_le_bytes());
        h[32..40].copy_from_slice(&alt.to_le_bytes());
        h[40..48].copy_from_slice(&34u64.to_le_bytes());
        h[48..56].copy_from_slice(&(sectors - 34).to_le_bytes());
        h[72..80].copy_from_slice(&entries_lba.to_le_bytes());
        h[80..84].copy_from_slice(&128u32.to_le_bytes());
        h[84..88].copy_from_slice(&128u32.to_le_bytes());
        h[88..92].copy_from_slice(&entries_crc.to_le_bytes());
        let crc = crc32fast::hash(&h);
        h[16..20].copy_from_slice(&crc.to_le_bytes());
        h
    };
    img[512..604].copy_from_slice(&header(1, sectors - 1, 2));
    img[1024..1024 + entries.len()].copy_from_slice(&entries);
    let backup_entries = (sectors - 33) as usize * 512;
    img[backup_entries..backup_entries + entries.len()].copy_from_slice(&entries);
    let last = (sectors - 1) as usize * 512;
    img[last..last + 92].copy_from_slice(&header(sectors - 1, 1, sectors - 33));
    if corrupt_primary {
        img[512 + 40] ^= 0xFF;
    }
    img
}

#[test]
fn gpt_primary() {
    let t = detect(&MemoryDevice::new(gpt_image(false))).unwrap();
    assert_eq!(t.scheme, Scheme::Gpt);
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
    assert_eq!(t.partitions.len(), 3);
    let hfs = &t.partitions[1];
    assert_eq!(hfs.hint, Hint::HfsPlus);
    assert_eq!(hfs.name, "Archivo Fotos");
    assert_eq!(hfs.start, 2088 * 512);
    assert_eq!(hfs.len, (12287 - 2088 + 1) * 512);
    assert_eq!(t.partitions[0].hint, Hint::EfiSystem);
}

#[test]
fn gpt_falls_back_to_backup_header() {
    let t = detect(&MemoryDevice::new(gpt_image(true))).unwrap();
    assert_eq!(t.scheme, Scheme::Gpt);
    assert_eq!(t.partitions.len(), 3);
    assert!(t.warnings[0].contains("respaldo"));
}

#[test]
fn apm_map() {
    let mut img = vec![0u8; 4 * MB];
    img[0..2].copy_from_slice(b"ER");
    img[2..4].copy_from_slice(&512u16.to_be_bytes());
    let entry = |img: &mut Vec<u8>, i: usize, start: u32, len: u32, name: &str, ty: &str| {
        let e = &mut img[512 * (1 + i)..512 * (2 + i)];
        e[0..2].copy_from_slice(b"PM");
        e[4..8].copy_from_slice(&2u32.to_be_bytes());
        e[8..12].copy_from_slice(&start.to_be_bytes());
        e[12..16].copy_from_slice(&len.to_be_bytes());
        e[16..16 + name.len()].copy_from_slice(name.as_bytes());
        e[48..48 + ty.len()].copy_from_slice(ty.as_bytes());
    };
    entry(&mut img, 0, 1, 63, "Apple", "Apple_partition_map");
    entry(&mut img, 1, 64, 8000, "disk image", "Apple_HFS");
    let t = detect(&MemoryDevice::new(img)).unwrap();
    assert_eq!(t.scheme, Scheme::Apm);
    assert_eq!(t.partitions[1].hint, Hint::HfsPlus);
    assert_eq!(t.partitions[1].start, 64 * 512);
}

#[test]
fn mbr_and_ntfs_boot_sector() {
    let mut img = vec![0u8; 4 * MB];
    img[446 + 4] = 0x07;
    img[446 + 8..446 + 12].copy_from_slice(&2048u32.to_le_bytes());
    img[446 + 12..446 + 16].copy_from_slice(&4096u32.to_le_bytes());
    img[510] = 0x55;
    img[511] = 0xAA;
    let t = detect(&MemoryDevice::new(img.clone())).unwrap();
    assert_eq!(t.scheme, Scheme::Mbr);
    assert_eq!(t.partitions[0].hint, Hint::Ntfs);

    // Un volumen NTFS sin tabla no debe confundirse con un MBR.
    img[3..11].copy_from_slice(b"NTFS    ");
    assert_eq!(detect(&MemoryDevice::new(img)).unwrap().scheme, Scheme::None);
}

#[test]
fn no_table() {
    assert_eq!(detect(&MemoryDevice::new(vec![0; MB])).unwrap().scheme, Scheme::None);
}
