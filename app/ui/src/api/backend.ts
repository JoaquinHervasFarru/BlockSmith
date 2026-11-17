import type {
  AlertDto, AppInfo, DeviceOptionDto, PreflightDto, TxDto, DiffDto, DiskDetailDto, DiskDto, HealthStatusDto, RuleDto, FindingDto, JobFinishedEvent, JobProgressEvent, RebuildDto, ReportDto, SessionDto, SettingsDto,
  SourceDto, TreeNodeDto, VolumeDto,
} from "../types/api";

/** Contrato común de la API real (Tauri) y la simulada (navegador). */
export interface Backend {
  readonly simulated: boolean;
  appInfo(): Promise<AppInfo>;

  /** Diálogo para elegir una imagen. `null` si se cancela. */
  pickImage(): Promise<string | null>;
  /** Diálogo para elegir dónde guardar. `null` si se cancela. */
  pickSavePath(suggested: string): Promise<string | null>;
  /** Diálogo para elegir una carpeta. `null` si se cancela. */
  pickFolder(current: string): Promise<string | null>;

  openImage(path: string): Promise<SourceDto>;
  listSources(): Promise<SourceDto[]>;
  removeSource(id: number): Promise<void>;
  getVolume(id: number): Promise<VolumeDto>;

  listSessions(): Promise<SessionDto[]>;
  getSession(id: number): Promise<SessionDto>;
  getFindings(sessionId: number): Promise<FindingDto[]>;

  getRebuild(id: number): Promise<RebuildDto>;
  getDiff(rebuildId: number): Promise<DiffDto[]>;
  treeChildren(side: "original" | "rebuilt", id: number, parent: number | null): Promise<TreeNodeDto[]>;
  suggestExportPath(rebuildId: number): Promise<string>;

  startScan(volumeId: number): Promise<number>;
  startPlan(sessionId: number): Promise<number>;
  startExport(rebuildId: number, outPath: string): Promise<number>;
  cancelJob(jobId: number): Promise<boolean>;

  writeReport(sessionId: number, format: "html" | "json", path?: string): Promise<ReportDto>;
  listReports(sessionId: number): Promise<ReportDto[]>;

  getSettings(): Promise<SettingsDto>;
  setSettings(s: SettingsDto): Promise<SettingsDto>;

  healthDisks(): Promise<DiskDto[]>;
  healthDisk(id: number): Promise<DiskDetailDto>;
  healthStatus(): Promise<HealthStatusDto>;
  startHealthRead(): Promise<number>;
  listAlerts(openOnly: boolean): Promise<AlertDto[]>;
  ackAlert(id: number): Promise<void>;
  getAlertRules(): Promise<RuleDto[]>;
  setAlertRule(rule: RuleDto): Promise<RuleDto[]>;
  preflight(rebuildId: number): Promise<PreflightDto>;
  startRepair(rebuildId: number, confirm: string): Promise<number>;
  listTransactions(): Promise<TxDto[]>;
  startTxRollback(txId: number, force: boolean): Promise<number>;
  startTxComplete(txId: number): Promise<number>;
  listOpenDevices(): Promise<DeviceOptionDto[]>;

  /** Aviso de que la app actualizó la salud en segundo plano. */
  onHealthUpdated(cb: () => void): Promise<() => void>;

  onJobProgress(cb: (e: JobProgressEvent) => void): Promise<() => void>;
  onJobFinished(cb: (e: JobFinishedEvent) => void): Promise<() => void>;
}
