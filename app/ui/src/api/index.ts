/**
 * Acceso a datos de la UI.
 *
 * - `backend`: la API real de Tauri dentro de la app de escritorio, o la simulada
 *   (`mock.ts`) en el navegador. Ambas cumplen el mismo contrato (`backend.ts`).
 *
 * Forzar la simulación dentro de Tauri: VITE_USE_MOCKS=true.
 */
import type { Backend } from "./backend";
import { mockBackend } from "./mock";
import { tauriBackend } from "./tauri";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const usingMocks = !isTauri || import.meta.env.VITE_USE_MOCKS === "true";
export const backend: Backend = usingMocks ? mockBackend : tauriBackend;
