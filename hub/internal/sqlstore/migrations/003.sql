-- M1-05 append-only feed. A feed row is emitted for every accepted revision.
CREATE TABLE IF NOT EXISTS usage_change_feed (
  change_seq INTEGER PRIMARY KEY AUTOINCREMENT,
  device_id TEXT NOT NULL REFERENCES devices(device_id) ON DELETE RESTRICT,
  record_id TEXT NOT NULL CHECK(length(record_id)=64 AND record_id NOT GLOB '*[^0-9a-f]*'),
  record_revision INTEGER NOT NULL CHECK(record_revision > 0),
  payload_hash TEXT NOT NULL CHECK(length(payload_hash)=64 AND payload_hash NOT GLOB '*[^0-9a-f]*'),
  payload_json TEXT NOT NULL CHECK(length(payload_json) <= 65536),
  changed_at_ms INTEGER NOT NULL CHECK(changed_at_ms >= 0)
);
CREATE INDEX IF NOT EXISTS ix_usage_change_feed_device_seq ON usage_change_feed(device_id,change_seq);
-- Bootstrap pre-M1-05 current records so a new client starting at cursor 0 can hydrate.
INSERT INTO usage_change_feed(device_id,record_id,record_revision,payload_hash,payload_json,changed_at_ms)
SELECT device_id,record_id,record_revision,payload_hash,payload_json,received_at_ms FROM usage_events ORDER BY device_id,record_id;
