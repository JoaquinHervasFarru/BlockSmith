import { Disc3, FolderOpen, HardDrive, History, Info, Trash2 } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { backend } from "../api";
import { FsBadge, SeverityCountBadges } from "../components/Badges";
import { EmptyState, ErrorCallout, Loading } from "../components/Feedback";
import { StatTile } from "../components/StatTile";
import { formatBytes, formatDate, formatNumber, usedRatio } from "../lib/format";
import { sessionStatusLabel } from "../lib/labels";
import { errorText, useAsync } from "../lib/useAsync";
import type { SourceDto } from "../types/api";

export function Home() {
  const navigate = useNavigate();
  const sources = useAsync(() => backend.listSources(), []);
  const sessions = useAsync(() => backend.listSessions(), []);
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const openImage = async () => {
    setError(null);
    try {
      const path = await backend.pickImage();
      if (!path) return;
      setOpening(true);
      const src = await backend.openImage(path);
      sources.reload();
      const first = src.volumes[0];
      if (src.volumes.length === 1 && first) navigate(`/volumen/${first.id}`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setOpening(false);
    }
  };

  const remove = async (s: SourceDto) => {
    if (!window.confirm(`¿Quitar «${s.label ?? s.path}» de la biblioteca? La imagen no se borra del disco.`)) return;
    try {
      await backend.removeSource(s.id);
      sources.reload();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const list = sources.data ?? [];
  const volumes = list.reduce((n, s) => n + s.volumes.length, 0);
  const sessionList = sessions.data ?? [];
  const issues = sessionList.filter((s) => s.status === "scanned" || s.status === "rebuilt").length;

  return (
    <>
      <div className="page-header">
        <div>
          <h1>Imágenes de disco</h1>
          <p>Abre una imagen (.dmg o raw) para verificarla. Todo se analiza en solo lectura y la imagen original nunca se modifica.</p>
        </div>
        <button className="btn btn--primary" onClick={openImage} disabled={opening}>
          <FolderOpen aria-hidden /> {opening ? "Abriendo…" : "Abrir imagen…"}
        </button>
      </div>

      {error && <ErrorCallout title="No se pudo abrir la imagen" message={error} />}

      <div className="stat-grid">
        <StatTile label="Imágenes en la biblioteca" value={list.length} />
        <StatTile label="Volúmenes detectados" value={volumes} hint="HFS+ y HFSX se pueden analizar" />
        <StatTile label="Análisis realizados" value={sessionList.length} />
        <StatTile label="Pendientes de reparar" value={issues} hint={issues ? "Revisa el historial" : "Nada pendiente"} />
      </div>

      <section className="stack">
        <h2 className="section-title">Biblioteca</h2>
        {sources.loading && !sources.data ? <Loading /> : sources.error ? <ErrorCallout message={sources.error} /> : list.length === 0 ? (
          <EmptyState icon={<Disc3 aria-hidden />} title="Aún no has abierto ninguna imagen">
            Usa «Abrir imagen…» para añadir un archivo .dmg o una imagen raw de un disco.
          </EmptyState>
        ) : (
          <div className="card-grid">
            {list.map((s) => <SourceCard key={s.id} source={s} onRemove={() => remove(s)} />)}
          </div>
        )}
      </section>

      {sessionList.length > 0 && (
        <section className="stack">
          <div className="row">
            <h2 className="section-title grow">Análisis recientes</h2>
            <Link to="/historial" className="btn btn--ghost btn--sm"><History aria-hidden /> Ver historial</Link>
          </div>
          <div className="card">
            <ul className="list">
              {sessionList.slice(0, 5).map((s) => (
                <li key={s.id}>
                  <Link to={`/sesion/${s.id}`} className="list__row">
                    <span className="grow">
                      <strong>{s.volumeName ?? "Volumen sin nombre"}</strong>
                      <span className="subtle"> · {formatDate(s.startedAt)} · {sessionStatusLabel[s.status] ?? s.status}</span>
                    </span>
                    <SeverityCountBadges counts={s.counts} />
                  </Link>
                </li>
              ))}
            </ul>
          </div>
        </section>
      )}

      <div className="callout callout--info">
        <Info aria-hidden />
        <div>
          <strong>Discos físicos</strong>
          El análisis directo de discos conectados llegará junto con el servicio con permisos de administrador.
          Mientras tanto, crea una imagen del disco y ábrela aquí.
        </div>
      </div>
    </>
  );
}

function SourceCard({ source, onRemove }: { source: SourceDto; onRemove: () => void }) {
  const Icon = source.kind === "udif" ? Disc3 : HardDrive;
  return (
    <article className="card source-card">
      <header className="source-card__head">
        <span className="source-card__icon"><Icon aria-hidden /></span>
        <div className="grow">
          <h3 className="truncate" title={source.path}>{source.label ?? source.path}</h3>
          <p className="subtle truncate" title={source.path}>
            {source.kind === "udif" ? "DMG" : "Imagen raw"} · {formatBytes(source.sizeBytes)} · {source.path}
          </p>
        </div>
        <button className="btn btn--ghost btn--icon btn--sm" onClick={onRemove} title="Quitar de la biblioteca" aria-label="Quitar de la biblioteca">
          <Trash2 aria-hidden />
        </button>
      </header>
      {!source.exists && <p className="source-card__alert">La imagen ya no está en esa ruta.</p>}
      <ul className="source-card__volumes">
        {source.volumes.map((v) => {
          const ratio = usedRatio(v.totalBlocks, v.freeBlocks);
          return (
            <li key={v.id}>
              <Link to={`/volumen/${v.id}`} className="source-card__volume">
                <div className="row">
                  <strong className="grow truncate">{v.name ?? "Volumen sin nombre"}</strong>
                  <FsBadge fs={v.fsType} />
                </div>
                {v.totalBlocks > 0 && (
                  <div className={"meter" + (ratio > 0.9 ? " meter--crit" : ratio > 0.8 ? " meter--warn" : "")}>
                    <span style={{ width: `${(ratio * 100).toFixed(1)}%` }} />
                  </div>
                )}
                <p className="subtle">
                  {v.totalBlocks > 0 ? `${formatBytes((v.totalBlocks - v.freeBlocks) * v.blockSize)} usados de ${formatBytes(v.totalBlocks * v.blockSize)}` : "Tamaño no disponible"}
                  {v.fileCount != null && ` · ${formatNumber(v.fileCount)} archivos`}
                </p>
                {v.lastSession ? (
                  <div className="row row--wrap">
                    <span className="subtle">Último análisis · {sessionStatusLabel[v.lastSession.status] ?? v.lastSession.status}</span>
                    <SeverityCountBadges counts={v.lastSession.counts} />
                  </div>
                ) : v.capabilities.scan === "yes" ? (
                  <span className="subtle">Sin analizar</span>
                ) : (
                  <span className="subtle">{v.capabilities.note}</span>
                )}
              </Link>
            </li>
          );
        })}
        {source.volumes.length === 0 && <li className="subtle">No se reconoció ningún volumen en esta imagen.</li>}
      </ul>
    </article>
  );
}
