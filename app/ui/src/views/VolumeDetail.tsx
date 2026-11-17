import { ArrowLeft, FileSearch, Play } from "lucide-react";
import { Link, useParams } from "react-router-dom";
import { backend } from "../api";
import { CapabilityChips, FsBadge, SeverityCountBadges } from "../components/Badges";
import { ErrorCallout, Loading } from "../components/Feedback";
import { formatBytes, formatDate, formatNumber } from "../lib/format";
import { fsName, sessionStatusLabel } from "../lib/labels";
import { useAsync } from "../lib/useAsync";
import { useStartJob } from "../lib/useStartJob";

export function VolumeDetail() {
  const id = Number(useParams().id);
  const vol = useAsync(() => backend.getVolume(id), [id]);
  const job = useStartJob();
  if (vol.error) return <ErrorCallout message={vol.error} />;
  if (!vol.data) return <Loading />;
  const v = vol.data;
  const caps = v.capabilities;
  const rows: [string, string][] = [
    ["Sistema de archivos", `${fsName(v.fsType)}${v.caseSensitive ? " (distingue mayúsculas)" : ""}`],
    ["Tamaño", v.totalBlocks ? formatBytes(v.totalBlocks * v.blockSize) : "—"],
    ["Espacio libre", v.totalBlocks ? formatBytes(v.freeBlocks * v.blockSize) : "—"],
    ["Tamaño de bloque", v.blockSize ? `${formatNumber(v.blockSize)} B` : "—"],
    ["Archivos", v.fileCount != null ? formatNumber(v.fileCount) : "—"],
    ["Carpetas", v.folderCount != null ? formatNumber(v.folderCount) : "—"],
    ["Journal", v.journaled ? (v.journalState === "pending" ? "Con transacciones pendientes" : "Activo") : "No"],
    ["Posición en la imagen", `${formatBytes(v.offsetBytes)}${v.partitionIndex != null ? ` · partición ${v.partitionIndex}` : ""}`],
  ];
  return (
    <>
      <Link to="/" className="back-link"><ArrowLeft aria-hidden /> Imágenes</Link>
      <div className="page-header">
        <div>
          <div className="row"><h1>{v.name ?? "Volumen sin nombre"}</h1><FsBadge fs={v.fsType} /></div>
          <p>Lectura de la cabecera del volumen. El análisis completo recorre el directorio sin modificar nada.</p>
        </div>
        <button className="btn btn--primary" disabled={caps.scan !== "yes" || job.starting}
          onClick={() => job.start("scan", () => backend.startScan(v.id), { volumeId: v.id })}>
          <Play aria-hidden /> {job.starting ? "Iniciando…" : "Analizar volumen"}
        </button>
      </div>
      {job.error && <ErrorCallout title="No se pudo iniciar el análisis" message={job.error} />}
      {v.journalState === "pending" && (
        <ErrorCallout title="Journal con transacciones pendientes"
          message="Monta el volumen una vez en macOS para que se apliquen antes de reconstruirlo; el análisis puede mostrar problemas que desaparecerían al aplicarlas." />
      )}

      <div className="split">
        <section className="card">
          <div className="card__header"><h3>Cabecera del volumen</h3></div>
          <dl className="kv">
            {rows.map(([k, val]) => (<div key={k}><dt>{k}</dt><dd>{val}</dd></div>))}
          </dl>
        </section>
        <section className="card">
          <div className="card__header"><h3>Qué se puede hacer</h3></div>
          <div className="card__body stack">
            <CapabilityChips caps={caps} />
            {caps.note && <p className="muted">{caps.note}</p>}
            {v.lastSession && (
              <Link to={`/sesion/${v.lastSession.id}`} className="list__row card--inset">
                <FileSearch aria-hidden />
                <span className="grow">
                  <strong>Último análisis</strong>
                  <span className="subtle"> · {formatDate(v.lastSession.startedAt)} · {sessionStatusLabel[v.lastSession.status] ?? v.lastSession.status}</span>
                </span>
                <SeverityCountBadges counts={v.lastSession.counts} />
              </Link>
            )}
          </div>
        </section>
      </div>
    </>
  );
}
