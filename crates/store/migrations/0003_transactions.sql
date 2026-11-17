-- BlockSmith · F5: varias transacciones por reconstrucción (reintentos tras cancelar
-- o revertir), modo de ejecución, estado «finalizada» y mensaje de error.

CREATE TABLE write_transactions_new (
  id              INTEGER PRIMARY KEY,
  rebuild_id      INTEGER NOT NULL REFERENCES rebuilds(id) ON DELETE CASCADE,
  device_path     TEXT NOT NULL,
  device_serial   TEXT,
  status          TEXT NOT NULL CHECK (status IN ('prepared','writing','committed','finalized','verified','rolled_back','interrupted','failed')),
  undo_log_path   TEXT NOT NULL,
  blocks_total    INTEGER NOT NULL,
  blocks_written  INTEGER NOT NULL DEFAULT 0,
  started_at      TEXT NOT NULL,
  finished_at     TEXT,
  helper_version  TEXT NOT NULL,
  mode            TEXT NOT NULL DEFAULT 'in_process' CHECK (mode IN ('in_process','helper')),
  error           TEXT
);
INSERT INTO write_transactions_new (id, rebuild_id, device_path, device_serial, status, undo_log_path, blocks_total,
  blocks_written, started_at, finished_at, helper_version)
  SELECT id, rebuild_id, device_path, device_serial, status, undo_log_path, blocks_total, blocks_written, started_at,
  finished_at, helper_version FROM write_transactions;
DROP TABLE write_transactions;
ALTER TABLE write_transactions_new RENAME TO write_transactions;
CREATE INDEX idx_tx_rebuild ON write_transactions(rebuild_id);
CREATE INDEX idx_tx_status ON write_transactions(status);
