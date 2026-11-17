//! Página de datos S.M.A.R.T. ATA (512 bytes) y página de umbrales.
//!
//! Formato (ATA/ATAPI Command Set): bytes 0–1 revisión; desde el byte 2, 30
//! entradas de 12 bytes: id, flags (u16 LE), valor actual, peor valor, 6 bytes de
//! valor bruto (LE) y 1 reservado. La página de umbrales usa la misma rejilla con
//! id y umbral. El byte 511 es una suma de comprobación (suma total ≡ 0 mod 256).

use crate::Attribute;

pub const PAGE: usize = 512;
const ENTRIES: usize = 30;
const ENTRY: usize = 12;

pub fn checksum_ok(page: &[u8]) -> bool {
    page.len() == PAGE && page.iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0
}

/// Nombres habituales de los atributos más comunes (identificadores del sector).
pub fn attribute_name(id: u8) -> &'static str {
    match id {
        1 => "Raw_Read_Error_Rate",
        2 => "Throughput_Performance",
        3 => "Spin_Up_Time",
        4 => "Start_Stop_Count",
        5 => "Reallocated_Sector_Ct",
        7 => "Seek_Error_Rate",
        8 => "Seek_Time_Performance",
        9 => "Power_On_Hours",
        10 => "Spin_Retry_Count",
        11 => "Calibration_Retry_Count",
        12 => "Power_Cycle_Count",
        170 => "Available_Reservd_Space",
        171 => "Program_Fail_Count",
        172 => "Erase_Fail_Count",
        173 => "Wear_Leveling_Count",
        174 => "Unexpect_Power_Loss_Ct",
        177 => "Wear_Leveling_Count",
        179 => "Used_Rsvd_Blk_Cnt_Tot",
        181 => "Program_Fail_Cnt_Total",
        182 => "Erase_Fail_Count_Total",
        183 => "Runtime_Bad_Block",
        184 => "End-to-End_Error",
        187 => "Reported_Uncorrect",
        188 => "Command_Timeout",
        189 => "High_Fly_Writes",
        190 => "Airflow_Temperature_Cel",
        191 => "G-Sense_Error_Rate",
        192 => "Power-Off_Retract_Count",
        193 => "Load_Cycle_Count",
        194 => "Temperature_Celsius",
        195 => "Hardware_ECC_Recovered",
        196 => "Reallocated_Event_Count",
        197 => "Current_Pending_Sector",
        198 => "Offline_Uncorrectable",
        199 => "UDMA_CRC_Error_Count",
        200 => "Multi_Zone_Error_Rate",
        231 => "SSD_Life_Left",
        232 => "Available_Reservd_Space",
        233 => "Media_Wearout_Indicator",
        235 => "POR_Recovery_Count",
        240 => "Head_Flying_Hours",
        241 => "Total_LBAs_Written",
        242 => "Total_LBAs_Read",
        _ => "Unknown_Attribute",
    }
}

/// Interpreta las páginas de datos y umbrales. Los umbrales son opcionales
/// (algunos puentes no los devuelven). Devuelve `None` si la página no es válida.
pub fn parse(data: &[u8], thresholds: Option<&[u8]>) -> Option<Vec<Attribute>> {
    if data.len() < PAGE {
        return None;
    }
    let thr = |id: u8| -> u8 {
        thresholds
            .filter(|t| t.len() >= PAGE)
            .and_then(|t| (0..ENTRIES).map(|i| &t[2 + i * ENTRY..2 + (i + 1) * ENTRY]).find(|e| e[0] == id))
            .map(|e| e[1])
            .unwrap_or(0)
    };
    let mut out = Vec::new();
    for i in 0..ENTRIES {
        let e = &data[2 + i * ENTRY..2 + (i + 1) * ENTRY];
        let id = e[0];
        if id == 0 {
            continue;
        }
        let mut raw = [0u8; 8];
        raw[..6].copy_from_slice(&e[5..11]);
        out.push(Attribute {
            id,
            name: attribute_name(id).to_string(),
            flags: u16::from_le_bytes([e[1], e[2]]),
            value: e[3],
            worst: e[4],
            threshold: thr(id),
            raw: u64::from_le_bytes(raw),
        });
    }
    Some(out)
}

/// Valor bruto interpretado como contador. Varios fabricantes guardan información
/// extra en los bytes altos; para los contadores críticos se usan los 32 bits bajos.
pub fn counter(a: &Attribute) -> u64 {
    a.raw & 0xFFFF_FFFF
}

/// Temperatura en °C (atributo 194 o, si no existe, 190): byte bajo del valor bruto.
pub fn temperature(attrs: &[Attribute]) -> Option<i16> {
    attrs
        .iter()
        .find(|a| a.id == 194)
        .or_else(|| attrs.iter().find(|a| a.id == 190))
        .map(|a| (a.raw & 0xFF) as i16)
        .filter(|t| (1..=125).contains(t))
}

/// Horas de funcionamiento (atributo 9): 32 bits bajos.
pub fn power_on_hours(attrs: &[Attribute]) -> Option<u64> {
    attrs.iter().find(|a| a.id == 9).map(|a| a.raw & 0xFFFF_FFFF)
}

/// Construye una página válida (para pruebas y para el backend de reproducción).
pub fn build_page(entries: &[(u8, u16, u8, u8, u64)], thresholds: bool) -> Vec<u8> {
    let mut p = vec![0u8; PAGE];
    p[0] = 0x10;
    for (i, (id, flags, value, worst, raw)) in entries.iter().enumerate().take(ENTRIES) {
        let e = &mut p[2 + i * ENTRY..2 + (i + 1) * ENTRY];
        e[0] = *id;
        if thresholds {
            e[1] = *value; // en la página de umbrales, el segundo campo es el umbral
        } else {
            e[1..3].copy_from_slice(&flags.to_le_bytes());
            e[3] = *value;
            e[4] = *worst;
            e[5..11].copy_from_slice(&raw.to_le_bytes()[..6]);
        }
    }
    let sum = p[..PAGE - 1].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    p[PAGE - 1] = 0u8.wrapping_sub(sum);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_data_and_threshold_pages() {
        let data = build_page(
            &[(1, 0x000F, 200, 200, 0), (5, 0x0033, 199, 199, 24), (9, 0x0032, 57, 57, 31_870), (194, 0x0022, 110, 101, 0x0000_2D00_1B00_0028)],
            false,
        );
        let thr = build_page(&[(1, 0, 51, 0, 0), (5, 0, 140, 0, 0), (9, 0, 0, 0, 0), (194, 0, 0, 0, 0)], true);
        assert!(checksum_ok(&data) && checksum_ok(&thr));
        let attrs = parse(&data, Some(&thr)).unwrap();
        assert_eq!(attrs.len(), 4);
        let r = &attrs[1];
        assert_eq!((r.id, r.name.as_str(), r.value, r.threshold, r.raw, r.flags), (5, "Reallocated_Sector_Ct", 199, 140, 24, 0x33));
        assert_eq!(power_on_hours(&attrs), Some(31_870));
        // Temperatura: byte bajo (40 °C) aunque los bytes altos traigan mínimos/máximos.
        assert_eq!(temperature(&attrs), Some(40));
        assert!(parse(&data[..100], None).is_none());
    }

    #[test]
    fn missing_thresholds_are_zero() {
        let data = build_page(&[(5, 0, 100, 100, 3)], false);
        let attrs = parse(&data, None).unwrap();
        assert_eq!(attrs[0].threshold, 0);
        assert!(!attrs[0].failing());
    }

    #[test]
    fn checksum_detects_corruption() {
        let mut data = build_page(&[(5, 0, 100, 100, 3)], false);
        data[10] ^= 1;
        assert!(!checksum_ok(&data));
    }
}
