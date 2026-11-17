import { Disc3, FolderOpen, HardDrive, History, Info, Trash2, Undo2, X } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { backend } from "../api";
import { FsBadge, SeverityCountBadges } from "../components/Badges";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EmptyState, ErrorCallout, Loading } from "../components/Feedback";
import { StatTile } from "../components/StatTile";
import { formatBytes, formatDate, formatNumber, usedRatio } from "../lib/format";
import { sessionStatusLabel } from "../lib/labels";
import { errorText, useAsync } from "../lib/useAsync";
import type { SourceDto } from "../types/api";

export function Home() {
  const navigate = useNavigate();
  const sources = useAsync(() => backend.listSources(), []);
  const txs = useAsync(() => backend.listTransactions(), []);
  const [showDevices, setShowDevices] = useState(false);
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

  const [removing, setRemoving] = useState<SourceDto | null>(null);
  const remove = async (s: SourceDto) => {
    setRemoving(null);
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
        <div className="row row--wrap">
          <button className="btn btn--secondary" onClick={() => setShowDevices(true)}><HardDrive aria-hidden /> Abrir disco…</button>
          <button className="btn btn--primary" onClick={openImage} disabled={opening}>
            <FolderOpen aria-hidden /> {opening ? "Abriendo…" : "Abrir imagen…"}
          </button>
        </div>
      </div>

      {(txs.data ?? []).filter((t) => t.status === "interrupted").map((t) => (
        <div key={t.id} className="callout callout--warn" role="alert">
          <Undo2 aria-hidden />
          <div>
            <strong>Hay una escritura directa interrumpida en «{t.volumeName ?? t.devicePath}».</strong>
            Se escribieron {t.blocksWritten} de {t.blocksTotal} bloques. <Link to="/transacciones">Revisar: revertir o completar</Link>
          </div>
        </div>
      ))}
      {removing && (
        <ConfirmDialog title="¿Quitar de la biblioteca?" confirmLabel="Quitar" onConfirm={() => remove(removing)} onCancel={() => setRemoving(null)}>
          «{removing.label ?? removing.path}» desaparecerá de la lista y de su historial. La imagen no se borra del disco.
        </ConfirmDialog>
      )}
      {showDevices && (
        <DevicePicker
          onClose={() => setShowDevices(false)}
          onOpen={async (path) => {
            setShowDevices(false);
            setError(null);
            try {
              const src = await backend.openImage(path);
              sources.reload();
              const first = src.volumes[0];
              if (src.volumes.length === 1 && first) navigate(`/volumen/${first.id}`);
            } catch (e) {
              setError(errorText(e));
            }
          }}
        />
      )}

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
            {list.map((s) => <SourceCard key={s.id} source={s} onRemove={() => setRemoving(s)} />)}
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
          Con «Abrir disco…» puedes analizar un disco conectado si tienes permisos para leerlo. Lo más seguro sigue siendo
          crear antes una imagen del disco y trabajar sobre ella.
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

function DevicePicker({ onClose, onOpen }: { onClose: () => void; onOpen: (path: string) => void }) {
  const devices = useAsync(() => backend.listOpenDevices(), []);
  return (
    <div className="modal" role="dialog" aria-modal="true" aria-labelledby="device-title" onClick={onClose}>
      <div className="modal__panel card" onClick={(e) => e.stopPropagation()}>
        <div className="card__header">
          <h3 id="device-title">Abrir un disco</h3>
          <button className="btn btn--ghost btn--icon btn--sm" onClick={onClose} aria-label="Cerrar"><X aria-hidden /></button>
        </div>
        {devices.error ? <div className="card__body"><ErrorCallout message={devices.error} /></div> : !devices.data ? <Loading /> : (
          <ul className="list">
            {devices.data.map((d) => (
              <li key={d.path}>
                <button className="list__row list__button" disabled={!d.readable} onClick={() => onOpen(d.path)}>
                  <HardDrive aria-hidden />
                  <span className="grow">
                    <strong>{d.model ?? d.path}</strong>
                    <div className="subtle mono">{d.path} · {formatBytes(d.sizeBytes)}{d.isBoot ? " · disco de arranque" : ""}</div>
                  </span>
                  {!d.readable && <span className="badge">Sin permisos de lectura</span>}
                </button>
              </li>
            ))}
            {devices.data.length === 0 && <li className="card__body subtle">No se encontraron discos.</li>}
          </ul>
        )}
        <p className="card__body subtle">Leer un disco directamente suele requerir permisos de administrador. Si no los tienes, crea una imagen del disco con otra herramienta y ábrela con «Abrir imagen…».</p>
      </div>
    </div>
  );
}
