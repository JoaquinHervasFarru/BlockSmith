//! Comparación de nombres del catálogo.
//!
//! HFS+ ordena los nombres sin distinguir mayúsculas e ignorando ciertos caracteres
//! de formato; HFSX puede usar comparación binaria. Implementación propia: plegado
//! a minúsculas por la tabla de Unicode de Rust y lista de caracteres ignorables.
//!
//! Limitación conocida: la tabla de plegado original es anterior a Unicode moderno y
//! puede diferir en caracteres poco comunes. Las diferencias de orden detectadas solo
//! en esos casos se informan, pero no se tratan como corrupción.

use std::cmp::Ordering;

use unicode_normalization::UnicodeNormalization;

fn is_ignorable(u: u16) -> bool {
    matches!(u, 0x200C..=0x200F | 0x202A..=0x202E | 0x206A..=0x206F | 0xFEFF)
}

/// Pliega una unidad UTF-16. Devuelve `None` si es ignorable.
pub fn fold(u: u16) -> Option<u16> {
    if is_ignorable(u) {
        return None;
    }
    if u == 0 {
        return Some(0xFFFF);
    }
    if (b'A' as u16..=b'Z' as u16).contains(&u) {
        return Some(u + 32);
    }
    if u < 0x80 {
        return Some(u);
    }
    if let Some(c) = char::from_u32(u as u32) {
        let mut l = c.to_lowercase();
        if let (Some(first), None) = (l.next(), l.next()) {
            if (first as u32) <= 0xFFFF {
                return Some(first as u16);
            }
        }
    }
    Some(u)
}

/// Comparación HFS+ (insensible a mayúsculas).
pub fn compare_folded(a: &[u16], b: &[u16]) -> Ordering {
    let mut ia = a.iter().filter_map(|&u| fold(u));
    let mut ib = b.iter().filter_map(|&u| fold(u));
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x != y => return x.cmp(&y),
            _ => {}
        }
    }
}

/// Comparación binaria (HFSX con keyCompareType 0xBC).
pub fn compare_binary(a: &[u16], b: &[u16]) -> Ordering {
    a.cmp(b)
}

/// Convierte un nombre al formato almacenado: forma descompuesta en UTF-16.
pub fn to_hfs_name(name: &str) -> Vec<u16> {
    name.nfd().collect::<String>().encode_utf16().collect()
}

/// Nombre legible (recompuesto) a partir de UTF-16 almacenado.
pub fn from_hfs_name(units: &[u16]) -> String {
    String::from_utf16_lossy(units).nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        to_hfs_name(s)
    }

    #[test]
    fn case_insensitive_and_ignorables() {
        assert_eq!(compare_folded(&u("Fotos"), &u("fOTOS")), Ordering::Equal);
        assert_eq!(compare_folded(&u("abc"), &u("abd")), Ordering::Less);
        assert_eq!(compare_folded(&u("ab"), &u("abc")), Ordering::Less);
        let mut zwj = u("ab");
        zwj.insert(1, 0x200D);
        assert_eq!(compare_folded(&zwj, &u("AB")), Ordering::Equal);
        assert_eq!(compare_folded(&u("Ñandú"), &u("ñANDÚ")), Ordering::Equal);
        assert_eq!(compare_binary(&u("A"), &u("a")), Ordering::Less);
    }

    #[test]
    fn names_are_stored_decomposed() {
        assert_eq!(u("é"), vec![0x0065, 0x0301]);
        assert_eq!(from_hfs_name(&u("Facturación")), "Facturación");
    }
}
