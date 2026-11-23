//! BitLocker / BitLocker To Go (F6b). Firma de volumen `-FVE-FS-`.

pub const VOLUME_SIGNATURE: &[u8; 8] = b"-FVE-FS-";

/// Valida el formato de una clave de recuperación de 48 dígitos
/// (8 grupos de 6). Cada grupo debe ser múltiplo de 11 y menor que 720 896.
/// Se usa en la UI para avisar de errores de tecleo antes de intentar desbloquear.
pub fn validate_recovery_password(input: &str) -> Result<[u16; 8], usize> {
    let groups: Vec<&str> = input.split('-').map(str::trim).collect();
    if groups.len() != 8 {
        return Err(groups.len().min(8));
    }
    let mut out = [0u16; 8];
    for (i, g) in groups.iter().enumerate() {
        let ok = g.len() == 6 && g.bytes().all(|b| b.is_ascii_digit());
        let n: u32 = if ok { g.parse().map_err(|_| i)? } else { return Err(i) };
        if !n.is_multiple_of(11) || n >= 720_896 {
            return Err(i);
        }
        out[i] = (n / 11) as u16;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_password_format() {
        let good = "000011-000022-000033-000044-000055-000066-000077-000088";
        assert_eq!(validate_recovery_password(good).unwrap()[7], 8);
        let bad = "000011-000022-000034-000044-000055-000066-000077-000088";
        assert_eq!(validate_recovery_password(bad), Err(2));
        assert!(validate_recovery_password("123").is_err());
    }
}
