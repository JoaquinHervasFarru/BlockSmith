//! Imágenes UDIF (.dmg), implementadas a partir de la descripción pública del formato.
//!
//! Estructura:
//! - Trailer `koly` de 512 bytes al final del archivo, con la ubicación del data fork
//!   y del plist XML.
//! - El plist contiene `resource-fork → blkx`: una lista de bloques `mish`, cada uno
//!   con «chunks» que describen cómo reconstruir un rango de sectores de 512 bytes.
//!
//! Tipos de chunk soportados: ceros, raw, zlib, ADC y bzip2. LZFSE y LZMA devuelven
//! `Unsupported` con un mensaje claro.

use std::io::Read;
use std::sync::Mutex;

use crate::endian::{be_u32, be_u64};
use crate::{check_bounds, BlockDevice, IoError, IoResult};

pub const KOLY_SIGNATURE: &[u8; 4] = b"koly";
pub const KOLY_SIZE: u64 = 512;
pub const MISH_SIGNATURE: &[u8; 4] = b"mish";
const SECTOR: u64 = 512;
const CHUNK_ENTRY_SIZE: usize = 40;
const MISH_HEADER_SIZE: usize = 204;

/// Devuelve `true` si el origen termina en un trailer UDIF.
pub fn has_koly_trailer(dev: &dyn BlockDevice) -> IoResult<bool> {
    if dev.len() < KOLY_SIZE {
        return Ok(false);
    }
    let sig = dev.read_vec(dev.len() - KOLY_SIZE, 4)?;
    Ok(sig == KOLY_SIGNATURE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    Zero,
    Raw,
    Zlib,
    Adc,
    Bzip2,
    Lzfse,
    Lzma,
}

impl ChunkKind {
    fn from_code(code: u32) -> Option<Option<Self>> {
        // Some(None) = entrada que no aporta datos (comentario / terminador).
        Some(match code {
            0x0000_0000 | 0x0000_0002 => Some(Self::Zero),
            0x0000_0001 => Some(Self::Raw),
            0x8000_0004 => Some(Self::Adc),
            0x8000_0005 => Some(Self::Zlib),
            0x8000_0006 => Some(Self::Bzip2),
            0x8000_0007 => Some(Self::Lzfse),
            0x8000_0008 => Some(Self::Lzma),
            0x7fff_fffe | 0xffff_ffff => None,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
struct Chunk {
    out_start: u64,
    out_len: u64,
    kind: ChunkKind,
    in_offset: u64,
    in_len: u64,
}

/// Partición declarada en el plist (`blkx`), útil para mostrar al usuario.
#[derive(Debug, Clone)]
pub struct UdifPartition {
    pub name: String,
    pub start_sector: u64,
    pub sector_count: u64,
}

/// Imagen UDIF descomprimida bajo demanda (con caché del último chunk).
pub struct UdifImage {
    inner: Box<dyn BlockDevice>,
    chunks: Vec<Chunk>,
    len: u64,
    partitions: Vec<UdifPartition>,
    cache: Mutex<Option<(usize, Vec<u8>)>>,
}

impl UdifImage {
    pub fn open(inner: Box<dyn BlockDevice>) -> IoResult<Self> {
        if !has_koly_trailer(inner.as_ref())? {
            return Err(IoError::UnknownFormat("falta el trailer koly".into()));
        }
        let koly = inner.read_vec(inner.len() - KOLY_SIZE, KOLY_SIZE as usize)?;
        let data_fork_offset = be_u64(&koly, 24);
        let xml_offset = be_u64(&koly, 216);
        let xml_length = be_u64(&koly, 224);
        if xml_length == 0 || xml_length > 64 * 1024 * 1024 {
            return Err(IoError::Corrupt(format!("longitud de plist no válida: {xml_length}")));
        }
        let xml = inner.read_vec(xml_offset, xml_length as usize)?;
        let blocks = parse_blkx(&xml)?;

        let mut chunks = Vec::new();
        let mut partitions = Vec::new();
        for (name, data) in blocks {
            let (part, mut c) = parse_mish(&data, data_fork_offset)?;
            partitions.push(UdifPartition { name, ..part });
            chunks.append(&mut c);
        }
        chunks.sort_by_key(|c| c.out_start);
        let len = chunks.last().map(|c| c.out_start + c.out_len).unwrap_or(0);
        for w in chunks.windows(2) {
            if w[0].out_start + w[0].out_len > w[1].out_start {
                return Err(IoError::Corrupt("chunks UDIF solapados".into()));
            }
        }
        Ok(Self { inner, chunks, len, partitions, cache: Mutex::new(None) })
    }

    pub fn partitions(&self) -> &[UdifPartition] {
        &self.partitions
    }

    fn chunk_data(&self, idx: usize) -> IoResult<Vec<u8>> {
        let c = &self.chunks[idx];
        let out_len = c.out_len as usize;
        let mut out = match c.kind {
            ChunkKind::Zero => return Ok(vec![0; out_len]),
            ChunkKind::Raw => return self.inner.read_vec(c.in_offset, c.in_len.min(c.out_len) as usize),
            _ => Vec::with_capacity(out_len),
        };
        let input = self.inner.read_vec(c.in_offset, c.in_len as usize)?;
        match c.kind {
            ChunkKind::Zlib => {
                flate2::read::ZlibDecoder::new(&input[..]).take(c.out_len).read_to_end(&mut out)?;
            }
            ChunkKind::Bzip2 => {
                bzip2::read::BzDecoder::new(&input[..]).take(c.out_len).read_to_end(&mut out)?;
            }
            ChunkKind::Adc => out = adc_decompress(&input, out_len)?,
            ChunkKind::Lzfse => return Err(IoError::Unsupported("DMG con compresión LZFSE")),
            ChunkKind::Lzma => return Err(IoError::Unsupported("DMG con compresión LZMA")),
            ChunkKind::Zero | ChunkKind::Raw => unreachable!(),
        }
        out.resize(out_len, 0);
        Ok(out)
    }
}

impl BlockDevice for UdifImage {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> IoResult<()> {
        check_bounds(self.len, offset, buf.len())?;
        let mut pos = offset;
        let mut done = 0usize;
        while done < buf.len() {
            // Último chunk cuyo inicio es <= pos.
            let idx = self.chunks.partition_point(|c| c.out_start <= pos).saturating_sub(1);
            let c = &self.chunks[idx];
            let within = pos.checked_sub(c.out_start).filter(|&w| w < c.out_len);
            let n;
            match within {
                None => {
                    // Hueco no descrito por ningún chunk: se lee como ceros.
                    let next = self.chunks.get(idx + 1).map(|c| c.out_start).unwrap_or(self.len);
                    n = ((next.max(pos + 1) - pos) as usize).min(buf.len() - done);
                    buf[done..done + n].fill(0);
                }
                Some(w) => {
                    n = ((c.out_len - w) as usize).min(buf.len() - done);
                    let mut cache = self.cache.lock().expect("caché UDIF envenenada");
                    if cache.as_ref().map(|(i, _)| *i) != Some(idx) {
                        *cache = Some((idx, self.chunk_data(idx)?));
                    }
                    let data = &cache.as_ref().expect("caché recién rellenada").1;
                    buf[done..done + n].copy_from_slice(&data[w as usize..w as usize + n]);
                }
            }
            done += n;
            pos += n as u64;
        }
        Ok(())
    }
}

fn parse_blkx(xml: &[u8]) -> IoResult<Vec<(String, Vec<u8>)>> {
    let value = plist::Value::from_reader_xml(xml)
        .map_err(|e| IoError::Corrupt(format!("plist UDIF ilegible: {e}")))?;
    let blkx = value
        .as_dictionary()
        .and_then(|d| d.get("resource-fork"))
        .and_then(|v| v.as_dictionary())
        .and_then(|d| d.get("blkx"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| IoError::Corrupt("el plist no contiene resource-fork/blkx".into()))?;
    let mut out = Vec::new();
    for entry in blkx {
        let Some(d) = entry.as_dictionary() else { continue };
        let Some(data) = d.get("Data").and_then(|v| v.as_data()) else { continue };
        let name = d
            .get("Name")
            .or_else(|| d.get("CFName"))
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        out.push((name, data.to_vec()));
    }
    Ok(out)
}

fn parse_mish(data: &[u8], data_fork_offset: u64) -> IoResult<(UdifPartition, Vec<Chunk>)> {
    if data.len() < MISH_HEADER_SIZE || &data[0..4] != MISH_SIGNATURE {
        return Err(IoError::Corrupt("bloque mish no válido".into()));
    }
    let first_sector = be_u64(data, 8);
    let sector_count = be_u64(data, 16);
    let base = data_fork_offset + be_u64(data, 24);
    let n = be_u32(data, 200) as usize;
    if data.len() < MISH_HEADER_SIZE + n * CHUNK_ENTRY_SIZE {
        return Err(IoError::Corrupt("tabla de chunks mish truncada".into()));
    }
    let mut chunks = Vec::with_capacity(n);
    for i in 0..n {
        let e = &data[MISH_HEADER_SIZE + i * CHUNK_ENTRY_SIZE..][..CHUNK_ENTRY_SIZE];
        let code = be_u32(e, 0);
        let kind = ChunkKind::from_code(code)
            .ok_or_else(|| IoError::Corrupt(format!("tipo de chunk desconocido 0x{code:08x}")))?;
        let Some(kind) = kind else { continue };
        let sectors = be_u64(e, 16);
        if sectors == 0 {
            continue;
        }
        chunks.push(Chunk {
            out_start: (first_sector + be_u64(e, 8)) * SECTOR,
            out_len: sectors * SECTOR,
            kind,
            in_offset: base + be_u64(e, 24),
            in_len: be_u64(e, 32),
        });
    }
    let part = UdifPartition { name: String::new(), start_sector: first_sector, sector_count };
    Ok((part, chunks))
}

/// Descompresor ADC (Apple Data Compression): un LZ77 sencillo de tres tipos de token.
pub fn adc_decompress(input: &[u8], expected: usize) -> IoResult<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(expected);
    let mut i = 0;
    let bad = || IoError::Corrupt("flujo ADC no válido".into());
    while i < input.len() && out.len() < expected {
        let b = input[i];
        if b & 0x80 != 0 {
            let len = (b & 0x7f) as usize + 1;
            let lit = input.get(i + 1..i + 1 + len).ok_or_else(bad)?;
            out.extend_from_slice(lit);
            i += 1 + len;
        } else {
            let (len, dist, adv) = if b & 0x40 != 0 {
                let hi = *input.get(i + 1).ok_or_else(bad)? as usize;
                let lo = *input.get(i + 2).ok_or_else(bad)? as usize;
                ((b & 0x3f) as usize + 4, (hi << 8) | lo, 3)
            } else {
                let lo = *input.get(i + 1).ok_or_else(bad)? as usize;
                (((b & 0x3f) >> 2) as usize + 3, (((b & 0x03) as usize) << 8) | lo, 2)
            };
            let start = out.len().checked_sub(dist + 1).ok_or_else(bad)?;
            for k in 0..len {
                let byte = out[start + k];
                out.push(byte);
            }
            i += adv;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryDevice;
    use std::io::Write;

    #[test]
    fn detects_trailer() {
        let mut data = vec![0u8; 2048];
        data[2048 - 512..2048 - 508].copy_from_slice(b"koly");
        assert!(has_koly_trailer(&MemoryDevice::new(data)).unwrap());
        assert!(!has_koly_trailer(&MemoryDevice::new(vec![0; 2048])).unwrap());
    }

    #[test]
    fn adc_literal_and_back_references() {
        // "abc" literal, luego copia de 3 bytes a distancia 3 (token de 2 bytes), y
        // copia de 4 bytes a distancia 6 (token de 3 bytes).
        let input = [0x82, b'a', b'b', b'c', 0x00, 0x02, 0x40, 0x00, 0x05];
        assert_eq!(adc_decompress(&input, 10).unwrap(), b"abcabcabca");
    }

    /// Construye un DMG sintético: 3 chunks (zlib, ceros, raw) + comentario + terminador.
    fn synthetic_dmg(payload: &[u8]) -> Vec<u8> {
        assert_eq!(payload.len(), 48 * 512);
        let mut file = Vec::new();
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&payload[..16 * 512]).unwrap();
        let zdata = z.finish().unwrap();
        file.extend_from_slice(&zdata);
        let raw_off = file.len() as u64;
        file.extend_from_slice(&payload[32 * 512..]);

        let entry = |code: u32, sec: u64, cnt: u64, off: u64, len: u64| {
            let mut e = vec![0u8; 40];
            e[0..4].copy_from_slice(&code.to_be_bytes());
            e[8..16].copy_from_slice(&sec.to_be_bytes());
            e[16..24].copy_from_slice(&cnt.to_be_bytes());
            e[24..32].copy_from_slice(&off.to_be_bytes());
            e[32..40].copy_from_slice(&len.to_be_bytes());
            e
        };
        let mut mish = vec![0u8; MISH_HEADER_SIZE];
        mish[0..4].copy_from_slice(b"mish");
        mish[4..8].copy_from_slice(&1u32.to_be_bytes());
        mish[16..24].copy_from_slice(&48u64.to_be_bytes());
        mish[200..204].copy_from_slice(&5u32.to_be_bytes());
        mish.extend(entry(0x8000_0005, 0, 16, 0, zdata.len() as u64));
        mish.extend(entry(0x0000_0000, 16, 16, 0, 0));
        mish.extend(entry(0x7fff_fffe, 32, 0, 0, 0));
        mish.extend(entry(0x0000_0001, 32, 16, raw_off, 16 * 512));
        mish.extend(entry(0xffff_ffff, 48, 0, 0, 0));

        let mut part = plist::Dictionary::new();
        part.insert("Name".into(), "disco sintético (Apple_HFS : 0)".into());
        part.insert("Data".into(), plist::Value::Data(mish));
        let mut rf = plist::Dictionary::new();
        rf.insert("blkx".into(), plist::Value::Array(vec![part.into()]));
        let mut root = plist::Dictionary::new();
        root.insert("resource-fork".into(), rf.into());
        let mut xml = Vec::new();
        plist::Value::Dictionary(root).to_writer_xml(&mut xml).unwrap();

        let xml_off = file.len() as u64;
        file.extend_from_slice(&xml);
        let mut koly = vec![0u8; 512];
        koly[0..4].copy_from_slice(b"koly");
        koly[4..8].copy_from_slice(&4u32.to_be_bytes());
        koly[8..12].copy_from_slice(&512u32.to_be_bytes());
        koly[32..40].copy_from_slice(&xml_off.to_be_bytes());
        koly[216..224].copy_from_slice(&xml_off.to_be_bytes());
        koly[224..232].copy_from_slice(&(xml.len() as u64).to_be_bytes());
        file.extend_from_slice(&koly);
        file
    }

    #[test]
    fn reads_synthetic_dmg_across_chunks() {
        let mut payload: Vec<u8> = (0..48 * 512).map(|i| (i % 251) as u8).collect();
        payload[16 * 512..32 * 512].fill(0);
        let img = UdifImage::open(Box::new(MemoryDevice::new(synthetic_dmg(&payload)))).unwrap();
        assert_eq!(img.len(), payload.len() as u64);
        assert_eq!(img.partitions()[0].sector_count, 48);
        assert!(img.partitions()[0].name.contains("Apple_HFS"));
        // Lectura que cruza los tres chunks.
        let got = img.read_vec(8000, 12_000).unwrap();
        assert_eq!(got, &payload[8000..20_000]);
        assert_eq!(img.read_vec(0, payload.len()).unwrap(), payload);
        assert!(img.read_vec(payload.len() as u64 - 4, 8).is_err());
    }
}
