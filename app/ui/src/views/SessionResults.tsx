import { ArrowLeft, FileCode, FileText, GitCompareArrows, Hammer, RotateCw } from "lucide-react";
import { useState } from "react";
import { Link, useParams } from "react-router-dom";
import { backend } from "../api";
import { FsBadge } from "../components/Badges";
import { ErrorCallout, Loading } from "../components/Feedback";
import { FindingsTable } from "../components/FindingsTable";
import { StatTile } from "../components/StatTile";
import { formatDate, formatNumber } from "../lib/format";
import { sessionStatusLabel } from "../lib/labels";
import { errorText, useAsync } from "../lib/useAsync";
import { useStartJob } from "../lib/useStartJob";

export function SessionResults() {
  const id = Number(useParams().id);
  const session = useAsync(() => backend.getSession(id), [id]);
  const findings = useAsync(() => backend.getFindings(id), [id]);
  const job = useStartJob();
  const [report, setReport] = useState<string | null>(null);
  const [reportError, setReportError] = useState<string | null>(null);

  if (session.error) return <ErrorCallout message={session.error} />;
  if (!session.data) return <Loading />;
  const s = session.data;
  const problems = s.counts.critical + s.counts.minor;
  const finished = s.status !== "scanning" && s.status !== "failed" && s.status !== "cancelled";

  const writeReport = async (format: "html" | "json") => {
    setReport(null);
    setReportError(null);
    try {
      const r = await backend.writeReport(id, format);
      setReport(r.path);
    } catch (e) {
      setReportError(errorText(e));
    }
  };

  return (
    <>
      <Link to={`/volumen/${s.volumeId}`} className="back-link"><ArrowLeft aria-hidden /> Volumen</Link>
      <div className="page-header">
        <div>
          <div className="row"><h1>{s.volumeName ?? "Volumen"} · resultados</h1><FsBadge fs={s.fsType} /></div>
          <p>{s.sourceLabel ?? s.sourcePath} · {formatDate(s.startedAt)} · {sessionStatusLabel[s.status] ?? s.status}</p>
        </div>
        <div className="row row--wrap">
          <button className="btn btn--secondary" onClick={() => writeReport("html")}><FileText aria-hidden /> Informe HTML</button>
          <button className="btn btn--secondary" onClick={() => writeReport("json")}><FileCode aria-hidden /> JSON</button>
          {s.latestRebuildId ? (
            <Link className="btn btn--primary" to={`/reconstruccion/${s.latestRebuildId}`}><GitCompareArrows aria-hidden /> Ver reconstrucción</Link>
          ) : (
            finished && (
              <button className="btn btn--primary" disabled={job.starting}
                onClick={() => job.start("plan", () => backend.startPlan(id), { sessionId: id })}>
                <Hammer aria-hidden /> {problems > 0 ? "Planificar reconstrucción" : "Optimizar directorio"}
              </button>
            )
          )}
        </div>
      </div>

      {s.error && <ErrorCallout title="El análisis no terminó" message={s.error}
        action={<button className="btn btn--secondary btn--sm" onClick={() => job.start("scan", () => backend.startScan(s.volumeId))}><RotateCw aria-hidden /> Repetir análisis</button>} />}
      {job.error && <ErrorCallout title="No se pudo iniciar" message={job.error} />}
      {report && <div className="callout callout--info"><FileText aria-hidden /><div><strong>Informe guardado</strong><span className="mono">{report}</span></div></div>}
      {reportError && <ErrorCallout title="No se pudo guardar el informe" message={reportError} />}

      {finished && (
        <div className="stat-grid">
          <StatTile label="Nodos del catálogo" value={s.catalogNodesTotal != null ? formatNumber(s.catalogNodesTotal) : "—"} />
          <StatTile label="Nodos dañados" value={s.catalogNodesDamaged ?? "—"} hint={s.catalogNodesDamaged ? "Sus registros se intentan rescatar" : undefined} />
          <StatTile label="Espacio sin usar en el directorio" value={s.fragmentationPct != null ? `${s.fragmentationPct.toLocaleString("es-ES", { maximumFractionDigits: 1 })} %` : "—"}
            hint="La reconstrucción lo compacta" />
          <StatTile label="Archivos / carpetas" value={`${formatNumber(s.fileCount ?? 0)} / ${formatNumber(s.folderCount ?? 0)}`} />
        </div>
      )}

      {problems === 0 && finished && !s.error && (
        <div className="callout callout--info">
          <Hammer aria-hidden />
          <div><strong>El directorio está en buen estado.</strong>Aun así, puedes reconstruirlo para compactarlo y ordenar sus nodos.</div>
        </div>
      )}

      {findings.data ? <FindingsTable findings={findings.data} /> : findings.error ? <ErrorCallout message={findings.error} /> : <Loading />}
    </>
  );
}
