//! Comprobación rápida: descomprime un .dmg completo y muestra tamaño y firma en el offset 1024.
use blocksmith_io::{BlockDevice, RawImage, UdifImage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("uso: udif_check <imagen.dmg>")?;
    let img = UdifImage::open(Box::new(RawImage::open(path)?))?;
    for p in img.partitions() {
        println!("bloque: {:?} sectores {}+{}", p.name, p.start_sector, p.sector_count);
    }
    let mut buf = vec![0u8; 1 << 20];
    let mut off = 0;
    while off < img.len() {
        let n = buf.len().min((img.len() - off) as usize);
        img.read_exact_at(off, &mut buf[..n])?;
        off += n as u64;
    }
    let sig = img.read_vec(1024, 2)?;
    println!("tamaño descomprimido: {} bytes; firma en 1024: {:?}", img.len(), String::from_utf8_lossy(&sig));
    Ok(())
}
