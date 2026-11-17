const nf = new Intl.NumberFormat("es-ES");
const df = new Intl.DateTimeFormat("es-ES", { dateStyle: "medium", timeStyle: "short" });

/** Bytes en unidades decimales (como los fabricantes de discos). */
export function formatBytes(bytes: number, digits = 1): string {
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let v = bytes;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v.toLocaleString("es-ES", { maximumFractionDigits: i === 0 ? 0 : digits })} ${units[i]}`;
}

export const formatNumber = (n: number) => nf.format(n);
export const formatDate = (iso: string) => df.format(new Date(iso));

export function usedRatio(totalBlocks: number, freeBlocks: number): number {
  return totalBlocks > 0 ? (totalBlocks - freeBlocks) / totalBlocks : 0;
}
