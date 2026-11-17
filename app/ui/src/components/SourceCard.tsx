import { Disc3, HardDrive, TriangleAlert, Usb } from "lucide-react";
import type { DiskOverview } from "../api";
import { busLabel } from "../lib/labels";
import { formatBytes, formatNumber, usedRatio } from "../lib/format";
import { EncryptionBadge, FsBadge, HealthBadge } from "./Badges";

export function SourceCard({ item }: { item: DiskOverview }) {
  const { disk, sources, volumes, latestSmart, openAlerts } = item;
  const source = sources[0];
  const Icon = disk ? (disk.bus === "usb" ? Usb : HardDrive) : Disc3;
  const title = disk?.model ?? source?.label ?? "Imagen";
  const size = disk?.capacityBytes ?? source?.sizeBytes ?? 0;

  return (
    <article className="card card--interactive source-card">
      <header className="source-card__head">
        <span className="source-card__icon"><Icon aria-hidden /></span>
        <div className="grow">
          <h3 className="truncate" title={title}>{title}</h3>
          <p className="subtle truncate">
            {disk ? `${busLabel[disk.bus]} · ${formatBytes(size)}` : `Imagen ${source?.kind === "udif" ? "DMG" : "raw"} · ${formatBytes(size)}`}
            {disk?.isBootDisk && " · Disco de arranque"}
          </p>
        </div>
        {disk && <HealthBadge overall={disk.smartSupported ? latestSmart?.overall ?? "unknown" : "unknown"} />}
      </header>

      {openAlerts > 0 && (
        <p className="source-card__alert"><TriangleAlert aria-hidden /> {openAlerts} alerta{openAlerts > 1 ? "s" : ""} sin revisar</p>
      )}

      <ul className="source-card__volumes">
        {volumes.map((v) => {
          const ratio = usedRatio(v.totalBlocks, v.freeBlocks);
          return (
            <li key={v.id}>
              <div className="row">
                <strong className="grow truncate">{v.name}</strong>
                <FsBadge fs={v.fsType} />
                <EncryptionBadge layer={v.encryption} />
              </div>
              <div className={"meter" + (ratio > 0.9 ? " meter--crit" : ratio > 0.8 ? " meter--warn" : "")}>
                <span style={{ width: `${(ratio * 100).toFixed(1)}%` }} />
              </div>
              <p className="subtle">
                {formatBytes((v.totalBlocks - v.freeBlocks) * v.blockSize)} usados de {formatBytes(v.totalBlocks * v.blockSize)}
                {v.fileCount != null && ` · ${formatNumber(v.fileCount)} archivos`}
                {v.isMounted && " · montado"}
              </p>
            </li>
          );
        })}
        {volumes.length === 0 && <li className="subtle">Sin volúmenes reconocidos</li>}
      </ul>
    </article>
  );
}
