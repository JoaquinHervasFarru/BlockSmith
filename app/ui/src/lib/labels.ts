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
  health: "Lectura de la salud de los discos",
  repair: "Reparación en el origen", rollback: "Reversión de una transacción", complete: "Compleción de una transacción",
};

/** Etiqueta del sistema de archivos para cualquier cadena del backend. */
export const fsName = (fs: string) =>
  ({ hfsplus: "HFS+", hfsx: "HFSX", apfs: "APFS", ntfs: "NTFS", ext4: "ext4" } as Record<string, string>)[fs] ?? fs;

export const readStatusLabel: Record<string, string> = {
  ok: "Leído", unsupported: "Sin datos S.M.A.R.T.", permission: "Sin permisos", failed: "Error de lectura",
};

export const ruleLabel: Record<string, { title: string; help: string; fields: { key: string; label: string; unit: string }[] }> = {
  SMART_FAILING: { title: "El disco predice un fallo", help: "Algún atributo cae por debajo del umbral del fabricante o el disco informa de un fallo.", fields: [] },
  REALLOCATED_INCREASE: { title: "Aumentan los sectores reasignados", help: "Compara con la lectura anterior.", fields: [{ key: "delta", label: "Aumento mínimo", unit: "sectores" }] },
  PENDING_SECTORS: { title: "Sectores pendientes de reasignación", help: "Sectores que el disco no pudo leer y espera reasignar.", fields: [{ key: "min", label: "A partir de", unit: "sectores" }] },
  NVME_CRITICAL_WARNING: { title: "Aviso crítico NVMe", help: "Cualquier bit del aviso crítico del registro de salud.", fields: [] },
  NVME_WEAR: { title: "Desgaste NVMe", help: "Porcentaje de vida útil estimada consumida.", fields: [{ key: "percentage_used", label: "A partir de", unit: "%" }] },
  TEMP_HIGH: { title: "Temperatura alta", help: "Temperatura en la lectura.", fields: [{ key: "celsius", label: "A partir de", unit: "°C" }] },
};

/** «hace 5 min», «hace 3 h»… */
export function timeAgo(iso: string): string {
  const s = Math.max(0, (Date.now() - Date.parse(iso)) / 1000);
  if (s < 90) return "hace un momento";
  if (s < 3600) return `hace ${Math.round(s / 60)} min`;
  if (s < 86_400 * 2) return `hace ${Math.round(s / 3600)} h`;
  return `hace ${Math.round(s / 86_400)} días`;
}

export const txStatusLabel: Record<string, string> = {
  prepared: "Preparada", writing: "Escribiendo", committed: "Cabecera actualizada", finalized: "Aplicada sin verificar",
  verified: "Aplicada y verificada", rolled_back: "Revertida", interrupted: "Interrumpida", failed: "Fallida",
};
