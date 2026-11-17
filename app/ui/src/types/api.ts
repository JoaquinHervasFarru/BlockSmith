/**
 * Tipos que devuelve el backend (app/src-tauri/src/core/dto.rs), en camelCase.
 */

export type SupportLevel = "yes" | "no" | "experimental";
export type SeverityKey = "critical" | "minor" | "cosmetic";

export interface CapabilitiesDto {
  scan: SupportLevel;
  recover: SupportLevel;
  rebuild: SupportLevel;
  writeInPlace: SupportLevel;
  note: string | null;
}

export interface SeverityCounts {
  critical: number;
  minor: number;
  cosmetic: number;
}

export interface SessionSummaryDto {
  id: number;
  status: string;
  startedAt: string;
  finishedAt: string | null;
  counts: SeverityCounts;
}

export interface VolumeDto {
  id: number;
  sourceId: number;
  partitionIndex: number | null;
  offsetBytes: number;
  lengthBytes: number;
  fsType: string;
  name: string | null;
  blockSize: number;
  totalBlocks: number;
  freeBlocks: number;
  fileCount: number | null;
  folderCount: number | null;
  caseSensitive: boolean;
  journaled: boolean;
  journalState: string;
  encrypted: boolean;
  capabilities: CapabilitiesDto;
  lastSession: SessionSummaryDto | null;
}

export interface SourceDto {
  id: number;
  kind: "raw_image" | "udif" | "device";
  path: string;
  sizeBytes: number;
  label: string | null;
  addedAt: string;
  partitionScheme: string;
  exists: boolean;
  volumes: VolumeDto[];
}

export type SessionStatus =
  | "pending" | "scanning" | "scanned" | "rebuilding" | "rebuilt" | "exported" | "failed" | "cancelled";

export interface SessionDto {
  id: number;
  volumeId: number;
  volumeName: string | null;
  fsType: string;
  sourcePath: string;
  sourceLabel: string | null;
  mode: string;
  status: SessionStatus;
  startedAt: string;
  finishedAt: string | null;
  catalogNodesTotal: number | null;
  catalogNodesDamaged: number | null;
  fragmentationPct: number | null;
  fileCount: number | null;
  folderCount: number | null;
  error: string | null;
  counts: SeverityCounts;
  latestRebuildId: number | null;
}

export interface FindingDto {
  id: number;
  sessionId: number;
  code: string;
  severity: SeverityKey;
  objectType: string;
  cnid: number | null;
  nodeNumber: number | null;
  path: string | null;
  message: string;
  fixedInRebuild: boolean;
}

export type ChangeKey =
  | "recovered" | "moved_to_recovered" | "renamed" | "valence_fixed" | "dropped"
  | "data_truncated" | "data_cloned" | "unchanged_flagged";

export interface RebuildDto {
  id: number;
  sessionId: number;
  status: "planned" | "exporting" | "done" | "failed";
  target: string;
  experimental: boolean;
  startedAt: string;
  finishedAt: string | null;
  nodesBefore: number;
  nodesAfter: number;
  recoveredCount: number;
  lostCount: number;
  outputPath: string | null;
  verified: boolean;
  bytesToWrite: number;
  changes: { change: ChangeKey; count: number }[];
  volumeName: string | null;
  sourcePath: string;
}

export interface DiffDto {
  id: number;
  rebuildId: number;
  cnid: number;
  change: ChangeKey;
  oldPath: string | null;
  newPath: string | null;
  itemType: "file" | "folder";
  sizeBytes: number | null;
  note: string | null;
}

export interface TreeNodeDto {
  id: number;
  name: string;
  isFolder: boolean;
  sizeBytes: number;
  childCount: number;
  change: ChangeKey | null;
}

export interface ExportResultDto {
  rebuildId: number;
  outputPath: string;
  verified: boolean;
  remainingFindings: FindingDto[];
  bytesCopied: number;
  bytesWritten: number;
}

export interface ReportDto {
  id: number;
  sessionId: number;
  format: "html" | "json";
  path: string;
  generatedAt: string;
}

export interface SettingsDto {
  outputDir: string;
  theme: "system" | "light" | "dark";
  logLevel: "error" | "warn" | "info" | "debug";
  exportEncryptedByDefault: boolean;
  healthCheckOnStart: boolean;
}

export interface AppInfo {
  version: string;
  platform: string;
  phase: string;
}

export type JobKind = "scan" | "plan" | "export" | "health";

export interface JobProgressEvent {
  jobId: number;
  kind: JobKind;
  phase: string;
  done: number;
  total: number;
}

export interface JobFinishedEvent {
  jobId: number;
  kind: JobKind;
  ok: boolean;
  cancelled: boolean;
  result: Record<string, unknown> | null;
  error: string | null;
}

// ── Salud de los discos (F4) ───────────────────────────────────────────────

export type OverallKey = "passed" | "warning" | "failing" | "unknown";

export interface AtaAttributeDto {
  id: number;
  name: string;
  flags: number;
  value: number;
  worst: number;
  threshold: number;
  raw: number;
}

export interface NvmeHealthDto {
  criticalWarning: number;
  temperatureK: number;
  availableSpare: number;
  spareThreshold: number;
  percentageUsed: number;
  dataUnitsRead: number;
  dataUnitsWritten: number;
  powerCycles: number;
  powerOnHours: number;
  unsafeShutdowns: number;
  mediaErrors: number;
  errorLogEntries: number;
}

export interface SnapshotDto {
  id: number;
  diskId: number;
  takenAt: string;
  overall: OverallKey;
  temperatureC: number | null;
  powerOnHours: number | null;
  percentageUsed: number | null;
  protocol: "ata" | "nvme" | null;
  attributes: AtaAttributeDto[];
  nvme: NvmeHealthDto | null;
  reasons: string[];
  source: string | null;
}

export interface DiskDto {
  id: number;
  path: string | null;
  model: string | null;
  serial: string | null;
  firmware: string | null;
  bus: "sata" | "nvme" | "usb" | "other";
  capacityBytes: number;
  isBootDisk: boolean;
  removable: boolean;
  rotational: boolean | null;
  readStatus: "ok" | "unsupported" | "permission" | "failed" | null;
  readNote: string | null;
  firstSeen: string;
  lastSeen: string;
  latest: SnapshotDto | null;
  openAlerts: number;
}

export interface AlertDto {
  id: number;
  diskId: number;
  diskModel: string | null;
  diskPath: string | null;
  rule: string;
  ruleLabel: string;
  severity: SeverityKey;
  message: string;
  createdAt: string;
  acknowledgedAt: string | null;
}

export interface DiskDetailDto {
  disk: DiskDto;
  history: SnapshotDto[];
  alerts: AlertDto[];
}

export interface RuleDto {
  id: number;
  rule: string;
  enabled: boolean;
  threshold: Record<string, number>;
  pollIntervalMin: number;
}

export interface PollStatusDto {
  at: string;
  by: string;
  backend: string;
  disks: number;
  read: number;
  error: string | null;
}

export interface HealthStatusDto {
  lastPoll: PollStatusDto | null;
  backend: string;
  pollIntervalMin: number;
  openAlerts: number;
}
