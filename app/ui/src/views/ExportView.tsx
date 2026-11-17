import { ArrowLeft, CircleCheck, Download, FolderOpen, ShieldAlert, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { backend } from "../api";
import { SeverityBadge } from "../components/Badges";
import { ErrorCallout, Loading } from "../components/Feedback";
import { formatBytes } from "../lib/format";
import { errorText, useAsync } from "../lib/useAsync";
import { useStartJob } from "../lib/useStartJob";
import { useJobs } from "../store/jobs";
import type { ExportResultDto } from "../types/api";

export function ExportView() {
  const id = Number(useParams().id);
  const [params] = useSearchParams();
  const jobId = Number(params.get("trabajo") ?? NaN);
  const navigate = useNavigate();
  const rebuild = useAsync(() => backend.getRebuild(id), [id]);
  const job = useStartJob();
  const result = useJobs((s) => (Number.isFinite(jobId) ? (s.jobs[jobId]?.result as unknown as ExportResultDto | undefined) : undefined));
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (rebuild.data && rebuild.data.status !== "done") {
      backend.suggestExportPath(id).then(setPath).catch(() => undefined);
    }
  }, [rebuild.data, id]);

  if (rebuild.error) return <ErrorCallout message={rebuild.error} />;
  if (!rebuild.data) return <Loading />;
  const r = rebuild.data;

  const choose = async () => {
    try {
      const p = await backend.pickSavePath(path);
      if (p) setPath(p);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const openResult = async () => {
    if (!r.outputPath) return;
    try {
      const src = await backend.openImage(r.outputPath);
      const v = src.volumes[0];
      navigate(v ? `/volumen/${v.id}` : "/");
    } catch (e) {
      setError(errorText(e));
    }
  };

  if (r.status === "done" && r.outputPath) {
    const remaining = result?.remainingFindings ?? [];
    return (
      <>
        <Link to={`/reconstruccion/${id}`} className="back-link"><ArrowLeft aria-hidden /> Reconstrucción</Link>
        <div className="page-header">
          <div>
            <h1>Imagen reparada</h1>
            <p>La imagen original no se ha modificado.</p>
          </div>
        </div>
        <section className={"card card--pad result-card " + (r.verified ? "result-card--ok" : "result-card--warn")}>
          {r.verified ? <ShieldCheck aria-hidden /> : <ShieldAlert aria-hidden />}
          <div className="grow stack">
            <h2>{r.verified ? "Verificada: el volumen reparado no tiene problemas" : "Exportada con problemas pendientes"}</h2>
            <p className="mono path">{r.outputPath}</p>
            {result && <p className="subtle">{formatBytes(result.bytesCopied)} copiados · {formatBytes(result.bytesWritten)} de estructuras nuevas</p>}
            <div className="row row--wrap">
              <button className="btn btn--primary" onClick={openResult}><FolderOpen aria-hidden /> Abrir la imagen reparada</button>
              <Link className="btn btn--secondary" to={`/sesion/${r.sessionId}`}>Volver a los resultados</Link>
            </div>
          </div>
        </section>
        {remaining.length > 0 && (
          <section className="card">
            <div className="card__header"><h3>Problemas que quedan</h3></div>
            <ul className="list">
              {remaining.map((f) => (
                <li key={f.id} className="list__row"><SeverityBadge severity={f.severity} /><span className="grow">{f.message}</span><span className="badge badge--outline badge--mono">{f.code}</span></li>
              ))}
            </ul>
          </section>
        )}
        {error && <ErrorCallout message={error} />}
      </>
    );
  }

  return (
    <>
      <Link to={`/reconstruccion/${id}`} className="back-link"><ArrowLeft aria-hidden /> Reconstrucción</Link>
      <div className="page-header">
        <div>
          <h1>Exportar imagen reparada</h1>
          <p>Se crea una copia de la imagen con el directorio reconstruido y después se verifica automáticamente.</p>
        </div>
      </div>

      <section className="card card--pad stack">
        <ul className="checklist">
          <li><CircleCheck aria-hidden /> La imagen original <strong>no se modifica</strong>: {r.sourcePath}</li>
          <li><CircleCheck aria-hidden /> Nunca se sobrescribe un archivo existente.</li>
          <li><CircleCheck aria-hidden /> Al terminar, la imagen nueva se analiza de nuevo para confirmar que quedó sana.</li>
          <li><CircleCheck aria-hidden /> Hace falta espacio para una copia completa (los tramos vacíos no ocupan disco).</li>
        </ul>
        <label className="field">
          <span>Guardar como</span>
          <div className="row">
            <input className="input grow mono" value={path} onChange={(e) => setPath(e.target.value)} aria-label="Ruta de la imagen reparada" />
            <button className="btn btn--secondary" onClick={choose}><FolderOpen aria-hidden /> Elegir…</button>
          </div>
        </label>
        <div className="row">
          <button className="btn btn--primary" disabled={!path.trim() || job.starting}
            onClick={() => job.start("export", () => backend.startExport(id, path.trim()), { rebuildId: id })}>
            <Download aria-hidden /> {job.starting ? "Iniciando…" : "Exportar y verificar"}
          </button>
        </div>
      </section>
      {(job.error || error) && <ErrorCallout title="No se pudo exportar" message={job.error ?? error ?? ""} />}
    </>
  );
}
