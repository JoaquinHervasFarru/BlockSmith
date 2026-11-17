import { ArrowLeft, CircleCheck, CircleX, Square } from "lucide-react";
import { useEffect, useMemo } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { backend } from "../api";
import { ErrorCallout, ProgressBar } from "../components/Feedback";
import { PhaseStepper } from "../components/PhaseStepper";
import { jobKindLabel } from "../lib/labels";
import { useJobs } from "../store/jobs";
import type { JobKind } from "../types/api";

const PHASES: Record<JobKind, string[]> = {
  scan: ["Cabecera del volumen", "Árbol de extents", "Catálogo: estructura", "Catálogo: coherencia", "Extents y bitmap de asignación"],
  plan: ["Cabecera del volumen", "Árbol de extents", "Catálogo: estructura", "Catálogo: coherencia", "Extents y bitmap de asignación", "Plan de reconstrucción"],
  export: ["Copiando la imagen", "Aplicando la reconstrucción", "Verificando la imagen reparada"],
  health: ["Leyendo la salud de los discos"],
  repair: ["Comprobaciones previas", "Recalculando el plan sobre el contenido actual", "Guardando la copia de deshacer",
    "Escribiendo el directorio nuevo", "Actualizando la cabecera del volumen", "Liberando los bloques antiguos", "Verificando el volumen"],
  rollback: ["Revirtiendo la transacción"],
  complete: ["Completando la transacción", "Verificando el volumen"],
};

/** Trabajos cuyas fases intermedias del motor (p. ej. el re-análisis) no se muestran como pasos propios. */
const FIXED_STEPS = new Set<JobKind>(["repair", "rollback", "complete"]);

/** Adónde ir al terminar cada tipo de trabajo. */
function destination(kind: JobKind, result: Record<string, unknown> | null, jobId: number): string | null {
  if (!result) return null;
  if (kind === "scan") return `/sesion/${result.sessionId}`;
  if (kind === "plan") return `/reconstruccion/${result.rebuildId}`;
  if (kind === "health") return "/salud";
  if (kind === "repair") return `/reconstruccion/${result.rebuildId}/aplicar?trabajo=${jobId}`;
  if (kind === "rollback" || kind === "complete") return "/transacciones";
  return `/reconstruccion/${result.rebuildId}/exportar?trabajo=${jobId}`;
}

export function Job() {
  const id = Number(useParams().id);
  const navigate = useNavigate();
  const job = useJobs((s) => s.jobs[id]);

  useEffect(() => {
    if (job?.status !== "ok") return;
    const to = destination(job.kind, job.result, id);
    if (to) {
      const t = setTimeout(() => navigate(to, { replace: true }), 600);
      return () => clearTimeout(t);
    }
  }, [job?.status, job?.kind, job?.result, id, navigate]);

  const phases = useMemo(() => {
    if (!job) return [];
    const base = PHASES[job.kind];
    if (FIXED_STEPS.has(job.kind)) return base;
    // Fases que el motor emita y no estén en la lista conocida se añaden al final.
    const extra = job.log.map((l) => l.phase).filter((p) => p && !base.includes(p));
    return [...base, ...new Set(extra)];
  }, [job]);
  // Paso resaltado: la última fase conocida (las intermedias no cambian el paso).
  const currentStep = useMemo(() => {
    if (!job) return "";
    return [...job.log].reverse().find((l) => phases.includes(l.phase))?.phase ?? job.phase;
  }, [job, phases]);

  if (!job) {
    return (
      <ErrorCallout title="Trabajo no encontrado" message="Este trabajo no está activo en esta ventana. Puede que la aplicación se haya reiniciado."
        action={<Link to="/historial" className="btn btn--secondary btn--sm">Ir al historial</Link>} />
    );
  }

  const running = job.status === "running";
  const fraction = job.total > 0 ? job.done / job.total : null;
  return (
    <>
      <Link to="/" className="back-link"><ArrowLeft aria-hidden /> Imágenes</Link>
      <div className="page-header">
        <div>
          <h1>{jobKindLabel[job.kind]}</h1>
          <p>{running ? "Puedes seguir usando la aplicación; el trabajo continúa en segundo plano." : "Trabajo terminado."}</p>
        </div>
        {running && (
          <button className="btn btn--secondary" onClick={() => backend.cancelJob(id)}>
            <Square aria-hidden /> Cancelar
          </button>
        )}
      </div>

      <section className="card card--pad stack">
        <div className="row">
          <strong className="grow">{running ? (job.phase || "Preparando…") : job.status === "ok" ? "Completado" : job.status === "cancelled" ? "Cancelado" : "Error"}</strong>
          {running && fraction !== null && <span className="subtle">{Math.round(fraction * 100)} %</span>}
          {job.status === "ok" && <CircleCheck className="ok-icon" aria-hidden />}
          {job.status === "failed" && <CircleX className="crit-icon" aria-hidden />}
        </div>
        <ProgressBar value={job.status === "ok" ? 1 : running ? fraction : 0} label="Progreso" />
        <PhaseStepper phases={phases} current={currentStep} finished={job.status === "ok"} />
        {running && job.kind === "repair" && (
          <p className="subtle">Cancelar solo tiene efecto antes de actualizar la cabecera del volumen; en ese caso se revierte todo lo escrito.</p>
        )}
      </section>

      {job.status === "failed" && <ErrorCallout title="El trabajo no pudo completarse" message={job.error ?? "Error desconocido"}
        action={<button className="btn btn--secondary btn--sm" onClick={() => navigate(-1)}>Volver</button>} />}
      {job.status === "cancelled" && (
        <div className="callout callout--warn"><Square aria-hidden /><div><strong>Cancelado</strong>No se ha modificado nada. Puedes repetirlo cuando quieras.</div></div>
      )}
    </>
  );
}
