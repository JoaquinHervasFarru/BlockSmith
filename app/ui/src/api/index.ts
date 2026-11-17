/**
 * Capa de acceso a datos de la UI.
 *
 * - Dentro de Tauri, los comandos se resolverán vía `invoke` (a partir de F3).
 * - En el navegador (`npm run dev`) o mientras el motor no exista, se sirven los
 *   mocks de `src/mocks/data.ts` con una latencia simulada.
 *
 * Forzar mocks dentro de Tauri: VITE_USE_MOCKS=true.
 */
import * as mock from "../mocks/data";
import type {
  Alert, Disk, EncryptionLayer, Finding, Session, SmartSnapshot, Source, Volume,
} from "../types/models";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const usingMocks = !isTauri || import.meta.env.VITE_USE_MOCKS === "true";

const delay = <T,>(value: T, ms = 180) =>
  new Promise<T>((resolve) => setTimeout(() => resolve(structuredClone(value)), ms));

export interface DiskOverview {
  disk: Disk | null;
  sources: Source[];
  volumes: (Volume & { encryption: EncryptionLayer | null })[];
  latestSmart: SmartSnapshot | null;
  openAlerts: number;
}

function latestSmart(diskId: number): SmartSnapshot | null {
  const list = mock.smartSnapshots.filter((s) => s.diskId === diskId);
  return list.length ? list[list.length - 1]! : null;
}

function volumesFor(sourceIds: number[]) {
  return mock.volumes
    .filter((v) => sourceIds.includes(v.sourceId))
    .map((v) => ({ ...v, encryption: mock.encryptionLayers.find((e) => e.volumeId === v.id) ?? null }));
}

export const api = {
  async overview(): Promise<DiskOverview[]> {
    const physical: DiskOverview[] = mock.disks.map((disk) => {
      const sources = mock.sources.filter((s) => s.diskId === disk.id);
      return {
        disk,
        sources,
        volumes: volumesFor(sources.map((s) => s.id)),
        latestSmart: latestSmart(disk.id),
        openAlerts: mock.alerts.filter((a) => a.diskId === disk.id && !a.acknowledgedAt).length,
      };
    });
    const images: DiskOverview[] = mock.sources
      .filter((s) => s.diskId === null)
      .map((s) => ({ disk: null, sources: [s], volumes: volumesFor([s.id]), latestSmart: null, openAlerts: 0 }));
    return delay([...physical, ...images]);
  },

  async smartHistory(diskId: number): Promise<SmartSnapshot[]> {
    return delay(mock.smartSnapshots.filter((s) => s.diskId === diskId));
  },

  async alerts(): Promise<Alert[]> {
    return delay(mock.alerts);
  },

  async sessions(): Promise<Session[]> {
    return delay(mock.sessions);
  },

  async findings(sessionId: number): Promise<Finding[]> {
    return delay(mock.findings.filter((f) => f.sessionId === sessionId));
  },

  async interruptedTransactions() {
    return delay(mock.writeTransactions.filter((t) => t.status === "interrupted"));
  },
};
