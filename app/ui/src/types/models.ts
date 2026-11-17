/**
 * Tipos de dominio de la UI. Reflejan el esquema SQLite
 * (app/src-tauri/migrations/0001_init.sql) en camelCase.
 */

export type ISODate = string;
export type Id = number;

export type Bus = "sata" | "nvme" | "usb" | "other";
export type FsType = "hfsplus" | "hfsx" | "apfs" | "ntfs" | "ext4";
export type Severity = "critical" | "minor" | "cosmetic";
export type SmartOverall = "passed" | "warning" | "failing" | "unknown";
export type Support = "no" | "yes" | "experimental";

export interface Disk {
  id: Id;
  serial: string | null;
  model: string;
  firmware: string | null;
  bus: Bus;
  capacityBytes: number;
  isBootDisk: boolean;
  smartSupported: boolean;
  firstSeen: ISODate;
  lastSeen: ISODate;
}

export interface Source {
  id: Id;
  diskId: Id | null;
  kind: "raw_image" | "udif" | "device";
  path: string;
  sizeBytes: number;
  sha256: string | null;
  label: string | null;
  addedAt: ISODate;
}

export interface Volume {
  id: Id;
  sourceId: Id;
  partitionIndex: number | null;
  offsetBytes: number;
  fsType: FsType;
  name: string;
  blockSize: number;
  totalBlocks: number;
  freeBlocks: number;
  fileCount: number | null;
  folderCount: number | null;
  caseSensitive: boolean;
  journaled: boolean;
  journalState: "clean" | "pending" | "none";
  containerUuid: string | null;
  volumeUuid: string | null;
  role: string | null;
  encrypted: boolean;
  encryptionKind: "none" | "software" | "hardware";
  isMounted: boolean;
  createdAtFs: ISODate | null;
  modifiedAtFs: ISODate | null;
}

export type EncryptionScheme =
  | "bitlocker" | "luks1" | "luks2" | "corestorage" | "apfs" | "fscrypt" | "veracrypt" | "hardware";
export type ProtectorKind = "tpm" | "password" | "recovery" | "keyfile";

export interface EncryptionLayer {
  id: Id;
  volumeId: Id;
  scheme: EncryptionScheme;
  cipher: string | null;
  headerOk: boolean;
  backupHeadersOk: boolean;
  /** Solo los tipos. Nunca valores. */
  protectors: ProtectorKind[];
  /** Estado en la sesión actual de la UI (no se persiste). */
  unlocked: boolean;
  detectedAt: ISODate;
}

export type SessionStatus =
  | "pending" | "scanning" | "scanned" | "rebuilding" | "rebuilt" | "exported" | "failed" | "cancelled";

export interface Capabilities {
  scan: Support;
  recover: Support;
  rebuild: Support;
  writeInPlace: Support;
}

export interface Session {
  id: Id;
  volumeId: Id;
  mode: "image" | "direct";
  status: SessionStatus;
  unlockedWith: "password" | "recovery" | "keyfile" | "none";
  capabilities: Capabilities;
  startedAt: ISODate;
  finishedAt: ISODate | null;
  catalogNodesTotal: number | null;
  catalogNodesDamaged: number | null;
  fragmentationPct: number | null;
  appVersion: string;
  error: string | null;
}

export interface Finding {
  id: Id;
  sessionId: Id;
  code: string;
  severity: Severity;
  objectType: "node" | "folder" | "file" | "extent" | "bitmap" | "header" | "encryption";
  cnid: number | null;
  nodeNumber: number | null;
  path: string | null;
  message: string;
  details?: Record<string, unknown>;
  fixedInRebuild: boolean;
}

export interface Rebuild {
  id: Id;
  sessionId: Id;
  status: "planned" | "running" | "done" | "failed";
  target: "new_image" | "in_place";
  experimental: boolean;
  startedAt: ISODate;
  finishedAt: ISODate | null;
  nodesBefore: number;
  nodesAfter: number;
  recoveredCount: number;
  lostCount: number;
  outputPath: string | null;
  outputSha256: string | null;
  verified: boolean;
  backupMetadataPath: string | null;
}

export type DiffChange =
  | "recovered" | "moved_to_recovered" | "renamed" | "valence_fixed" | "dropped"
  | "data_truncated" | "data_cloned" | "unchanged_flagged";

export interface DiffEntry {
  id: Id;
  rebuildId: Id;
  cnid: number;
  change: DiffChange;
  oldPath: string | null;
  newPath: string | null;
  itemType: "file" | "folder";
  sizeBytes: number | null;
  note?: string | null;
}

export interface WriteTransaction {
  id: Id;
  rebuildId: Id;
  devicePath: string;
  deviceSerial: string | null;
  status: "prepared" | "writing" | "committed" | "verified" | "rolled_back" | "interrupted";
  undoLogPath: string;
  blocksTotal: number;
  blocksWritten: number;
  startedAt: ISODate;
  finishedAt: ISODate | null;
  helperVersion: string;
}

export interface SmartAttribute {
  attrId: number;
  name: string;
  value: number;
  worst: number;
  threshold: number;
  rawValue: number;
}

export interface SmartSnapshot {
  id: Id;
  diskId: Id;
  takenAt: ISODate;
  overall: SmartOverall;
  temperatureC: number | null;
  powerOnHours: number | null;
  percentageUsed: number | null;
  attributes: SmartAttribute[];
}

export interface Alert {
  id: Id;
  diskId: Id;
  rule: string;
  severity: Severity;
  message: string;
  createdAt: ISODate;
  acknowledgedAt: ISODate | null;
}

export interface Report {
  id: Id;
  sessionId: Id;
  format: "html" | "json";
  path: string;
  generatedAt: ISODate;
}

export interface BootMedia {
  id: Id;
  deviceSerial: string | null;
  imageVersion: string;
  imageSha256: string;
  arch: "x86_64" | "arm64";
  kind: "linux_live" | "macos_external";
  createdAt: ISODate;
  verified: boolean;
}

/** Nodo del árbol de directorio (snapshot) para las vistas de comparación. */
export interface TreeNode {
  cnid: number;
  name: string;
  type: "file" | "folder";
  sizeBytes?: number;
  change?: DiffChange;
  children?: TreeNode[];
}

export interface Settings {
  outputDir: string;
  language: "es" | "en";
  theme: "system" | "light" | "dark";
  logLevel: "error" | "warn" | "info" | "debug";
  exportEncryptedByDefault: boolean;
}
