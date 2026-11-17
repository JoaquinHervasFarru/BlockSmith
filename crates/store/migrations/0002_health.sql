-- BlockSmith · F4: identificación estable de discos y estado de la última lectura.

ALTER TABLE disks ADD COLUMN device_key TEXT;
ALTER TABLE disks ADD COLUMN path TEXT;
-- ok | unsupported | permission | failed
ALTER TABLE disks ADD COLUMN read_status TEXT;
ALTER TABLE disks ADD COLUMN read_note TEXT;
ALTER TABLE disks ADD COLUMN rotational INTEGER;
ALTER TABLE disks ADD COLUMN removable INTEGER NOT NULL DEFAULT 0;
CREATE UNIQUE INDEX idx_disks_device_key ON disks(device_key);

INSERT INTO alert_rules (rule, threshold_json, poll_interval_min) VALUES ('SMART_FAILING', '{}', 360);
