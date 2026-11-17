import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { JobFinishedEvent, JobProgressEvent } from "../types/api";
import type { Backend } from "./backend";

/** API real: comandos de app/src-tauri/src/commands. */
export const tauriBackend: Backend = {
  simulated: false,
  appInfo: () => invoke("app_info"),

  async pickImage() {
    const r = await open({
      multiple: false,
      directory: false,
      title: "Abrir imagen de disco",
      filters: [
        { name: "Imágenes de disco", extensions: ["dmg", "img", "iso", "raw", "bin", "dd", "hfs"] },
        { name: "Todos los archivos", extensions: ["*"] },
      ],
    });
    return typeof r === "string" ? r : null;
  },
  async pickSavePath(suggested) {
    const r = await save({ title: "Guardar imagen reparada", defaultPath: suggested });
    return r ?? null;
  },
  async pickFolder(current) {
    const r = await open({ directory: true, multiple: false, defaultPath: current, title: "Carpeta de salida" });
    return typeof r === "string" ? r : null;
  },

  openImage: (path) => invoke("open_image", { path }),
  listSources: () => invoke("list_sources"),
  removeSource: (id) => invoke("remove_source", { id }),
  getVolume: (id) => invoke("get_volume", { id }),
  listSessions: () => invoke("list_sessions"),
  getSession: (id) => invoke("get_session", { id }),
  getFindings: (sessionId) => invoke("get_findings", { sessionId }),
  getRebuild: (id) => invoke("get_rebuild", { id }),
  getDiff: (rebuildId) => invoke("get_diff", { rebuildId }),
  treeChildren: (side, id, parent) => invoke("tree_children", { side, id, parent }),
  suggestExportPath: (rebuildId) => invoke("suggest_export_path", { rebuildId }),
  startScan: (volumeId) => invoke("start_scan", { volumeId }),
  startPlan: (sessionId) => invoke("start_plan", { sessionId }),
  startExport: (rebuildId, outPath) => invoke("start_export", { rebuildId, outPath }),
  cancelJob: (jobId) => invoke("cancel_job", { jobId }),
  writeReport: (sessionId, format, path) => invoke("write_report", { sessionId, format, path: path ?? null }),
  listReports: (sessionId) => invoke("list_reports", { sessionId }),
  getSettings: () => invoke("get_settings"),
  setSettings: (settings) => invoke("set_settings", { settings }),

  healthDisks: () => invoke("health_disks"),
  healthDisk: (id) => invoke("health_disk", { id }),
  healthStatus: () => invoke("health_status"),
  startHealthRead: () => invoke("start_health_read"),
  listAlerts: (openOnly) => invoke("list_alerts", { openOnly }),
  ackAlert: (id) => invoke("ack_alert", { id }),
  getAlertRules: () => invoke("get_alert_rules"),
  setAlertRule: (rule) => invoke("set_alert_rule", { rule }),
  preflight: (rebuildId) => invoke("preflight", { rebuildId }),
  startRepair: (rebuildId, confirm) => invoke("start_repair", { rebuildId, confirm }),
  listTransactions: () => invoke("list_transactions"),
  startTxRollback: (txId, force) => invoke("start_tx_rollback", { txId, force }),
  startTxComplete: (txId) => invoke("start_tx_complete", { txId }),
  listOpenDevices: () => invoke("list_open_devices"),
  onHealthUpdated: (cb) => listen("health-updated", () => cb()),

  onJobProgress: (cb) => listen<JobProgressEvent>("job-progress", (e) => cb(e.payload)),
  onJobFinished: (cb) => listen<JobFinishedEvent>("job-finished", (e) => cb(e.payload)),
};
