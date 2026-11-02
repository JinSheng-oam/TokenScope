CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS devices (
  device_id TEXT PRIMARY KEY CHECK(length(device_id) BETWEEN 8 AND 128 AND device_id NOT GLOB '*[^A-Za-z0-9_-]*' AND substr(device_id,1,1) GLOB '[A-Za-z0-9]'),
  display_name TEXT NOT NULL CHECK(length(display_name) BETWEEN 1 AND 256),
  os_name TEXT NOT NULL CHECK(os_name IN ('windows','macos','linux','other')),
  app_version TEXT NOT NULL CHECK(length(app_version) BETWEEN 1 AND 64 AND app_version NOT GLOB '*[^A-Za-z0-9.+_-]*'),
  time_zone TEXT NOT NULL CHECK(length(time_zone) BETWEEN 1 AND 64),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  last_seen_ms INTEGER,
  revoked_at_ms INTEGER
);
CREATE TABLE IF NOT EXISTS device_credentials (
  device_id TEXT NOT NULL REFERENCES devices(device_id) ON DELETE RESTRICT,
  token_sha256 TEXT NOT NULL UNIQUE CHECK(length(token_sha256) = 64 AND token_sha256 NOT GLOB '*[^0-9a-f]*'),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  revoked_at_ms INTEGER,
  PRIMARY KEY(device_id, token_sha256)
);
CREATE INDEX IF NOT EXISTS ix_device_credentials_active ON device_credentials(token_sha256, revoked_at_ms);
CREATE TABLE IF NOT EXISTS pairing_codes (
  code_sha256 TEXT PRIMARY KEY CHECK(length(code_sha256) = 64 AND code_sha256 NOT GLOB '*[^0-9a-f]*'),
  created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0),
  expires_at_ms INTEGER NOT NULL,
  consumed_at_ms INTEGER CHECK(consumed_at_ms IS NULL OR consumed_at_ms >= created_at_ms),
  CHECK(expires_at_ms > created_at_ms)
);
CREATE INDEX IF NOT EXISTS ix_pairing_codes_expiry ON pairing_codes(expires_at_ms, consumed_at_ms);
