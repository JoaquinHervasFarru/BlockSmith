import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useJobs } from "../store/jobs";
import type { JobKind } from "../types/api";
import { errorText } from "./useAsync";

/** Lanza un trabajo y navega a su vista de progreso. */
export function useStartJob() {
  const navigate = useNavigate();
  const track = useJobs((s) => s.track);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const start = async (kind: JobKind, run: () => Promise<number>, context?: Record<string, number>) => {
    setError(null);
    setStarting(true);
    try {
      const id = await run();
      track(id, kind, context);
      navigate(`/trabajo/${id}`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStarting(false);
    }
  };
  return { start, error, starting };
}
