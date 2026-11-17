/**
 * Datos simulados realistas para desarrollar la UI sin motor ni base de datos.
 *
 * Escenarios cubiertos:
 *  - Disco de arranque NVMe interno (APFS, cifrado por hardware → nivel D no aplicable).
 *  - SSD SATA en caja USB sin S.M.A.R.T. (HFS+ con directorio dañado y ya reconstruido).
 *  - HDD de NAS con sectores reasignados en aumento (ext4 sobre LUKS2, bloqueado).
 *  - HDD de PC fallando (NTFS con BitLocker protegido por TPM + clave de recuperación).
 *  - Memoria USB con DirForge Live.
 *  - Imágenes: .dmg con HFS+ y .img raw con ext4 + fscrypt.
 *  - Una transacción de escritura directa interrumpida (para la vista de recuperación).
 *
 * Todo es ficticio: números de serie, rutas y hashes son inventados.
 */

import type {
  Alert, BootMedia, Capabilities, DiffEntry, Disk, EncryptionLayer, Finding, Rebuild, Report,
  Session, Settings, SmartAttribute, SmartSnapshot, Source, TreeNode, Volume, WriteTransaction,
} from "../types/models";

const GB = 1_000_000_000;
const TB = 1_000 * GB;

/** Fecha de referencia de los mocks (fija para que sean deterministas). */
export const MOCK_NOW = "2026-11-17T10:30:00Z";

// ─────────────────────────────────────────────────────────────── Discos
export const disks: Disk[] = [
  {
    id: 1, serial: "FV9K2M7Q41XA", model: "Apple SSD AP0512Z", firmware: "1381.40.3",
    bus: "nvme", capacityBytes: 500.28 * GB, isBootDisk: true, smartSupported: true,
    firstSeen: "2026-03-02T09:12:00Z", lastSeen: MOCK_NOW,
  },
  {
    id: 2, serial: null, model: "Samsung SSD 870 EVO 1TB (caja USB-C)", firmware: null,
    bus: "usb", capacityBytes: 1 * TB, isBootDisk: false, smartSupported: false,
    firstSeen: "2026-05-18T17:40:00Z", lastSeen: MOCK_NOW,
  },
  {
    id: 3, serial: "WX42D81F7KLP", model: "WDC WD40EFRX-68N32N0", firmware: "82.00A82",
    bus: "sata", capacityBytes: 4 * TB, isBootDisk: false, smartSupported: true,
    firstSeen: "2026-01-11T08:00:00Z", lastSeen: MOCK_NOW,
  },
  {
    id: 4, serial: "ZA1QJ9RT", model: "Seagate BarraCuda ST2000DM008", firmware: "0001",
    bus: "sata", capacityBytes: 2 * TB, isBootDisk: false, smartSupported: true,
    firstSeen: "2026-11-15T19:05:00Z", lastSeen: MOCK_NOW,
  },
  {
    id: 5, serial: "4C530001170921113532", model: "SanDisk Ultra Fit 64GB", firmware: "1.00",
    bus: "usb", capacityBytes: 61.5 * GB, isBootDisk: false, smartSupported: false,
    firstSeen: "2026-10-30T12:00:00Z", lastSeen: MOCK_NOW,
  },
];

// ─────────────────────────────────────────────────────────────── Orígenes
export const sources: Source[] = [
  { id: 1, diskId: 1, kind: "device", path: "/dev/disk0", sizeBytes: 500.28 * GB, sha256: null, label: "Interno", addedAt: "2026-03-02T09:12:00Z" },
  { id: 2, diskId: 2, kind: "device", path: "/dev/disk4", sizeBytes: 1 * TB, sha256: null, label: "Archivo externo", addedAt: "2026-05-18T17:40:00Z" },
  { id: 3, diskId: 3, kind: "device", path: "/dev/sdb", sizeBytes: 4 * TB, sha256: null, label: "Disco del NAS", addedAt: "2026-01-11T08:00:00Z" },
  { id: 4, diskId: 4, kind: "device", path: "\\\\.\\PhysicalDrive1", sizeBytes: 2 * TB, sha256: null, label: "PC de la oficina", addedAt: "2026-11-15T19:05:00Z" },
  {
    id: 5, diskId: null, kind: "udif", path: "/Volumes/Archivo/imagenes/fotos-2019.dmg", sizeBytes: 64.2 * GB,
    sha256: "9f2c4e1b7a0d83f5c6e29b14d7a8f03e5b61c9d2a47f8e0b3c15d6a92e7f4b08", label: "Copia fotos 2019", addedAt: "2026-09-04T11:20:00Z",
  },
  {
    id: 6, diskId: null, kind: "raw_image", path: "/home/usuario/imagenes/portatil-home.img", sizeBytes: 256 * GB,
    sha256: null, label: "Home del portátil", addedAt: "2026-11-10T16:45:00Z",
  },
];

// ─────────────────────────────────────────────────────────────── Volúmenes
const base = {
  partitionIndex: 1, offsetBytes: 0, caseSensitive: false, journaled: true, journalState: "clean" as const,
  containerUuid: null, volumeUuid: null, role: null, encrypted: false, encryptionKind: "none" as const,
  isMounted: false, createdAtFs: null, modifiedAtFs: null,
};

export const volumes: Volume[] = [
  {
    ...base, id: 1, sourceId: 1, partitionIndex: 2, offsetBytes: 524_288_000, fsType: "apfs", name: "Macintosh HD",
    blockSize: 4096, totalBlocks: 122_138_133, freeBlocks: 41_022_310, fileCount: 512_884, folderCount: 118_302,
    journaled: false, journalState: "none", role: "system", containerUuid: "3B8F1C2E-6A71-4D0B-9F2C-81E4A6D07C55",
    volumeUuid: "A1C64E0F-2B9D-4E8A-B3F7-5D2C9E10A784", encrypted: true, encryptionKind: "hardware", isMounted: true,
    createdAtFs: "2026-03-02T09:30:00Z", modifiedAtFs: MOCK_NOW,
  },
  {
    ...base, id: 2, sourceId: 1, partitionIndex: 2, offsetBytes: 524_288_000, fsType: "apfs", name: "Macintosh HD - Data",
    blockSize: 4096, totalBlocks: 122_138_133, freeBlocks: 41_022_310, fileCount: 1_874_119, folderCount: 402_557,
    journaled: false, journalState: "none", role: "data", containerUuid: "3B8F1C2E-6A71-4D0B-9F2C-81E4A6D07C55",
    volumeUuid: "E7D2B0A9-5C14-4F63-8A2E-0B9F6C3D1E28", encrypted: true, encryptionKind: "hardware", isMounted: true,
    createdAtFs: "2026-03-02T09:30:00Z", modifiedAtFs: MOCK_NOW,
  },
  {
    ...base, id: 3, sourceId: 2, partitionIndex: 2, offsetBytes: 209_735_680, fsType: "hfsplus", name: "Archivo Fotos",
    blockSize: 4096, totalBlocks: 244_071_423, freeBlocks: 61_208_904, fileCount: 1_206_447, folderCount: 38_912,
    createdAtFs: "2019-06-21T18:02:11Z", modifiedAtFs: "2026-11-16T22:14:09Z",
  },
  {
    ...base, id: 4, sourceId: 3, fsType: "ext4", name: "nas-datos", offsetBytes: 1_048_576,
    blockSize: 4096, totalBlocks: 976_754_176, freeBlocks: 288_310_455, fileCount: null, folderCount: null,
    caseSensitive: true, encrypted: true, encryptionKind: "software",
    createdAtFs: "2026-01-11T08:20:00Z", modifiedAtFs: "2026-11-17T03:00:12Z",
  },
  {
    ...base, id: 5, sourceId: 4, partitionIndex: 3, offsetBytes: 290_455_552, fsType: "ntfs", name: "Windows",
    blockSize: 4096, totalBlocks: 488_303_103, freeBlocks: 202_118_640, fileCount: null, folderCount: null,
    journalState: "pending", encrypted: true, encryptionKind: "software",
    createdAtFs: "2024-02-08T10:11:00Z", modifiedAtFs: "2026-11-15T18:58:40Z",
  },
  {
    ...base, id: 6, sourceId: 5, partitionIndex: 1, offsetBytes: 20_480, fsType: "hfsplus", name: "Fotos 2019",
    blockSize: 4096, totalBlocks: 15_673_339, freeBlocks: 902_117, fileCount: 84_310, folderCount: 2_118,
    createdAtFs: "2019-01-03T09:00:00Z", modifiedAtFs: "2020-02-14T20:31:00Z",
  },
  {
    ...base, id: 7, sourceId: 6, fsType: "ext4", name: "home", offsetBytes: 1_048_576,
    blockSize: 4096, totalBlocks: 62_499_840, freeBlocks: 19_004_233, fileCount: 640_221, folderCount: 71_090,
    caseSensitive: true, encrypted: true, encryptionKind: "software",
    createdAtFs: "2025-04-01T07:45:00Z", modifiedAtFs: "2026-11-09T23:59:10Z",
  },
];

// ─────────────────────────────────────────────────────────────── Cifrado
export const encryptionLayers: EncryptionLayer[] = [
  { id: 1, volumeId: 1, scheme: "hardware", cipher: null, headerOk: true, backupHeadersOk: true, protectors: [], unlocked: true, detectedAt: MOCK_NOW },
  { id: 2, volumeId: 2, scheme: "hardware", cipher: null, headerOk: true, backupHeadersOk: true, protectors: [], unlocked: true, detectedAt: MOCK_NOW },
  { id: 3, volumeId: 4, scheme: "luks2", cipher: "aes-xts-plain64 (512 bits)", headerOk: true, backupHeadersOk: false, protectors: ["password", "keyfile"], unlocked: false, detectedAt: "2026-11-17T09:58:00Z" },
  { id: 4, volumeId: 5, scheme: "bitlocker", cipher: "XTS-AES 128", headerOk: true, backupHeadersOk: true, protectors: ["tpm", "recovery"], unlocked: false, detectedAt: "2026-11-15T19:06:00Z" },
  { id: 5, volumeId: 7, scheme: "fscrypt", cipher: "AES-256-XTS / AES-256-CTS", headerOk: true, backupHeadersOk: true, protectors: ["password"], unlocked: false, detectedAt: "2026-11-10T16:50:00Z" },
];

// ─────────────────────────────────────────────────────────────── Capacidades por formato
export const capabilitiesByFs: Record<Volume["fsType"], Capabilities> = {
  hfsplus: { scan: "yes", recover: "yes", rebuild: "yes", writeInPlace: "yes" },
  hfsx: { scan: "yes", recover: "yes", rebuild: "yes", writeInPlace: "yes" },
  apfs: { scan: "yes", recover: "yes", rebuild: "experimental", writeInPlace: "no" },
  ntfs: { scan: "yes", recover: "yes", rebuild: "yes", writeInPlace: "experimental" },
  ext4: { scan: "yes", recover: "yes", rebuild: "yes", writeInPlace: "yes" },
};

// ─────────────────────────────────────────────────────────────── Sesiones
export const sessions: Session[] = [
  {
    id: 1, volumeId: 3, mode: "image", status: "rebuilt", unlockedWith: "none", capabilities: capabilitiesByFs.hfsplus,
    startedAt: "2026-11-16T22:20:00Z", finishedAt: "2026-11-16T22:51:37Z", catalogNodesTotal: 48_211,
    catalogNodesDamaged: 37, fragmentationPct: 41.8, appVersion: "0.1.0", error: null,
  },
  {
    id: 2, volumeId: 6, mode: "image", status: "exported", unlockedWith: "none", capabilities: capabilitiesByFs.hfsplus,
    startedAt: "2026-09-04T11:25:00Z", finishedAt: "2026-09-04T11:39:02Z", catalogNodesTotal: 3_904,
    catalogNodesDamaged: 2, fragmentationPct: 12.4, appVersion: "0.1.0", error: null,
  },
  {
    id: 3, volumeId: 5, mode: "image", status: "cancelled", unlockedWith: "none", capabilities: capabilitiesByFs.ntfs,
    startedAt: "2026-11-15T19:10:00Z", finishedAt: "2026-11-15T19:10:41Z", catalogNodesTotal: null,
    catalogNodesDamaged: null, fragmentationPct: null, appVersion: "0.1.0",
    error: "Volumen BitLocker bloqueado: solo protector TPM disponible en este equipo. Introduce la clave de recuperación.",
  },
  {
    id: 4, volumeId: 7, mode: "image", status: "scanned", unlockedWith: "none", capabilities: capabilitiesByFs.ext4,
    startedAt: "2026-11-10T17:00:00Z", finishedAt: "2026-11-10T17:08:55Z", catalogNodesTotal: 711_311,
    catalogNodesDamaged: 4, fragmentationPct: 6.1, appVersion: "0.1.0", error: null,
  },
  {
    id: 5, volumeId: 3, mode: "direct", status: "failed", unlockedWith: "none", capabilities: capabilitiesByFs.hfsplus,
    startedAt: "2026-11-17T08:02:00Z", finishedAt: null, catalogNodesTotal: 48_211,
    catalogNodesDamaged: 37, fragmentationPct: 41.8, appVersion: "0.1.0",
    error: "La caja USB se desconectó durante la escritura (bloque 1 412 de 2 980). La transacción puede revertirse.",
  },
];

// ─────────────────────────────────────────────────────────────── Hallazgos
let fid = 0;
const f = (s: Omit<Finding, "id">): Finding => ({ id: ++fid, ...s });

export const findings: Finding[] = [
  f({ sessionId: 1, code: "HFS_BTREE_KEY_ORDER", severity: "critical", objectType: "node", cnid: null, nodeNumber: 18_204, path: null, message: "Claves fuera de orden en un nodo hoja del catálogo (registros 7–12).", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_BTREE_LINK_BROKEN", severity: "critical", objectType: "node", cnid: null, nodeNumber: 18_205, path: null, message: "El enlace al nodo siguiente apunta a un nodo libre (fLink = 40 977).", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_NODE_UNREADABLE", severity: "critical", objectType: "node", cnid: null, nodeNumber: 22_610, path: null, message: "Nodo con descriptor inválido; sus 23 registros se recuperaron por barrido.", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_ORPHAN_RECORD", severity: "critical", objectType: "folder", cnid: 402_118, nodeNumber: null, path: "/Fotos/2023/Viaje Lisboa", message: "Carpeta sin thread record: no se puede ubicar en la jerarquía.", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_ORPHAN_RECORD", severity: "critical", objectType: "file", cnid: 402_131, nodeNumber: null, path: "/Fotos/2023/Viaje Lisboa/IMG_4471.HEIC", message: "Archivo cuyo padre no existe en el catálogo.", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_VALENCE_MISMATCH", severity: "minor", objectType: "folder", cnid: 18_442, nodeNumber: null, path: "/Fotos/2022", message: "La carpeta declara 214 elementos y contiene 211.", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_VALENCE_MISMATCH", severity: "minor", objectType: "folder", cnid: 77_003, nodeNumber: null, path: "/Proyectos/Catálogo primavera", message: "La carpeta declara 58 elementos y contiene 59.", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_EXTENT_OVERLAP", severity: "critical", objectType: "extent", cnid: 391_877, nodeNumber: null, path: "/Vídeo/Boda Ana y Luis/clip_0032.mov", message: "El extent 3 (bloques 88 120 330–88 121 101) se solapa con otro archivo.", fixedInRebuild: false }),
  f({ sessionId: 1, code: "HFS_EXTENT_OUT_OF_RANGE", severity: "critical", objectType: "extent", cnid: 391_902, nodeNumber: null, path: "/Vídeo/Boda Ana y Luis/clip_0047.mov", message: "Extent fuera de los límites del volumen; los datos del archivo están incompletos.", fixedInRebuild: false }),
  f({ sessionId: 1, code: "HFS_BITMAP_MISMATCH", severity: "minor", objectType: "bitmap", cnid: null, nodeNumber: null, path: null, message: "12 044 bloques marcados como usados no pertenecen a ningún archivo.", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_FILE_COUNT", severity: "minor", objectType: "header", cnid: null, nodeNumber: null, path: null, message: "El Volume Header indica 1 206 410 archivos; el catálogo contiene 1 206 447.", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_NAME_NORMALIZATION", severity: "cosmetic", objectType: "file", cnid: 233_510, nodeNumber: null, path: "/Documentos/Facturación/2024/resumen.pdf", message: "Nombre con forma Unicode no descompuesta; se normaliza en la reconstrucción.", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_BTREE_DEPTH", severity: "cosmetic", objectType: "header", cnid: null, nodeNumber: 0, path: null, message: "Profundidad del árbol 5 (óptima 4) por nodos medio vacíos.", fixedInRebuild: true }),
  f({ sessionId: 1, code: "HFS_DATE_INVALID", severity: "cosmetic", objectType: "file", cnid: 300_014, nodeNumber: null, path: "/Fotos/2019/escaneos/abuela_1962.tif", message: "Fecha de creación posterior a la de modificación.", fixedInRebuild: false }),

  f({ sessionId: 2, code: "HFS_VALENCE_MISMATCH", severity: "minor", objectType: "folder", cnid: 2_201, nodeNumber: null, path: "/Navidad", message: "La carpeta declara 412 elementos y contiene 410.", fixedInRebuild: true }),
  f({ sessionId: 2, code: "HFS_BTREE_DEPTH", severity: "cosmetic", objectType: "header", cnid: null, nodeNumber: 0, path: null, message: "Nodos del catálogo ocupados al 38 % de media.", fixedInRebuild: true }),

  f({ sessionId: 3, code: "CRYPT_TPM_ONLY_NO_RECOVERY", severity: "critical", objectType: "encryption", cnid: null, nodeNumber: null, path: null, message: "BitLocker con protector TPM: fuera del equipo original solo se puede abrir con la clave de recuperación de 48 dígitos.", fixedInRebuild: false }),

  f({ sessionId: 4, code: "EXT4_HTREE_BAD", severity: "minor", objectType: "folder", cnid: 1_310_722, nodeNumber: null, path: "/usuario/.cache/thumbnails/large", message: "Índice htree con hash fuera de rango en el bloque 3.", fixedInRebuild: false }),
  f({ sessionId: 4, code: "EXT4_LINKCOUNT", severity: "minor", objectType: "file", cnid: 1_442_908, nodeNumber: null, path: "/usuario/proyectos/api/.git/objects/pack/pack-1e0c.idx", message: "El inodo indica 2 enlaces y tiene 1.", fixedInRebuild: false }),
  f({ sessionId: 4, code: "EXT4_ORPHAN_INODE", severity: "minor", objectType: "file", cnid: 1_502_377, nodeNumber: null, path: null, message: "Inodo en uso sin entrada de directorio (se movería a lost+found).", fixedInRebuild: false }),
  f({ sessionId: 4, code: "CRYPT_FSCRYPT_LOCKED_NAMES", severity: "cosmetic", objectType: "encryption", cnid: null, nodeNumber: null, path: "/usuario/Privado", message: "Directorio fscrypt bloqueado: nombres cifrados. La estructura se puede verificar igualmente.", fixedInRebuild: false }),
];

// ─────────────────────────────────────────────────────────────── Reconstrucciones
export const rebuilds: Rebuild[] = [
  {
    id: 1, sessionId: 1, status: "done", target: "new_image", experimental: false,
    startedAt: "2026-11-16T22:40:12Z", finishedAt: "2026-11-16T22:51:37Z", nodesBefore: 48_211, nodesAfter: 31_877,
    recoveredCount: 41, lostCount: 0, outputPath: "/Volumes/Destino/dirforge/archivo-fotos-reparado.img",
    outputSha256: "c41e0a9d7b3f25e8d6014ab9f3c2e7d0185b6fa94c2d3e81a07b5f9c6d2e4a13", verified: true,
    backupMetadataPath: "/Volumes/Destino/dirforge/backups/archivo-fotos-20261116.meta.zst",
  },
  {
    id: 2, sessionId: 2, status: "done", target: "new_image", experimental: false,
    startedAt: "2026-09-04T11:33:00Z", finishedAt: "2026-09-04T11:39:02Z", nodesBefore: 3_904, nodesAfter: 1_766,
    recoveredCount: 0, lostCount: 0, outputPath: "/Volumes/Archivo/imagenes/fotos-2019-reparada.img",
    outputSha256: "07be33f1a9d4c62e85f0b17a3d9c4e26f1a8b05d7c3e92f416ab0d8e5c7f2391", verified: true,
    backupMetadataPath: "/Volumes/Archivo/imagenes/backups/fotos-2019.meta.zst",
  },
  {
    id: 3, sessionId: 5, status: "failed", target: "in_place", experimental: false,
    startedAt: "2026-11-17T08:05:00Z", finishedAt: null, nodesBefore: 48_211, nodesAfter: 31_877,
    recoveredCount: 41, lostCount: 0, outputPath: null, outputSha256: null, verified: false,
    backupMetadataPath: "/Users/usuario/DirForge/backups/archivo-fotos-20261117.meta.zst",
  },
];

export const diffEntries: DiffEntry[] = [
  { id: 1, rebuildId: 1, cnid: 402_118, change: "recovered", oldPath: null, newPath: "/Fotos/2023/Viaje Lisboa", itemType: "folder", sizeBytes: null },
  { id: 2, rebuildId: 1, cnid: 402_131, change: "recovered", oldPath: null, newPath: "/Fotos/2023/Viaje Lisboa/IMG_4471.HEIC", itemType: "file", sizeBytes: 3_412_880 },
  { id: 3, rebuildId: 1, cnid: 402_132, change: "recovered", oldPath: null, newPath: "/Fotos/2023/Viaje Lisboa/IMG_4472.HEIC", itemType: "file", sizeBytes: 2_987_004 },
  { id: 4, rebuildId: 1, cnid: 402_133, change: "recovered", oldPath: null, newPath: "/Fotos/2023/Viaje Lisboa/IMG_4475.MOV", itemType: "file", sizeBytes: 148_220_416 },
  { id: 5, rebuildId: 1, cnid: 512_004, change: "moved_to_recovered", oldPath: null, newPath: "/Recovered Items/presupuesto_v3.numbers", itemType: "file", sizeBytes: 402_118 },
  { id: 6, rebuildId: 1, cnid: 512_005, change: "moved_to_recovered", oldPath: null, newPath: "/Recovered Items/Sin título 12.txt", itemType: "file", sizeBytes: 1_024 },
  { id: 7, rebuildId: 1, cnid: 18_442, change: "valence_fixed", oldPath: "/Fotos/2022", newPath: "/Fotos/2022", itemType: "folder", sizeBytes: null },
  { id: 8, rebuildId: 1, cnid: 77_003, change: "valence_fixed", oldPath: "/Proyectos/Catálogo primavera", newPath: "/Proyectos/Catálogo primavera", itemType: "folder", sizeBytes: null },
  { id: 9, rebuildId: 1, cnid: 233_510, change: "renamed", oldPath: "/Documentos/Facturación/2024/resumen.pdf", newPath: "/Documentos/Facturación/2024/resumen.pdf", itemType: "file", sizeBytes: 88_310 },
  { id: 10, rebuildId: 1, cnid: 391_877, change: "unchanged_flagged", oldPath: "/Vídeo/Boda Ana y Luis/clip_0032.mov", newPath: "/Vídeo/Boda Ana y Luis/clip_0032.mov", itemType: "file", sizeBytes: 2_140_331_008 },
  { id: 11, rebuildId: 1, cnid: 391_902, change: "unchanged_flagged", oldPath: "/Vídeo/Boda Ana y Luis/clip_0047.mov", newPath: "/Vídeo/Boda Ana y Luis/clip_0047.mov", itemType: "file", sizeBytes: 1_806_770_176 },
];

// ─────────────────────────────────────────────────────────────── Escritura directa
export const writeTransactions: WriteTransaction[] = [
  {
    id: 1, rebuildId: 3, devicePath: "/dev/disk4s2", deviceSerial: null, status: "interrupted",
    undoLogPath: "/Users/usuario/DirForge/undo/tx-0001.undo", blocksTotal: 2_980, blocksWritten: 1_412,
    startedAt: "2026-11-17T08:06:10Z", finishedAt: null, helperVersion: "0.1.0",
  },
];

// ─────────────────────────────────────────────────────────────── S.M.A.R.T.
/** Pseudoaleatorio determinista para generar históricos estables. */
function seeded(seed: number) {
  let s = seed;
  return () => ((s = (s * 1_103_515_245 + 12_345) % 2_147_483_648) / 2_147_483_648);
}

function attrs(list: [number, string, number, number, number, number][]): SmartAttribute[] {
  return list.map(([attrId, name, value, worst, threshold, rawValue]) => ({ attrId, name, value, worst, threshold, rawValue }));
}

/** Últimos 30 días, una lectura diaria a las 03:00 UTC. */
function history(
  diskId: number,
  idBase: number,
  build: (day: number, rnd: () => number) => Omit<SmartSnapshot, "id" | "diskId" | "takenAt">,
): SmartSnapshot[] {
  const rnd = seeded(diskId * 97);
  const end = Date.parse("2026-11-17T03:00:00Z");
  return Array.from({ length: 30 }, (_, i) => ({
    id: idBase + i,
    diskId,
    takenAt: new Date(end - (29 - i) * 86_400_000).toISOString(),
    ...build(i, rnd),
  }));
}

export const smartSnapshots: SmartSnapshot[] = [
  // NVMe interno sano
  ...history(1, 1_000, (d, r) => ({
    overall: "passed", temperatureC: Math.round(34 + r() * 8), powerOnHours: 6_120 + d * 9,
    percentageUsed: 4, attributes: [],
  })),
  // HDD del NAS: reasignados creciendo de 0 a 24
  ...history(3, 2_000, (d, r) => {
    const realloc = d < 12 ? 0 : Math.min(24, Math.round((d - 11) ** 1.25));
    const pending = d < 20 ? 0 : d < 26 ? 1 : 3;
    return {
      overall: realloc > 0 || pending > 0 ? "warning" : "passed",
      temperatureC: Math.round(39 + r() * 6), powerOnHours: 31_870 + d * 24, percentageUsed: null,
      attributes: attrs([
        [1, "Raw_Read_Error_Rate", 200, 200, 51, d > 18 ? 4 : 0],
        [3, "Spin_Up_Time", 177, 175, 21, 8_141],
        [5, "Reallocated_Sector_Ct", realloc > 0 ? 199 : 200, realloc > 0 ? 199 : 200, 140, realloc],
        [9, "Power_On_Hours", 57, 57, 0, 31_870 + d * 24],
        [10, "Spin_Retry_Count", 100, 100, 0, 0],
        [12, "Power_Cycle_Count", 100, 100, 0, 77],
        [194, "Temperature_Celsius", 110, 101, 0, Math.round(39 + r() * 6)],
        [196, "Reallocated_Event_Count", 200, 200, 0, Math.ceil(realloc / 3)],
        [197, "Current_Pending_Sector", 200, 200, 0, pending],
        [198, "Offline_Uncorrectable", 100, 253, 0, 0],
        [199, "UDMA_CRC_Error_Count", 200, 200, 0, 0],
        [200, "Multi_Zone_Error_Rate", 200, 200, 0, d > 22 ? 2 : 0],
      ]),
    };
  }),
  // HDD de PC fallando: solo dos lecturas (se conectó hace dos días)
  {
    id: 3_000, diskId: 4, takenAt: "2026-11-15T19:06:00Z", overall: "failing", temperatureC: 47,
    powerOnHours: 22_408, percentageUsed: null,
    attributes: attrs([
      [1, "Raw_Read_Error_Rate", 61, 44, 6, 191_388_102],
      [5, "Reallocated_Sector_Ct", 8, 8, 10, 3_872],
      [9, "Power_On_Hours", 75, 75, 0, 22_408],
      [187, "Reported_Uncorrect", 1, 1, 0, 412],
      [194, "Temperature_Celsius", 47, 55, 0, 47],
      [197, "Current_Pending_Sector", 100, 100, 0, 288],
      [198, "Offline_Uncorrectable", 100, 100, 0, 288],
    ]),
  },
  {
    id: 3_001, diskId: 4, takenAt: "2026-11-17T03:00:00Z", overall: "failing", temperatureC: 46,
    powerOnHours: 22_410, percentageUsed: null,
    attributes: attrs([
      [1, "Raw_Read_Error_Rate", 58, 44, 6, 201_004_877],
      [5, "Reallocated_Sector_Ct", 7, 7, 10, 3_960],
      [9, "Power_On_Hours", 75, 75, 0, 22_410],
      [187, "Reported_Uncorrect", 1, 1, 0, 431],
      [194, "Temperature_Celsius", 46, 55, 0, 46],
      [197, "Current_Pending_Sector", 100, 100, 0, 304],
      [198, "Offline_Uncorrectable", 100, 100, 0, 304],
    ]),
  },
];

export const alerts: Alert[] = [
  { id: 1, diskId: 4, rule: "SMART_FAILING", severity: "critical", message: "Reallocated_Sector_Ct por debajo del umbral (7 ≤ 10). Clona o recupera los datos antes de cualquier reparación.", createdAt: "2026-11-15T19:06:05Z", acknowledgedAt: null },
  { id: 2, diskId: 4, rule: "PENDING_SECTORS", severity: "critical", message: "304 sectores pendientes de reasignación (+16 en 2 días).", createdAt: "2026-11-17T03:00:04Z", acknowledgedAt: null },
  { id: 3, diskId: 3, rule: "REALLOCATED_INCREASE", severity: "minor", message: "Sectores reasignados: 0 → 24 en los últimos 18 días.", createdAt: "2026-11-17T03:00:02Z", acknowledgedAt: null },
  { id: 4, diskId: 3, rule: "PENDING_SECTORS", severity: "minor", message: "3 sectores pendientes de reasignación.", createdAt: "2026-11-14T03:00:02Z", acknowledgedAt: "2026-11-14T09:12:00Z" },
  { id: 5, diskId: 1, rule: "TEMP_HIGH", severity: "cosmetic", message: "Temperatura puntual de 61 °C durante una exportación.", createdAt: "2026-11-02T15:40:00Z", acknowledgedAt: "2026-11-02T16:00:00Z" },
];

// ─────────────────────────────────────────────────────────────── Informes, medios y ajustes
export const reports: Report[] = [
  { id: 1, sessionId: 1, format: "html", path: "/Volumes/Destino/dirforge/informes/archivo-fotos-20261116.html", generatedAt: "2026-11-16T22:52:00Z" },
  { id: 2, sessionId: 2, format: "html", path: "/Volumes/Archivo/imagenes/informes/fotos-2019.html", generatedAt: "2026-09-04T11:40:00Z" },
  { id: 3, sessionId: 2, format: "json", path: "/Volumes/Archivo/imagenes/informes/fotos-2019.json", generatedAt: "2026-09-04T11:40:00Z" },
];

export const bootMedia: BootMedia[] = [
  {
    id: 1, deviceSerial: "4C530001170921113532", imageVersion: "live-0.1.0", arch: "x86_64", kind: "linux_live",
    imageSha256: "5e0d7a2c9f14b83e6a01c7d42f9be3a05d8c61f7e2b49a03c5d17e8f6a2b9c40", createdAt: "2026-10-30T12:20:00Z", verified: true,
  },
];

export const settings: Settings = {
  outputDir: "/Volumes/Destino/dirforge",
  language: "es",
  theme: "system",
  logLevel: "info",
  exportEncryptedByDefault: true,
};

// ─────────────────────────────────────────────────────────────── Árboles (comparación)
/** Fragmento del árbol original de «Archivo Fotos» (sesión 1). */
export const originalTree: TreeNode = {
  cnid: 2, name: "Archivo Fotos", type: "folder", children: [
    { cnid: 16, name: "Documentos", type: "folder", children: [
      { cnid: 233_400, name: "Facturación", type: "folder", children: [
        { cnid: 233_401, name: "2024", type: "folder", children: [
          { cnid: 233_510, name: "resumen.pdf", type: "file", sizeBytes: 88_310 },
          { cnid: 233_511, name: "facturas_q4.xlsx", type: "file", sizeBytes: 241_664 },
        ] },
      ] },
    ] },
    { cnid: 17, name: "Fotos", type: "folder", children: [
      { cnid: 18_442, name: "2022", type: "folder", children: [
        { cnid: 18_500, name: "IMG_0001.HEIC", type: "file", sizeBytes: 2_201_004 },
      ] },
      { cnid: 18_900, name: "2023", type: "folder", children: [] },
    ] },
    { cnid: 19, name: "Vídeo", type: "folder", children: [
      { cnid: 391_800, name: "Boda Ana y Luis", type: "folder", children: [
        { cnid: 391_877, name: "clip_0032.mov", type: "file", sizeBytes: 2_140_331_008 },
        { cnid: 391_902, name: "clip_0047.mov", type: "file", sizeBytes: 1_806_770_176 },
      ] },
    ] },
  ],
};

/** El mismo fragmento tras la reconstrucción, con los cambios marcados. */
export const rebuiltTree: TreeNode = {
  cnid: 2, name: "Archivo Fotos", type: "folder", children: [
    { cnid: 16, name: "Documentos", type: "folder", children: [
      { cnid: 233_400, name: "Facturación", type: "folder", children: [
        { cnid: 233_401, name: "2024", type: "folder", children: [
          { cnid: 233_510, name: "resumen.pdf", type: "file", sizeBytes: 88_310, change: "renamed" },
          { cnid: 233_511, name: "facturas_q4.xlsx", type: "file", sizeBytes: 241_664 },
        ] },
      ] },
    ] },
    { cnid: 17, name: "Fotos", type: "folder", children: [
      { cnid: 18_442, name: "2022", type: "folder", change: "valence_fixed", children: [
        { cnid: 18_500, name: "IMG_0001.HEIC", type: "file", sizeBytes: 2_201_004 },
      ] },
      { cnid: 18_900, name: "2023", type: "folder", children: [
        { cnid: 402_118, name: "Viaje Lisboa", type: "folder", change: "recovered", children: [
          { cnid: 402_131, name: "IMG_4471.HEIC", type: "file", sizeBytes: 3_412_880, change: "recovered" },
          { cnid: 402_132, name: "IMG_4472.HEIC", type: "file", sizeBytes: 2_987_004, change: "recovered" },
          { cnid: 402_133, name: "IMG_4475.MOV", type: "file", sizeBytes: 148_220_416, change: "recovered" },
        ] },
      ] },
    ] },
    { cnid: 19, name: "Vídeo", type: "folder", children: [
      { cnid: 391_800, name: "Boda Ana y Luis", type: "folder", children: [
        { cnid: 391_877, name: "clip_0032.mov", type: "file", sizeBytes: 2_140_331_008, change: "unchanged_flagged" },
        { cnid: 391_902, name: "clip_0047.mov", type: "file", sizeBytes: 1_806_770_176, change: "unchanged_flagged" },
      ] },
    ] },
    { cnid: 512_000, name: "Recovered Items", type: "folder", change: "recovered", children: [
      { cnid: 512_004, name: "presupuesto_v3.numbers", type: "file", sizeBytes: 402_118, change: "moved_to_recovered" },
      { cnid: 512_005, name: "Sin título 12.txt", type: "file", sizeBytes: 1_024, change: "moved_to_recovered" },
    ] },
  ],
};
