import type { Bus, DiffChange, EncryptionScheme, FsType, Severity, SmartOverall } from "../types/models";

export const fsLabel: Record<FsType, string> = {
  hfsplus: "HFS+", hfsx: "HFSX", apfs: "APFS", ntfs: "NTFS", ext4: "ext4",
};

export const busLabel: Record<Bus, string> = { sata: "SATA", nvme: "NVMe", usb: "USB", other: "Otro" };

export const severityLabel: Record<Severity, string> = {
  critical: "Crítico", minor: "Menor", cosmetic: "Cosmético",
};

export const smartLabel: Record<SmartOverall, string> = {
  passed: "Sano", warning: "Atención", failing: "Fallando", unknown: "No disponible",
};

export const schemeLabel: Record<EncryptionScheme, string> = {
  bitlocker: "BitLocker", luks1: "LUKS1", luks2: "LUKS2", corestorage: "Core Storage",
  apfs: "FileVault (APFS)", fscrypt: "fscrypt", veracrypt: "VeraCrypt", hardware: "Hardware",
};

export const changeLabel: Record<DiffChange, string> = {
  recovered: "Recuperado", moved_to_recovered: "A «Recovered Items»", renamed: "Nombre normalizado",
  valence_fixed: "Recuento corregido", dropped: "Descartado",
  data_truncated: "Datos truncados", data_cloned: "Datos duplicados", unchanged_flagged: "Sin cambios · revisar",
};

export const sessionStatusLabel: Record<string, string> = {
  pending: "Pendiente", scanning: "Analizando", scanned: "Analizado", rebuilding: "Reconstruyendo",
  rebuilt: "Reconstrucción lista", exported: "Exportado", failed: "Error", cancelled: "Cancelado",
};

export const jobKindLabel: Record<string, string> = {
  scan: "Análisis del volumen", plan: "Plan de reconstrucción", export: "Exportación de la imagen reparada",
};

/** Etiqueta del sistema de archivos para cualquier cadena del backend. */
export const fsName = (fs: string) =>
  ({ hfsplus: "HFS+", hfsx: "HFSX", apfs: "APFS", ntfs: "NTFS", ext4: "ext4" } as Record<string, string>)[fs] ?? fs;
