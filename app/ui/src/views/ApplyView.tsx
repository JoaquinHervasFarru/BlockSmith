import { ArrowLeft, CircleCheck, ShieldAlert, ShieldCheck, Undo2, Wrench } from "lucide-react";
import { useState } from "react";
import { Link, useParams, useSearchParams } from "react-router-dom";
import { backend } from "../api";
import { SeverityBadge } from "../components/Badges";
import { ErrorCallout, Loading } from "../components/Feedback";
import { PreflightChecklist } from "../components/PreflightChecklist";
import { useAsync } from "../lib/useAsync";
import { useStartJob } from "../lib/useStartJob";
import { useJobs } from "../store/jobs";
import type { RepairResultDto } from "../types/api";

export function ApplyView() {
  const id = Number(useParams().id);
  const [params] = useSearchParams();
  const jobId = Number(params.get("trabajo") ?? NaN);
  const result = useJobs((s) => (Number.isFinite(jobId) ? (s.jobs[jobId]?.result as unknown as RepairResultDto | undefined) : undefined));
  const pre = useAsync(() => backend.preflight(id), [id, jobId]);
  const rebuild = useAsync(() => backend.getRebuild(id), [id, jobId]);
  const job = useStartJob();
  const [confirm, setConfirm] = useState("");
  const [understood, setUnderstood] = useState(false);

  if (pre.error) return <ErrorCallout message={pre.error} />;
  if (!pre.data || !rebuild.data) return <Loading />;
  const p = pre.data;
  const r = rebuild.data;
  const name = p.volumeName ?? "";

  if (result || (r.status === "done" && r.target === "in_place")) {
    const verified = result?.verified ?? r.verified;
    return (
      <>
        <Link to={`/reconstruccion/${id}`} className="back-link"><ArrowLeft aria-hidden /> Reconstrucción</Link>
        <div className="page-header"><div><h1>Reparación en el origen</h1><p>{p.target}</p></div></div>
        <section className={"card card--pad result-card " + (verified ? "result-card--ok" : "result-card--warn")}>
          {verified ? <ShieldCheck aria-hidden /> : <ShieldAlert aria-hidden />}
          <div className="grow stack">
            <h2>{verified ? "Reparado y verificado: el volumen no tiene problemas" : "Aplicado, pero quedan problemas"}</h2>
            <p className="muted">La copia de deshacer permite volver al estado anterior desde «Transacciones» mientras no se modifique el volumen.</p>
            {result && <p className="mono path">{result.undoLogPath}</p>}
            <div className="row row--wrap">
              <Link className="btn btn--primary" to={`/sesion/${r.sessionId}`}>Volver a los resultados</Link>
              <Link className="btn btn--secondary" to="/transacciones"><Undo2 aria-hidden /> Transacciones</Link>
            </div>
          </div>
        </section>
        {result && result.remainingFindings.length > 0 && (
          <section className="card">
            <div className="card__header"><h3>Problemas que quedan</h3></div>
            <ul className="list">
              {result.remainingFindings.map((f) => (
                <li key={f.id} className="list__row"><SeverityBadge severity={f.severity} /><span className="grow">{f.message}</span></li>
              ))}
            </ul>
          </section>
        )}
      </>
    );
  }

  const ready = p.canProceed && confirm.trim() === name.trim() && understood;
  return (
    <>
      <Link to={`/reconstruccion/${id}`} className="back-link"><ArrowLeft aria-hidden /> Reconstrucción</Link>
      <div className="page-header">
        <div>
          <h1>Reparar en el origen</h1>
          <p>Escribe el directorio reconstruido directamente en <span className="mono">{p.target}</span>, de forma transaccional.</p>
        </div>
      </div>

      <section className="card">
        <div className="card__header"><h3>1 · Comprobaciones previas</h3>
          {p.mode === "helper" && <span className="badge badge--warn">Se pedirán permisos de administrador</span>}
          {p.mode === "in_process" && <span className="badge badge--info">Sin permisos especiales</span>}
        </div>
        <div className="card__body"><PreflightChecklist checks={p.checks} /></div>
      </section>

      <section className="card card--pad stack">
        <h3>2 · Cómo se protege el volumen</h3>
        <ul className="checklist">
          <li><CircleCheck aria-hidden /> Antes de escribir se guarda una copia de cada bloque que se va a modificar en <span className="mono">{p.undoDir}</span>.</li>
          <li><CircleCheck aria-hidden /> Las estructuras nuevas se escriben en espacio libre; la cabecera del volumen se actualiza al final. Un corte antes deja la versión anterior intacta.</li>
          <li><CircleCheck aria-hidden /> Si algo se interrumpe, al volver a abrir BlockSmith podrás revertir o completar la transacción.</li>
          <li><CircleCheck aria-hidden /> Al terminar, el volumen se analiza de nuevo para confirmar que quedó sano.</li>
        </ul>
      </section>

      <section className="card card--pad stack">
        <h3>3 · Confirmación</h3>
        <label className="field">
          <span>Escribe el nombre del volumen para confirmar: <strong className="mono">{name}</strong></span>
          <input className="input mono" value={confirm} onChange={(e) => setConfirm(e.target.value)} disabled={!p.canProceed}
            aria-label="Nombre del volumen para confirmar" autoComplete="off" spellCheck={false} />
        </label>
        <label className="check">
          <input type="checkbox" checked={understood} onChange={(e) => setUnderstood(e.target.checked)} disabled={!p.canProceed} />
          <span>Entiendo que se modificará el origen. Para conservarlo sin cambios, exporta mejor una imagen nueva.</span>
        </label>
        <div className="row row--wrap">
          <button className="btn btn--write" disabled={!ready || job.starting}
            onClick={() => job.start("repair", () => backend.startRepair(id, confirm), { rebuildId: id })}>
            <Wrench aria-hidden /> Reparar en el origen
          </button>
          <Link className="btn btn--secondary" to={`/reconstruccion/${id}/exportar`}>Exportar una imagen nueva</Link>
        </div>
        {!p.canProceed && <p className="subtle">Resuelve los puntos marcados en rojo para continuar.</p>}
      </section>
      {job.error && <ErrorCallout title="No se pudo iniciar la reparación" message={job.error} />}
    </>
  );
}
