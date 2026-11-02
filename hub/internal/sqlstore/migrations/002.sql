CREATE TABLE IF NOT EXISTS usage_events (
  device_id TEXT NOT NULL REFERENCES devices(device_id) ON DELETE RESTRICT,
  record_id TEXT NOT NULL CHECK(length(record_id)=64 AND record_id NOT GLOB '*[^0-9a-f]*'),
  record_revision INTEGER NOT NULL CHECK(record_revision > 0),
  payload_hash TEXT NOT NULL CHECK(length(payload_hash)=64 AND payload_hash NOT GLOB '*[^0-9a-f]*'),
  payload_json TEXT NOT NULL CHECK(length(payload_json) <= 65536),
  received_at_ms INTEGER NOT NULL CHECK(received_at_ms >= 0),
  PRIMARY KEY(device_id, record_id)
);
CREATE INDEX IF NOT EXISTS ix_usage_events_device_received ON usage_events(device_id, received_at_ms);
CREATE INDEX IF NOT EXISTS ix_usage_events_record ON usage_events(record_id);
