//! Lectura de enteros con orden de bytes explícito. Devuelven 0 si el slice es
//! corto: quien llama valida los tamaños antes; así un dato corrupto nunca provoca pánico.

macro_rules! getter {
    ($name:ident, $ty:ty, $n:expr, $from:ident) => {
        #[inline]
        pub fn $name(b: &[u8], off: usize) -> $ty {
            b.get(off..off + $n)
                .map(|s| <$ty>::$from(s.try_into().expect("longitud fija")))
                .unwrap_or(0)
        }
    };
}

getter!(be_u16, u16, 2, from_be_bytes);
getter!(be_u32, u32, 4, from_be_bytes);
getter!(be_u64, u64, 8, from_be_bytes);
getter!(le_u16, u16, 2, from_le_bytes);
getter!(le_u32, u32, 4, from_le_bytes);
getter!(le_u64, u64, 8, from_le_bytes);

#[inline]
pub fn put_be_u16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_be_bytes());
}
#[inline]
pub fn put_be_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
#[inline]
pub fn put_be_u64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_be_bytes());
}
