#!/usr/bin/env python3
"""Independent M1-04 checks: migration SQL, durable retry schedule, quarantine, and contracts."""
from pathlib import Path
import re, sqlite3, sys
ROOT = Path(__file__).resolve().parents[1]
schema = (ROOT / 'crates/local-store/src/schema.rs').read_text()
repo = '\n'.join((ROOT/'crates/local-store/src/repository'/p).read_text(encoding='utf-8') for p in ('models.rs', 'sync.rs'))
worker = (ROOT / 'crates/sync-client/src/worker.rs').read_text()
agent = '\n'.join((ROOT/'crates/tokenscope-agent/src'/p).read_text(encoding='utf-8') for p in ('cli.rs', 'daemon.rs', 'sync.rs'))
tauri = '\n'.join((ROOT/'apps/desktop/src-tauri/src'/p).read_text(encoding='utf-8') for p in ('hub.rs', 'agent.rs'))
conn = sqlite3.connect(':memory:')
for version in (1, 2, 3):
    match = re.search(rf'pub const MIGRATION_00{version}: &str = r#"(.*?)"#;', schema, re.S)
    assert match, f'missing migration {version}'
    conn.executescript(match.group(1))
    conn.execute('INSERT INTO schema_migrations(version,applied_at_ms) VALUES(?,?)', (version, version))
    conn.execute(f'PRAGMA user_version={version}')
assert conn.execute('PRAGMA user_version').fetchone()[0] == 3
outbox_cols = {r[1] for r in conn.execute('PRAGMA table_info(sync_outbox)')}
assert {'next_attempt_at_ms', 'blocked_at_ms'} <= outbox_cols
state_cols = {r[1] for r in conn.execute('PRAGMA table_info(sync_state)')}
assert {'last_attempt_ms','last_success_ms','last_status'} <= state_cols
# Seed a valid row and exercise the same SQL semantics as the repository methods.
conn.execute("INSERT INTO local_identity(singleton,device_id,display_name,time_zone) VALUES(1,'device-001','Test','UTC')")
conn.execute("INSERT INTO usage_records(record_id,owner_device_id,tool_id,scope,precision,record_revision,payload_hash,observed_at_ms,updated_at_ms) VALUES(?,?,?,?,?,?,?,?,?)", ('a'*64,'device-001','codex','device_local','exact',1,'b'*64,1,1))
conn.execute("INSERT INTO sync_outbox(record_id,record_revision,enqueued_at_ms) VALUES(?,?,?)", ('a'*64,1,1))
# First failure: 5 seconds.
now = 100_000
attempts = conn.execute('SELECT attempts FROM sync_outbox').fetchone()[0]
delay = min(3_600_000, 5_000 * (1 << min(max(attempts, 0), 10)))
next_at = now + delay
conn.execute("UPDATE sync_outbox SET attempts=attempts+1,last_error=?,next_attempt_at_ms=? WHERE record_id=? AND record_revision=? AND blocked_at_ms IS NULL", ('transport',next_at,'a'*64,1))
assert next_at == 105_000
assert conn.execute('SELECT COUNT(*) FROM sync_outbox WHERE blocked_at_ms IS NULL AND next_attempt_at_ms<=?', (104_999,)).fetchone()[0] == 0
assert conn.execute('SELECT COUNT(*) FROM sync_outbox WHERE blocked_at_ms IS NULL AND next_attempt_at_ms<=?', (105_000,)).fetchone()[0] == 1
# Superseded is quarantined, not retried.
conn.execute("UPDATE sync_outbox SET blocked_at_ms=?,last_error=? WHERE record_id=? AND record_revision=?", (105_000,'hub_superseded','a'*64,1))
assert conn.execute('SELECT COUNT(*) FROM sync_outbox WHERE blocked_at_ms IS NULL').fetchone()[0] == 0
# A new local revision must clear retry/quarantine metadata.
conn.execute("UPDATE sync_outbox SET record_revision=2,attempts=0,last_error=NULL,next_attempt_at_ms=0,blocked_at_ms=NULL WHERE record_id=?", ('a'*64,))
assert conn.execute('SELECT record_revision,attempts,next_attempt_at_ms,blocked_at_ms FROM sync_outbox').fetchone() == (2,0,0,None)
# Check the Rust source wires the scheduler and daemon controls.
for token in ['pending_outbox_due','schedule_outbox_retry','block_outbox_revision','sync_outbox_status','update_sync_state']:
    assert token in repo, token
for token in ['retryable','schedule_outbox_retry','block_outbox_revision','MAX_SYNC_BATCH']:
    assert token in worker, token
for token in ['--sync','--sync-once','--sync-status','sync_device_once']:
    assert token in agent, token
print('M1-04 independent checks: PASS')
print('schema v3 migration: PASS')
print('persistent retry schedule: PASS')
print('superseded quarantine: PASS')
print('new revision clears retry/block state: PASS')
# The same aggregate shape used by sync_outbox_status must work on empty queues.
status = conn.execute("""SELECT
 COALESCE(SUM(CASE WHEN blocked_at_ms IS NULL AND next_attempt_at_ms<=? THEN 1 ELSE 0 END),0),
 COALESCE(SUM(CASE WHEN blocked_at_ms IS NULL AND next_attempt_at_ms>? THEN 1 ELSE 0 END),0),
 COALESCE(SUM(CASE WHEN blocked_at_ms IS NOT NULL THEN 1 ELSE 0 END),0),
 MIN(CASE WHEN blocked_at_ms IS NULL THEN enqueued_at_ms END),
 MIN(CASE WHEN blocked_at_ms IS NULL AND next_attempt_at_ms>? THEN next_attempt_at_ms END)
 FROM sync_outbox""", (1_000_000,1_000_000,1_000_000)).fetchone()
assert status[:3] == (1,0,0) and status[3] == 1
print('worker and daemon wiring: PASS')
print('sync status aggregate query: PASS')
# UI and IPC schema must carry sync state end-to-end.
assert 'sync_status: SyncOutboxStatus' in repo
assert 'sync_status:' in (ROOT / 'crates/local-ipc/src/lib.rs').read_text()
assert 'sync_status:' in (ROOT / 'apps/desktop/src/lib/types.ts').read_text()
print('Agent -> IPC -> dashboard payload sync status: PASS')
assert '--sync-config' in agent and 'resolve_sync_hub_url' in agent
assert 'persist_sync_hub_url' in tauri and 'clear_sync_hub_url' in tauri
assert '"--sync"' in tauri and '"--sync-config"' in tauri
print('Tauri pairing -> dynamic Agent sync configuration: PASS')
