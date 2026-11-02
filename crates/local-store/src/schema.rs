//! Schema v1 is append-only at the migration layer. No destructive schema reset.

pub const VERSION: i64 = 5;
pub const MIGRATION_001: &str = r#"
CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at_ms INTEGER NOT NULL
);
CREATE TABLE local_identity (
  singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
  device_id TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  time_zone TEXT NOT NULL,
  next_local_seq INTEGER NOT NULL DEFAULT 1 CHECK(next_local_seq > 0)
);
CREATE TABLE source_checkpoints (
  tool_id TEXT NOT NULL,
  source_key TEXT NOT NULL,
  source_fingerprint TEXT NOT NULL,
  read_offset INTEGER NOT NULL DEFAULT 0 CHECK(read_offset >= 0),
  parser_version INTEGER NOT NULL CHECK(parser_version > 0),
  last_scan_ms INTEGER NOT NULL,
  last_complete_scan_ms INTEGER,
  last_status TEXT NOT NULL CHECK(last_status IN ('complete','partial','degraded')),
  PRIMARY KEY(tool_id, source_key)
);
CREATE TABLE usage_records (
  record_id TEXT PRIMARY KEY,
  owner_device_id TEXT NOT NULL,
  tool_id TEXT NOT NULL,
  scope TEXT NOT NULL CHECK(scope IN ('device_local','account_global','unknown')),
  account_id TEXT,
  model_id TEXT,
  project_id TEXT,
  session_id TEXT,
  event_at_ms INTEGER,
  input_uncached_tokens INTEGER NOT NULL DEFAULT 0 CHECK(input_uncached_tokens >= 0),
  cache_read_tokens INTEGER NOT NULL DEFAULT 0 CHECK(cache_read_tokens >= 0),
  cache_write_tokens INTEGER NOT NULL DEFAULT 0 CHECK(cache_write_tokens >= 0),
  output_tokens INTEGER NOT NULL DEFAULT 0 CHECK(output_tokens >= 0),
  unclassified_tokens INTEGER NOT NULL DEFAULT 0 CHECK(unclassified_tokens >= 0),
  reasoning_tokens INTEGER CHECK(reasoning_tokens >= 0 AND reasoning_tokens <= output_tokens),
  estimated_cost_microusd INTEGER CHECK(estimated_cost_microusd >= 0),
  price_version TEXT,
  precision TEXT NOT NULL CHECK(precision IN ('exact','aggregate','estimated','unknown')),
  record_revision INTEGER NOT NULL CHECK(record_revision > 0),
  payload_hash TEXT NOT NULL,
  is_deleted INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0,1)),
  observed_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
CREATE INDEX ix_usage_device_time ON usage_records(owner_device_id,event_at_ms);
CREATE INDEX ix_usage_tool_time ON usage_records(tool_id,event_at_ms);
CREATE INDEX ix_usage_account ON usage_records(account_id,scope);
CREATE TABLE record_sources (
  record_id TEXT NOT NULL,
  tool_id TEXT NOT NULL,
  source_key TEXT NOT NULL,
  PRIMARY KEY(record_id,tool_id,source_key),
  FOREIGN KEY (record_id) REFERENCES usage_records(record_id) ON DELETE RESTRICT
);
CREATE INDEX ix_record_sources_source ON record_sources(tool_id,source_key);
CREATE TABLE sync_outbox (
  record_id TEXT PRIMARY KEY,
  record_revision INTEGER NOT NULL CHECK(record_revision > 0),
  enqueued_at_ms INTEGER NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
  last_error TEXT,
  FOREIGN KEY (record_id) REFERENCES usage_records(record_id) ON DELETE RESTRICT
);
CREATE INDEX ix_outbox_queue ON sync_outbox(enqueued_at_ms, record_id);
CREATE TABLE remote_usage_cache (
  owner_device_id TEXT NOT NULL,
  record_id TEXT NOT NULL,
  record_revision INTEGER NOT NULL CHECK(record_revision > 0),
  payload_json TEXT NOT NULL,
  is_deleted INTEGER NOT NULL CHECK(is_deleted IN (0,1)),
  server_change_seq INTEGER NOT NULL CHECK(server_change_seq >= 0),
  PRIMARY KEY(owner_device_id,record_id)
);
CREATE INDEX ix_remote_owner ON remote_usage_cache(owner_device_id);
CREATE TABLE sync_state (
  singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
  hub_url TEXT,
  last_pull_seq INTEGER NOT NULL DEFAULT 0 CHECK(last_pull_seq >= 0),
  last_push_ms INTEGER,
  last_pull_ms INTEGER,
  last_error TEXT
);
INSERT INTO sync_state(singleton) VALUES (1);
"#;

// M0-09: activity and quota are observations, NOT usage events.
// Neither table is attached to sync_outbox, preventing accidental upload.
pub const MIGRATION_002: &str = r#"
CREATE TABLE antigravity_activity_sources (
  source_key TEXT PRIMARY KEY,
  known_turns INTEGER NOT NULL CHECK(known_turns >= 0),
  unknown_turns INTEGER NOT NULL CHECK(unknown_turns >= 0),
  observed_at_ms INTEGER NOT NULL
);
CREATE TABLE antigravity_model_activity (
  source_key TEXT NOT NULL,
  model_id TEXT NOT NULL,
  response_count INTEGER NOT NULL CHECK(response_count > 0),
  PRIMARY KEY(source_key, model_id),
  FOREIGN KEY(source_key) REFERENCES antigravity_activity_sources(source_key) ON DELETE CASCADE
);
CREATE INDEX ix_antigravity_model_activity_model ON antigravity_model_activity(model_id);
CREATE TABLE antigravity_quota_snapshots (
  source_id TEXT NOT NULL,
  pool_id TEXT NOT NULL,
  window_id TEXT NOT NULL CHECK(window_id IN ('five_hour','weekly','daily','other')),
  remaining_basis_points INTEGER CHECK(remaining_basis_points BETWEEN 0 AND 10000),
  reset_at_ms INTEGER,
  observed_at_ms INTEGER NOT NULL CHECK(observed_at_ms >= 0),
  received_at_ms INTEGER NOT NULL CHECK(received_at_ms >= 0),
  PRIMARY KEY(source_id,pool_id,window_id)
);
CREATE INDEX ix_antigravity_quota_age ON antigravity_quota_snapshots(observed_at_ms);
"#;

// M1-04: durable retry scheduling and explicit conflict quarantine.
// Deferred and blocked rows remain visible; only due, non-blocked rows are uploaded.
pub const MIGRATION_003: &str = r#"
ALTER TABLE sync_outbox ADD COLUMN next_attempt_at_ms INTEGER NOT NULL DEFAULT 0 CHECK(next_attempt_at_ms >= 0);
ALTER TABLE sync_outbox ADD COLUMN blocked_at_ms INTEGER CHECK(blocked_at_ms IS NULL OR blocked_at_ms >= 0);
CREATE INDEX ix_outbox_due ON sync_outbox(blocked_at_ms,next_attempt_at_ms,enqueued_at_ms,record_id);
ALTER TABLE sync_state ADD COLUMN last_attempt_ms INTEGER;
ALTER TABLE sync_state ADD COLUMN last_success_ms INTEGER;
ALTER TABLE sync_state ADD COLUMN last_status TEXT NOT NULL DEFAULT 'never' CHECK(last_status IN ('never','idle','success','deferred','blocked','auth_required','error'));
"#;

pub const MIGRATION_004: &str = r#"
ALTER TABLE remote_usage_cache ADD COLUMN payload_hash TEXT NOT NULL DEFAULT '';
CREATE INDEX ix_remote_device_seq ON remote_usage_cache(owner_device_id,server_change_seq);
"#;

// Codex plan quota is a local observation, never usage or sync data.
pub const MIGRATION_005: &str = r#"
CREATE TABLE codex_quota_snapshots (
  source_id TEXT NOT NULL,
  pool_id TEXT NOT NULL,
  window_duration_minutes INTEGER NOT NULL CHECK(window_duration_minutes > 0),
  remaining_basis_points INTEGER CHECK(remaining_basis_points BETWEEN 0 AND 10000),
  reset_at_ms INTEGER CHECK(reset_at_ms IS NULL OR reset_at_ms >= 0),
  observed_at_ms INTEGER NOT NULL CHECK(observed_at_ms >= 0),
  received_at_ms INTEGER NOT NULL CHECK(received_at_ms >= 0),
  PRIMARY KEY(source_id,pool_id,window_duration_minutes)
);
CREATE INDEX ix_codex_quota_age ON codex_quota_snapshots(observed_at_ms);
"#;
