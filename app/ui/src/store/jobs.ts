import { create } from "zustand";
import { backend } from "../api";
import type { JobKind } from "../types/api";

export type JobStatus = "running" | "ok" | "failed" | "cancelled";

export interface JobState {
  id: number;
  kind: JobKind;
  phase: string;
  done: number;
  total: number;
  /** Fases vistas, con la hora en que empezaron. */
  log: { phase: string; at: number }[];
  status: JobStatus;
  result: Record<string, unknown> | null;
  error: string | null;
  startedAt: number;
  /** Contexto para volver a la vista de origen (p. ej. el id de reconstrucción). */
  context?: Record<string, number>;
}

interface JobsStore {
  jobs: Record<number, JobState>;
  track: (id: number, kind: JobKind, context?: Record<string, number>) => void;
}

const blank = (id: number, kind: JobKind): JobState => ({
  id, kind, phase: "", done: 0, total: 0, log: [], status: "running", result: null, error: null, startedAt: Date.now(),
});

export const useJobs = create<JobsStore>((set) => ({
  jobs: {},
  track: (id, kind, context) =>
    set((s) => ({ jobs: { ...s.jobs, [id]: { ...(s.jobs[id] ?? blank(id, kind)), kind, context } } })),
}));

let installed = false;

/** Suscribe la tienda a los eventos de trabajos del backend (una sola vez). */
export function installJobListeners() {
  if (installed) return;
  installed = true;
  void backend.onJobProgress((e) =>
    useJobs.setState((s) => {
      const j = s.jobs[e.jobId] ?? blank(e.jobId, e.kind);
      const log = j.log.at(-1)?.phase === e.phase ? j.log : [...j.log, { phase: e.phase, at: Date.now() }];
      return { jobs: { ...s.jobs, [e.jobId]: { ...j, phase: e.phase, done: e.done, total: e.total, log } } };
    }),
  );
  void backend.onJobFinished((e) =>
    useJobs.setState((s) => {
      const j = s.jobs[e.jobId] ?? blank(e.jobId, e.kind);
      const status: JobStatus = e.ok ? "ok" : e.cancelled ? "cancelled" : "failed";
      return { jobs: { ...s.jobs, [e.jobId]: { ...j, status, result: e.result, error: e.error } } };
    }),
  );
}
