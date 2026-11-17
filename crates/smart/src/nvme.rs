//! Registro de salud NVMe («SMART / Health Information», página de log 0x02, 512 bytes),
//! según la especificación NVM Express Base.

use crate::NvmeHealth;

pub const LOG_PAGE_ID: u8 = 0x02;
pub const LOG_SIZE: usize = 512;

fn le(b: &[u8], off: usize, len: usize) -> u64 {
    // Los contadores de 128 bits se recortan a 64: suficiente en la práctica.
    let mut v = [0u8; 8];
    let n = len.min(8);
    v[..n].copy_from_slice(&b[off..off + n]);
    u64::from_le_bytes(v)
}

pub fn parse(b: &[u8]) -> Option<NvmeHealth> {
    if b.len() < LOG_SIZE {
        return None;
    }
    Some(NvmeHealth {
        critical_warning: b[0],
        temperature_k: u16::from_le_bytes([b[1], b[2]]),
        available_spare: b[3],
        spare_threshold: b[4],
        percentage_used: b[5],
        data_units_read: le(b, 32, 16),
        data_units_written: le(b, 48, 16),
        power_cycles: le(b, 112, 16),
        power_on_hours: le(b, 128, 16),
        unsafe_shutdowns: le(b, 144, 16),
        media_errors: le(b, 160, 16),
        error_log_entries: le(b, 176, 16),
    })
}

/// Temperatura compuesta en °C (el registro la da en kelvin; 0 = no disponible).
pub fn temperature_c(n: &NvmeHealth) -> Option<i16> {
    (n.temperature_k > 0).then(|| n.temperature_k as i16 - 273)
}

/// Bits del aviso crítico, en palabras.
pub fn warning_reasons(bits: u8) -> Vec<String> {
    let names = [
        "bloques de reserva por debajo del umbral",
        "temperatura fuera de rango",
        "fiabilidad degradada por errores internos",
        "el disco pasó a solo lectura",
        "fallo de la memoria de respaldo volátil",
        "memoria persistente en solo lectura",
    ];
    names.iter().enumerate().filter(|(i, _)| bits & (1 << i) != 0).map(|(_, n)| n.to_string()).collect()
}

/// Construye un registro válido (pruebas y reproducción).
pub fn build_log(h: &NvmeHealth) -> Vec<u8> {
    let mut b = vec![0u8; LOG_SIZE];
    b[0] = h.critical_warning;
    b[1..3].copy_from_slice(&h.temperature_k.to_le_bytes());
    b[3] = h.available_spare;
    b[4] = h.spare_threshold;
    b[5] = h.percentage_used;
    for (off, v) in [
        (32, h.data_units_read), (48, h.data_units_written), (112, h.power_cycles), (128, h.power_on_hours),
        (144, h.unsafe_shutdowns), (160, h.media_errors), (176, h.error_log_entries),
    ] {
        b[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_fields() {
        let h = NvmeHealth {
            critical_warning: 0b10, temperature_k: 311, available_spare: 100, spare_threshold: 10, percentage_used: 4,
            data_units_read: 9_000_000, data_units_written: 12_345_678, power_cycles: 1_204, power_on_hours: 6_120,
            unsafe_shutdowns: 31, media_errors: 0, error_log_entries: 2,
        };
        let log = build_log(&h);
        // Offsets fijos de la especificación.
        assert_eq!(log[5], 4);
        assert_eq!(u64::from_le_bytes(log[128..136].try_into().unwrap()), 6_120);
        let p = parse(&log).unwrap();
        assert_eq!(p, h);
        assert_eq!(temperature_c(&p), Some(38));
        assert_eq!(warning_reasons(p.critical_warning), vec!["temperatura fuera de rango"]);
        assert!(parse(&log[..10]).is_none());
    }
}
