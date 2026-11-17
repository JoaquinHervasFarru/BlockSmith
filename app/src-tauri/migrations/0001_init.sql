-- BlockSmith · esquema inicial (plan técnico v3, §8)
-- Nunca se almacenan claves, contraseñas ni claves de recuperación.

CREATE TABLE disks (
  id               INTEGER PRIMARY KEY,
  serial           TEXT,
  model            TEXT,
  firmware         TEXT,
  bus              TEXT NOT NULL CHECK (bus IN ('sata','nvme','usb','other')),
  capacity_bytes   INTEGER NOT NULL,
  is_boot_disk     INTEGER NOT NULL DEFAULT 0,
  smart_supported  INTEGER NOT NULL DEFAULT 0,
  first_seen       TEXT NOT NULL,
  last_seen        TEXT NOT NULL
);

CREATE TABLE sources (
  id          INTEGER PRIMARY KEY,
  disk_id     INTEGER REFERENCES disks(id) ON DELETE SET NULL,
  kind        TEXT NOT NULL CHECK (kind IN ('raw_image','udif','device')),
  path        TEXT NOT NULL,
  size_bytes  INTEGER NOT NULL,
  sha256      TEXT,
  label       TEXT,
  added_at    TEXT NOT NULL
);

CREATE TABLE volumes (
  id               INTEGER PRIMARY KEY,
  source_id        INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  partition_index  INTEGER,
  offset_bytes     INTEGER NOT NULL DEFAULT 0,
  length_bytes     INTEGER NOT NULL DEFAULT 0,
  fs_type          TEXT NOT NULL CHECK (fs_type IN ('hfsplus','hfsx','apfs','ntfs','ext4')),
  name             TEXT,
  block_size       INTEGER,
  total_blocks     INTEGER,
  free_blocks      INTEGER,
  file_count       INTEGER,
  folder_count     INTEGER,
  case_sensitive   INTEGER NOT NULL DEFAULT 0,
  journaled        INTEGER NOT NULL DEFAULT 0,
  journal_state    TEXT NOT NULL DEFAULT 'none' CHECK (journal_state IN ('clean','pending','none')),
  container_uuid   TEXT,
  volume_uuid      TEXT,
  role             TEXT,
  encrypted        INTEGER NOT NULL DEFAULT 0,
  encryption_kind  TEXT NOT NULL DEFAULT 'none' CHECK (encryption_kind IN ('none','software','hardware')),
  is_mounted       INTEGER NOT NULL DEFAULT 0,
  created_at_fs    TEXT,
  modified_at_fs   TEXT
);

CREATE TABLE encryption_layers (
  id                 INTEGER PRIMARY KEY,
  volume_id          INTEGER NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
  scheme             TEXT NOT NULL CHECK (scheme IN ('bitlocker','luks1','luks2','corestorage','apfs','fscrypt','veracrypt','hardware')),
  cipher             TEXT,
  header_ok          INTEGER NOT NULL DEFAULT 1,
  backup_headers_ok  INTEGER NOT NULL DEFAULT 1,
  protectors_json    TEXT NOT NULL DEFAULT '[]',   -- solo tipos: tpm|password|recovery|keyfile
  detected_at        TEXT NOT NULL
);

CREATE TABLE sessions (
  id                     INTEGER PRIMARY KEY,
  volume_id              INTEGER NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
  mode                   TEXT NOT NULL DEFAULT 'image' CHECK (mode IN ('image','direct')),
  status                 TEXT NOT NULL CHECK (status IN ('pending','scanning','scanned','rebuilding','rebuilt','exported','failed','cancelled')),
  unlocked_with          TEXT NOT NULL DEFAULT 'none' CHECK (unlocked_with IN ('password','recovery','keyfile','none')),
  capabilities_json      TEXT,
  started_at             TEXT NOT NULL,
  finished_at            TEXT,
  catalog_nodes_total    INTEGER,
  catalog_nodes_damaged  INTEGER,
  fragmentation_pct      REAL,
  snapshot_path          TEXT,
  app_version            TEXT NOT NULL,
  error                  TEXT
);

CREATE TABLE findings (
  id                INTEGER PRIMARY KEY,
  session_id        INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  code              TEXT NOT NULL,
  severity          TEXT NOT NULL CHECK (severity IN ('critical','minor','cosmetic')),
  object_type       TEXT NOT NULL,
  cnid              INTEGER,
  node_number       INTEGER,
  path              TEXT,
  message           TEXT NOT NULL,
  details_json      TEXT,
  fixed_in_rebuild  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE rebuilds (
  id                    INTEGER PRIMARY KEY,
  session_id            INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  status                TEXT NOT NULL,
  target                TEXT NOT NULL CHECK (target IN ('new_image','in_place')),
  experimental          INTEGER NOT NULL DEFAULT 0,
  started_at            TEXT NOT NULL,
  finished_at           TEXT,
  nodes_before          INTEGER,
  nodes_after           INTEGER,
  recovered_count       INTEGER,
  lost_count            INTEGER,
  output_path           TEXT,
  output_sha256         TEXT,
  verified              INTEGER NOT NULL DEFAULT 0,
  backup_metadata_path  TEXT
);
-- recovered_count/lost_count ya existen; experimental indica reconstrucciones sin garantías.

CREATE TABLE diff_entries (
  id          INTEGER PRIMARY KEY,
  rebuild_id  INTEGER NOT NULL REFERENCES rebuilds(id) ON DELETE CASCADE,
  cnid        INTEGER NOT NULL,
  change      TEXT NOT NULL CHECK (change IN ('recovered','moved_to_recovered','renamed','valence_fixed','dropped','data_truncated','data_cloned','unchanged_flagged')),
  old_path    TEXT,
  new_path    TEXT,
  item_type   TEXT NOT NULL CHECK (item_type IN ('file','folder')),
  size_bytes  INTEGER,
  note        TEXT
);

CREATE TABLE write_transactions (
  id              INTEGER PRIMARY KEY,
  rebuild_id      INTEGER NOT NULL UNIQUE REFERENCES rebuilds(id) ON DELETE CASCADE,
  device_path     TEXT NOT NULL,
  device_serial   TEXT,
  status          TEXT NOT NULL CHECK (status IN ('prepared','writing','committed','verified','rolled_back','interrupted')),
  undo_log_path   TEXT NOT NULL,
  blocks_total    INTEGER NOT NULL,
  blocks_written  INTEGER NOT NULL DEFAULT 0,
  started_at      TEXT NOT NULL,
  finished_at     TEXT,
  helper_version  TEXT NOT NULL
);

CREATE TABLE undo_blocks (
  id               INTEGER PRIMARY KEY,
  tx_id            INTEGER NOT NULL REFERENCES write_transactions(id) ON DELETE CASCADE,
  seq              INTEGER NOT NULL,
  offset_bytes     INTEGER NOT NULL,
  length           INTEGER NOT NULL,
  original_sha256  TEXT NOT NULL,
  new_sha256       TEXT NOT NULL,
  written          INTEGER NOT NULL DEFAULT 0,
  verified         INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE smart_snapshots (
  id               INTEGER PRIMARY KEY,
  disk_id          INTEGER NOT NULL REFERENCES disks(id) ON DELETE CASCADE,
  taken_at         TEXT NOT NULL,
  overall          TEXT NOT NULL CHECK (overall IN ('passed','warning','failing','unknown')),
  temperature_c    INTEGER,
  power_on_hours   INTEGER,
  percentage_used  INTEGER,
  raw_json         TEXT
);

CREATE TABLE smart_attributes (
  id           INTEGER PRIMARY KEY,
  snapshot_id  INTEGER NOT NULL REFERENCES smart_snapshots(id) ON DELETE CASCADE,
  attr_id      INTEGER NOT NULL,
  name         TEXT NOT NULL,
  value        INTEGER,
  worst        INTEGER,
  threshold    INTEGER,
  raw_value    INTEGER,
  flags        INTEGER
);

CREATE TABLE alerts (
  id               INTEGER PRIMARY KEY,
  disk_id          INTEGER NOT NULL REFERENCES disks(id) ON DELETE CASCADE,
  rule             TEXT NOT NULL,
  severity         TEXT NOT NULL CHECK (severity IN ('critical','minor','cosmetic')),
  message          TEXT NOT NULL,
  created_at       TEXT NOT NULL,
  acknowledged_at  TEXT
);

CREATE TABLE alert_rules (
  id                 INTEGER PRIMARY KEY,
  rule               TEXT NOT NULL UNIQUE,
  enabled            INTEGER NOT NULL DEFAULT 1,
  threshold_json     TEXT,
  poll_interval_min  INTEGER NOT NULL DEFAULT 360
);

CREATE TABLE reports (
  id            INTEGER PRIMARY KEY,
  session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  format        TEXT NOT NULL CHECK (format IN ('html','json')),
  path          TEXT NOT NULL,
  generated_at  TEXT NOT NULL
);

CREATE TABLE boot_media (
  id              INTEGER PRIMARY KEY,
  device_serial   TEXT,
  image_version   TEXT NOT NULL,
  image_sha256    TEXT NOT NULL,
  arch            TEXT NOT NULL CHECK (arch IN ('x86_64','arm64')),
  kind            TEXT NOT NULL CHECK (kind IN ('linux_live','macos_external')),
  created_at      TEXT NOT NULL,
  verified        INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE settings (
  key         TEXT PRIMARY KEY,
  value_json  TEXT NOT NULL
);

CREATE INDEX idx_findings_session_severity ON findings(session_id, severity);
CREATE INDEX idx_findings_cnid            ON findings(cnid);
CREATE INDEX idx_diff_rebuild_change      ON diff_entries(rebuild_id, change);
CREATE INDEX idx_sessions_volume_started  ON sessions(volume_id, started_at);
CREATE INDEX idx_smart_disk_taken         ON smart_snapshots(disk_id, taken_at);
CREATE INDEX idx_alerts_disk_ack          ON alerts(disk_id, acknowledged_at);
CREATE INDEX idx_undo_tx_seq              ON undo_blocks(tx_id, seq);
CREATE INDEX idx_volumes_source           ON volumes(source_id);
CREATE UNIQUE INDEX idx_sources_path      ON sources(path);

INSERT INTO alert_rules (rule, threshold_json, poll_interval_min) VALUES
  ('REALLOCATED_INCREASE',   '{"delta": 1}',            360),
  ('PENDING_SECTORS',        '{"min": 1}',              360),
  ('NVME_CRITICAL_WARNING',  '{"mask": 255}',           360),
  ('NVME_WEAR',              '{"percentage_used": 90}', 1440),
  ('TEMP_HIGH',              '{"celsius": 60}',         60);
