import { FolderOpen, Info, Undo2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, type DiskOverview } from "../api";
import { SourceCard } from "../components/SourceCard";
import { StatTile } from "../components/StatTile";

export function Home() {
  const [items, setItems] = useState<DiskOverview[] | null>(null);
  useEffect(() => {
    api.overview().then(setItems);
  }, []);

  const physical = items?.filter((i) => i.disk) ?? [];
  const images = items?.filter((i) => !i.disk) ?? [];
  const volumes = items?.reduce((n, i) => n + i.volumes.length, 0) ?? 0;
  const alerts = items?.reduce((n, i) => n + i.openAlerts, 0) ?? 0;

  return (
    <>
      <div className="page-header">
        <div>
          <h1>Discos e imágenes</h1>
          <p>Elige un volumen para verificarlo. Todo se analiza en solo lectura hasta que confirmes un cambio.</p>
        </div>
        <button className="btn btn--primary"><FolderOpen aria-hidden /> Abrir imagen…</button>
      </div>

      <div className="callout callout--warn">
        <Undo2 aria-hidden />
        <div>
          <strong>Hay una escritura directa interrumpida en «Archivo Fotos».</strong>
          Se escribieron 1 412 de 2 980 bloques antes de que se desconectara la caja USB.{" "}
          <Link to="/transacciones">Revisar y revertir</Link>
        </div>
      </div>

      <div className="stat-grid">
        <StatTile label="Discos físicos" value={physical.length} hint="Incluye el disco de arranque" />
        <StatTile label="Volúmenes detectados" value={volumes} hint="HFS+, APFS, NTFS y ext4" />
        <StatTile label="Imágenes abiertas" value={images.length} />
        <StatTile label="Alertas abiertas" value={alerts} hint={alerts ? "Revisa la salud de los discos" : "Todo en orden"} />
      </div>

      <section className="stack">
        <h2 className="section-title">Discos físicos</h2>
        <div className="card-grid">
          {items === null ? <SkeletonCards /> : physical.map((i) => <SourceCard key={i.disk!.id} item={i} />)}
        </div>
      </section>

      <section className="stack">
        <h2 className="section-title">Imágenes de disco</h2>
        <div className="card-grid">
          {items === null ? <SkeletonCards n={2} /> : images.map((i) => <SourceCard key={i.sources[0]!.id} item={i} />)}
        </div>
      </section>

      <div className="callout callout--info">
        <Info aria-hidden />
        <div>
          <strong>El disco de arranque no se puede reparar mientras está en uso.</strong>
          Crea un medio de arranque externo para trabajar sobre él con el sistema detenido.
        </div>
      </div>
    </>
  );
}

function SkeletonCards({ n = 3 }: { n?: number }) {
  return <>{Array.from({ length: n }, (_, i) => <div key={i} className="card skeleton" style={{ height: 180 }} />)}</>;
}
