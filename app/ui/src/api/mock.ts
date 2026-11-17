/**
 * Backend simulado para el navegador: mismo contrato que el real, con estado en
 * memoria y trabajos que avanzan por fases como los de verdad. Permite recorrer
 * el flujo completo (abrir → analizar → planificar → exportar) sin Tauri.
 */
import * as mock from "../mocks/data";
import type { TreeNode } from "../types/models";
import type {
  AlertDto, ChangeKey, DiffDto, DiskDto, NvmeHealthDto, RuleDto, SnapshotDto, FindingDto, JobFinishedEvent, JobKind, JobProgressEvent, RebuildDto, ReportDto, SessionDto,
  SettingsDto, SourceDto, TreeNodeDto, VolumeDto,
} from "../types/api";
import type { Backend } from "./backend";

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const clone = <T,>(v: T): T => structuredClone(v);
const nowIso = () => new Date().toISOString().replace(/\.\d+Z$/, "Z");

const HFS_CAPS = {
  scan: "yes", recover: "no", rebuild: "yes", writeInPlace: "no",
  note: "La reconstrucción se exporta a una imagen nueva; la escritura directa llega en una fase posterior.",
} as const;
const EXT4_CAPS = { scan: "no", recover: "no", rebuild: "no", writeInPlace: "no", note: "El análisis de ext4 llegará en una fase posterior." } as const;

const PHASES: Record<JobKind, string[]> = {
  scan: ["Cabecera del volumen", "Árbol de extents", "Catálogo: estructura", "Catálogo: coherencia", "Extents y bitmap de asignación"],
  plan: ["Cabecera del volumen", "Catálogo: estructura", "Catálogo: coherencia", "Plan de reconstrucción"],
  export: ["Copiando la imagen", "Aplicando la reconstrucción", "Verificando la imagen reparada"],
  health: ["Leyendo la salud de los discos"],
};

// ── Salud de los discos (a partir de mocks/data.ts) ─────────────────────────
const diskPaths: Record<number, string> = { 1: "/dev/disk0", 2: "/dev/disk4", 3: "/dev/sdb", 4: "\\\\.\\PhysicalDrive1", 5: "/dev/disk6" };
const reasonsFor: Record<string, string[]> = {
  warning: ["24 sectores reasignados", "3 sectores pendientes de reasignación"],
  failing: ["Reallocated_Sector_Ct por debajo del umbral (7 ≤ 10)"],
};
const appleNvme: NvmeHealthDto = {
  criticalWarning: 0, temperatureK: 311, availableSpare: 100, spareThreshold: 99, percentageUsed: 4, dataUnitsRead: 41_208_331,
  dataUnitsWritten: 38_812_004, powerCycles: 1_204, powerOnHours: 6_381, unsafeShutdowns: 31, mediaErrors: 0, errorLogEntries: 0,
};
function snapshotDto(s: (typeof mock.smartSnapshots)[number]): SnapshotDto {
  const disk = mock.disks.find((d) => d.id === s.diskId)!;
  const isNvme = disk.bus === "nvme";
  return {
    id: s.id, diskId: s.diskId, takenAt: s.takenAt, overall: s.overall, temperatureC: s.temperatureC, powerOnHours: s.powerOnHours,
    percentageUsed: s.percentageUsed, protocol: isNvme ? "nvme" : "ata",
    attributes: s.attributes.map((a) => ({ id: a.attrId, name: a.name, flags: 0, value: a.value, worst: a.worst, threshold: a.threshold, raw: a.rawValue })),
    nvme: isNvme ? { ...appleNvme, temperatureK: (s.temperatureC ?? 38) + 273, powerOnHours: s.powerOnHours ?? 0 } : null,
    reasons: reasonsFor[s.overall] ?? [], source: "agente",
  };
}
const healthState = {
  alerts: mock.alerts.map<AlertDto>((a) => {
    const d = mock.disks.find((x) => x.id === a.diskId)!;
    const labels: Record<string, string> = {
      SMART_FAILING: "El disco predice un fallo", PENDING_SECTORS: "Sectores pendientes de reasignación",
      REALLOCATED_INCREASE: "Aumentan los sectores reasignados", TEMP_HIGH: "Temperatura alta",
    };
    return {
      id: a.id, diskId: a.diskId, diskModel: d.model, diskPath: diskPaths[d.id] ?? null, rule: a.rule, ruleLabel: labels[a.rule] ?? a.rule,
      severity: a.severity, message: a.message, createdAt: a.createdAt, acknowledgedAt: a.acknowledgedAt,
    };
  }),
  rules: [
    ["SMART_FAILING", {}, 360], ["REALLOCATED_INCREASE", { delta: 1 }, 360], ["PENDING_SECTORS", { min: 1 }, 360],
    ["NVME_CRITICAL_WARNING", { mask: 255 }, 360], ["NVME_WEAR", { percentage_used: 90 }, 1440], ["TEMP_HIGH", { celsius: 60 }, 60],
  ].map<RuleDto>(([rule, threshold, poll], i) => ({ id: i + 1, rule: rule as string, enabled: true, threshold: threshold as Record<string, number>, pollIntervalMin: poll as number })),
  lastPoll: { at: "2026-11-17T03:00:04Z", by: "agente", backend: "nativo", disks: 5, read: 3, error: null as string | null },
};
function diskDto(d: (typeof mock.disks)[number]): DiskDto {
  const snaps = mock.smartSnapshots.filter((s) => s.diskId === d.id);
  const last = snaps.at(-1);
  return {
    id: d.id, path: diskPaths[d.id] ?? null, model: d.model, serial: d.serial, firmware: d.firmware, bus: d.bus, capacityBytes: d.capacityBytes,
    isBootDisk: d.isBootDisk, removable: d.bus === "usb", rotational: d.bus === "sata" ? true : d.bus === "nvme" ? false : null,
    readStatus: d.smartSupported ? "ok" : "unsupported",
    readNote: d.smartSupported ? null : "La caja USB no transmite los datos S.M.A.R.T. del disco.",
    firstSeen: d.firstSeen, lastSeen: d.lastSeen, latest: last ? snapshotDto(last) : null,
    openAlerts: healthState.alerts.filter((a) => a.diskId === d.id && !a.acknowledgedAt).length,
  };
}
const healthCbs = new Set<() => void>();

interface State {
  sources: SourceDto[];
  sessions: SessionDto[];
  findings: FindingDto[];
  rebuilds: RebuildDto[];
  diffs: DiffDto[];
  reports: ReportDto[];
  settings: SettingsDto;
  /** Volúmenes con el directorio dañado (sus análisis encuentran problemas). */
  damaged: Set<number>;
  nextId: number;
}

function volume(id: number, sourceId: number, name: string, fsType: string, files: number, folders: number, totalBlocks: number, freeBlocks: number): VolumeDto {
  return {
    id, sourceId, partitionIndex: fsType === "ext4" ? 1 : null, offsetBytes: 0, lengthBytes: totalBlocks * 4096, fsType, name,
    blockSize: 4096, totalBlocks, freeBlocks, fileCount: files, folderCount: folders, caseSensitive: false,
    journaled: fsType !== "ext4", journalState: "clean", encrypted: false,
    capabilities: { ...(fsType === "ext4" ? EXT4_CAPS : HFS_CAPS) }, lastSession: null,
  };
}

const state: State = {
  sources: [
    {
      id: 1, kind: "udif", path: "/Volumes/Archivo/imagenes/archivo-fotos.dmg", sizeBytes: 999_653_638_144,
      label: "archivo-fotos.dmg", addedAt: "2026-11-16T22:18:00Z", partitionScheme: "gpt", exists: true,
      volumes: [volume(1, 1, "Archivo Fotos", "hfsplus", 1_206_447, 38_912, 244_071_423, 61_208_904)],
    },
    {
      id: 2, kind: "raw_image", path: "/home/usuario/imagenes/fotos-2019.img", sizeBytes: 64_199_032_832,
      label: "fotos-2019.img", addedAt: "2026-09-04T11:20:00Z", partitionScheme: "none", exists: true,
      volumes: [volume(2, 2, "Fotos 2019", "hfsplus", 84_310, 2_118, 15_673_339, 902_117)],
    },
    {
      id: 3, kind: "raw_image", path: "/home/usuario/imagenes/portatil-home.img", sizeBytes: 256_000_000_000,
      label: "portatil-home.img", addedAt: "2026-11-10T16:45:00Z", partitionScheme: "gpt", exists: true,
      volumes: [volume(3, 3, "home", "ext4", 640_221, 71_090, 62_499_840, 19_004_233)],
    },
  ],
  sessions: [
    {
      id: 1, volumeId: 2, volumeName: "Fotos 2019", fsType: "hfsplus", sourcePath: "/home/usuario/imagenes/fotos-2019.img",
      sourceLabel: "fotos-2019.img", mode: "image", status: "exported", startedAt: "2026-09-04T11:25:00Z",
      finishedAt: "2026-09-04T11:39:02Z", catalogNodesTotal: 3_904, catalogNodesDamaged: 0, fragmentationPct: 12.4,
      fileCount: 84_310, folderCount: 2_118, error: null, counts: { critical: 0, minor: 1, cosmetic: 1 }, latestRebuildId: 1,
    },
  ],
  findings: mock.findings.filter((f) => f.sessionId === 2).map((f) => ({
    id: f.id, sessionId: 1, code: f.code, severity: f.severity, objectType: f.objectType, cnid: f.cnid,
    nodeNumber: f.nodeNumber, path: f.path, message: f.message, fixedInRebuild: true,
  })),
  rebuilds: [
    {
      id: 1, sessionId: 1, status: "done", target: "new_image", experimental: false, startedAt: "2026-09-04T11:33:00Z",
      finishedAt: "2026-09-04T11:39:02Z", nodesBefore: 3_904, nodesAfter: 1_766, recoveredCount: 0, lostCount: 0,
      outputPath: "/home/usuario/BlockSmith/fotos-2019-reparada.img", verified: true, bytesToWrite: 9_437_184,
      changes: [{ change: "valence_fixed", count: 1 }], volumeName: "Fotos 2019", sourcePath: "/home/usuario/imagenes/fotos-2019.img",
    },
  ],
  diffs: [
    { id: 1, rebuildId: 1, cnid: 2201, change: "valence_fixed", oldPath: "/Navidad", newPath: "/Navidad", itemType: "folder", sizeBytes: null, note: "412 → 410 elementos" },
  ],
  reports: [],
  settings: { outputDir: "/home/usuario/BlockSmith", theme: "system", logLevel: "info", exportEncryptedByDefault: true, healthCheckOnStart: true },
  damaged: new Set([1]),
  nextId: 100,
};
state.sources[1]!.volumes[0]!.lastSession = { id: 1, status: "exported", startedAt: "2026-09-04T11:25:00Z", finishedAt: "2026-09-04T11:39:02Z", counts: { critical: 0, minor: 1, cosmetic: 1 } };

// ── Árboles ──────────────────────────────────────────────────────────────
function index(tree: TreeNode) {
  const byId = new Map<number, TreeNode>();
  const walk = (n: TreeNode) => {
    byId.set(n.cnid, n);
    n.children?.forEach(walk);
  };
  walk(tree);
  return byId;
}
const originalIdx = index(mock.originalTree);
const rebuiltIdx = index(mock.rebuiltTree);

function toDto(n: TreeNode, changes: Map<number, ChangeKey>): TreeNodeDto {
  return {
    id: n.cnid, name: n.name, isFolder: n.type === "folder", sizeBytes: n.sizeBytes ?? 0,
    childCount: n.children?.length ?? 0, change: changes.get(n.cnid) ?? n.change ?? null,
  };
}

function sortNodes(list: TreeNodeDto[]) {
  return list.sort((a, b) => Number(b.isFolder) - Number(a.isFolder) || a.name.localeCompare(b.name, "es"));
}

// ── Trabajos simulados ───────────────────────────────────────────────────
const progressCbs = new Set<(e: JobProgressEvent) => void>();
const finishedCbs = new Set<(e: JobFinishedEvent) => void>();
const cancelled = new Set<number>();

function runJob(kind: JobKind, work: () => Record<string, unknown>): number {
  const jobId = state.nextId++;
  void (async () => {
    for (const phase of PHASES[kind]) {
      for (let step = 0; step <= 4; step++) {
        if (cancelled.has(jobId)) {
          finishedCbs.forEach((cb) => cb({ jobId, kind, ok: false, cancelled: true, result: null, error: "operación cancelada" }));
          return;
        }
        progressCbs.forEach((cb) => cb({ jobId, kind, phase, done: step, total: step === 0 ? 0 : 4 }));
        await wait(110);
      }
    }
    try {
      const result = work();
      finishedCbs.forEach((cb) => cb({ jobId, kind, ok: true, cancelled: false, result, error: null }));
    } catch (e) {
      finishedCbs.forEach((cb) => cb({ jobId, kind, ok: false, cancelled: false, result: null, error: String(e) }));
    }
  })();
  return jobId;
}

function findVolume(id: number): VolumeDto {
  for (const s of state.sources) for (const v of s.volumes) if (v.id === id) return v;
  throw new Error(`volumen ${id} no encontrado`);
}

function sessionOf(id: number): SessionDto {
  const s = state.sessions.find((x) => x.id === id);
  if (!s) throw new Error(`sesión ${id} no encontrada`);
  return s;
}

function rebuildOf(id: number): RebuildDto {
  const r = state.rebuilds.find((x) => x.id === id);
  if (!r) throw new Error(`reconstrucción ${id} no encontrada`);
  return r;
}

function sourceOfVolume(v: VolumeDto): SourceDto {
  return state.sources.find((s) => s.id === v.sourceId)!;
}

export const mockBackend: Backend = {
  simulated: true,
  appInfo: async () => ({ version: "0.1.0", platform: "navegador", phase: "F3" }),

  async pickImage() {
    await wait(150);
    return "/home/usuario/imagenes/copia-seguridad-2024.dmg";
  },
  async pickSavePath(suggested) {
    await wait(150);
    return suggested;
  },
  async pickFolder(current) {
    await wait(150);
    return current;
  },

  async openImage(path) {
    await wait(250);
    const existing = state.sources.find((s) => s.path === path);
    if (existing) return clone(existing);
    const id = state.nextId++;
    const label = path.split("/").pop() ?? path;
    const src: SourceDto = {
      id, kind: path.endsWith(".dmg") ? "udif" : "raw_image", path, sizeBytes: 500_107_862_016, label,
      addedAt: nowIso(), partitionScheme: "gpt", exists: true,
      volumes: [volume(state.nextId++, id, "Copia de seguridad", "hfsplus", 402_118, 18_221, 122_096_646, 30_118_220)],
    };
    state.damaged.add(src.volumes[0]!.id);
    state.sources.unshift(src);
    return clone(src);
  },
  async listSources() {
    await wait(120);
    return clone(state.sources);
  },
  async removeSource(id) {
    state.sources = state.sources.filter((s) => s.id !== id);
  },
  async getVolume(id) {
    await wait(80);
    return clone(findVolume(id));
  },

  async listSessions() {
    await wait(120);
    return clone([...state.sessions].sort((a, b) => b.id - a.id));
  },
  async getSession(id) {
    await wait(80);
    return clone(sessionOf(id));
  },
  async getFindings(sessionId) {
    await wait(120);
    return clone(state.findings.filter((f) => f.sessionId === sessionId));
  },

  async getRebuild(id) {
    await wait(80);
    return clone(rebuildOf(id));
  },
  async getDiff(rebuildId) {
    await wait(100);
    return clone(state.diffs.filter((d) => d.rebuildId === rebuildId));
  },
  async treeChildren(side, id, parent) {
    await wait(90);
    const rebuildId = side === "rebuilt" ? id : sessionOf(id).latestRebuildId;
    const changes = new Map<number, ChangeKey>(
      state.diffs.filter((d) => d.rebuildId === rebuildId).map((d) => [d.cnid, d.change] as const),
    );
    const idx = side === "rebuilt" ? rebuiltIdx : originalIdx;
    const root = side === "rebuilt" ? mock.rebuiltTree : mock.originalTree;
    if (parent === null) return [toDto(root, changes)];
    const node = idx.get(parent);
    return sortNodes((node?.children ?? []).map((c) => toDto(c, side === "rebuilt" ? changes : new Map())));
  },
  async suggestExportPath(rebuildId) {
    const r = rebuildOf(rebuildId);
    const stem = (r.sourcePath.split("/").pop() ?? "volumen").replace(/\.[^.]+$/, "");
    return `${state.settings.outputDir}/${stem}-reparada.img`;
  },

  async startScan(volumeId) {
    const v = findVolume(volumeId);
    if (v.capabilities.scan !== "yes") throw new Error(v.capabilities.note ?? "No disponible");
    return runJob("scan", () => {
      const id = state.nextId++;
      const damaged = state.damaged.has(volumeId);
      const found: FindingDto[] = damaged
        ? mock.findings.filter((f) => f.sessionId === 1).map((f) => ({
            id: state.nextId++, sessionId: id, code: f.code, severity: f.severity, objectType: f.objectType,
            cnid: f.cnid, nodeNumber: f.nodeNumber, path: f.path, message: f.message, fixedInRebuild: false,
          }))
        : [];
      const counts = { critical: 0, minor: 0, cosmetic: 0 };
      found.forEach((f) => counts[f.severity]++);
      state.findings.push(...found);
      const src = sourceOfVolume(v);
      const s: SessionDto = {
        id, volumeId, volumeName: v.name, fsType: v.fsType, sourcePath: src.path, sourceLabel: src.label, mode: "image",
        status: "scanned", startedAt: nowIso(), finishedAt: nowIso(), catalogNodesTotal: damaged ? 48_211 : 3_904,
        catalogNodesDamaged: damaged ? 37 : 0, fragmentationPct: damaged ? 41.8 : 9.5, fileCount: v.fileCount,
        folderCount: v.folderCount, error: null, counts, latestRebuildId: null,
      };
      state.sessions.push(s);
      v.lastSession = { id, status: s.status, startedAt: s.startedAt, finishedAt: s.finishedAt, counts };
      return { sessionId: id };
    });
  },
  async startPlan(sessionId) {
    const s = sessionOf(sessionId);
    return runJob("plan", () => {
      const id = state.nextId++;
      const entries: DiffDto[] = (s.counts.critical + s.counts.minor > 0 ? mock.diffEntries : []).map((d) => ({
        id: state.nextId++, rebuildId: id, cnid: d.cnid, change: d.change, oldPath: d.oldPath, newPath: d.newPath,
        itemType: d.itemType, sizeBytes: d.sizeBytes, note: d.note ?? null,
      }));
      state.diffs.push(...entries);
      const counts = new Map<ChangeKey, number>();
      entries.forEach((d) => counts.set(d.change, (counts.get(d.change) ?? 0) + 1));
      state.rebuilds.push({
        id, sessionId, status: "planned", target: "new_image", experimental: false, startedAt: nowIso(), finishedAt: null,
        nodesBefore: s.catalogNodesTotal ?? 0, nodesAfter: Math.round((s.catalogNodesTotal ?? 0) * 0.66),
        recoveredCount: entries.filter((d) => d.change === "recovered" || d.change === "moved_to_recovered").length,
        lostCount: 0, outputPath: null, verified: false, bytesToWrite: 18_874_368,
        changes: [...counts].map(([change, count]) => ({ change, count })).sort((a, b) => b.count - a.count),
        volumeName: s.volumeName, sourcePath: s.sourcePath,
      });
      s.latestRebuildId = id;
      s.status = "rebuilt";
      return { rebuildId: id };
    });
  },
  async startExport(rebuildId, outPath) {
    const r = rebuildOf(rebuildId);
    r.status = "exporting";
    return runJob("export", () => {
      r.status = "done";
      r.outputPath = outPath;
      r.verified = true;
      r.finishedAt = nowIso();
      const s = sessionOf(r.sessionId);
      s.status = "exported";
      state.findings.filter((f) => f.sessionId === s.id).forEach((f) => (f.fixedInRebuild = true));
      return { rebuildId, outputPath: outPath, verified: true, remainingFindings: [], bytesCopied: 749_000_000_000, bytesWritten: r.bytesToWrite };
    });
  },
  async cancelJob(jobId) {
    cancelled.add(jobId);
    return true;
  },

  async writeReport(sessionId, format, path) {
    await wait(200);
    const rep: ReportDto = {
      id: state.nextId++, sessionId, format,
      path: path ?? `${state.settings.outputDir}/informes/blocksmith-sesion-${sessionId}.${format}`, generatedAt: nowIso(),
    };
    state.reports.unshift(rep);
    return clone(rep);
  },
  async listReports(sessionId) {
    return clone(state.reports.filter((r) => r.sessionId === sessionId));
  },

  async getSettings() {
    return clone(state.settings);
  },
  async setSettings(s) {
    if (!s.outputDir.trim()) throw new Error("la carpeta de salida no puede estar vacía");
    state.settings = clone(s);
    return clone(s);
  },

  async healthDisks() {
    await wait(120);
    return mock.disks.map(diskDto).sort((a, b) => Number(b.isBootDisk) - Number(a.isBootDisk));
  },
  async healthDisk(id) {
    await wait(100);
    const d = mock.disks.find((x) => x.id === id);
    if (!d) throw new Error(`disco ${id} no encontrado`);
    return {
      disk: diskDto(d),
      history: mock.smartSnapshots.filter((s) => s.diskId === id).map(snapshotDto),
      alerts: clone(healthState.alerts.filter((a) => a.diskId === id)),
    };
  },
  async healthStatus() {
    await wait(60);
    return { lastPoll: clone(healthState.lastPoll), backend: "nativo", pollIntervalMin: 60, openAlerts: healthState.alerts.filter((a) => !a.acknowledgedAt).length };
  },
  async startHealthRead() {
    return runJob("health", () => {
      healthState.lastPoll = { ...healthState.lastPoll, at: nowIso(), by: "app" };
      healthCbs.forEach((cb) => cb());
      return { disks: 5, read: 3, unsupported: 2, permissionDenied: 0, failed: 0, newAlerts: [] };
    });
  },
  async listAlerts(openOnly) {
    await wait(80);
    return clone(healthState.alerts.filter((a) => !openOnly || !a.acknowledgedAt));
  },
  async ackAlert(id) {
    const a = healthState.alerts.find((x) => x.id === id);
    if (!a || a.acknowledgedAt) throw new Error(`la alerta ${id} no existe o ya estaba confirmada`);
    a.acknowledgedAt = nowIso();
    healthCbs.forEach((cb) => cb());
  },
  async getAlertRules() {
    return clone(healthState.rules);
  },
  async setAlertRule(rule) {
    if (Object.values(rule.threshold).some((v) => v < 0)) throw new Error("los umbrales no pueden ser negativos");
    healthState.rules = healthState.rules.map((r) => (r.id === rule.id ? { ...clone(rule), pollIntervalMin: Math.min(10_080, Math.max(15, rule.pollIntervalMin)) } : r));
    return clone(healthState.rules);
  },
  async onHealthUpdated(cb) {
    healthCbs.add(cb);
    return () => healthCbs.delete(cb);
  },

  async onJobProgress(cb) {
    progressCbs.add(cb);
    return () => progressCbs.delete(cb);
  },
  async onJobFinished(cb) {
    finishedCbs.add(cb);
    return () => finishedCbs.delete(cb);
  },
};
