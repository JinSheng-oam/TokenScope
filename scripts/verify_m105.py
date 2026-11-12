#!/usr/bin/env python3
"""Offline M1-05 contract checks: Hub feed migration and local remote-cache semantics."""
from __future__ import annotations
import json, pathlib, re, sqlite3
ROOT = pathlib.Path(__file__).resolve().parents[1]

def hub_migration_check() -> None:
    db = sqlite3.connect(':memory:')
    for version in (1, 2):
        db.executescript((ROOT / f'hub/internal/sqlstore/migrations/{version:03}.sql').read_text())
        db.execute('INSERT INTO schema_migrations(version,applied_at_ms) VALUES(?,0)', (version,))
        db.execute(f'PRAGMA user_version={version}')
    db.execute("INSERT INTO devices(device_id,display_name,os_name,app_version,time_zone,created_at_ms) VALUES('office-pc-01','Office','windows','0.1.0','UTC',1)")
    record = {
        'record_id': 'a' * 64, 'owner_device_id': 'office-pc-01', 'tool_id': 'codex',
        'scope': 'device_local', 'precision': 'exact', 'account_id': None, 'model_id': 'gpt-6',
        'project_id': None, 'session_id': None, 'event_at_ms': 1000,
        'usage': {'input_uncached_tokens': 2, 'cache_read_tokens': 0, 'cache_write_tokens': 0,
                  'output_tokens': 3, 'unclassified_tokens': 0, 'reasoning_tokens': None},
        'estimated_cost_microusd': None, 'price_version': None, 'is_deleted': False,
    }
    raw = json.dumps(record, separators=(',', ':'))
    db.execute('INSERT INTO usage_events(device_id,record_id,record_revision,payload_hash,payload_json,received_at_ms) VALUES(?,?,?,?,?,?)',
               ('office-pc-01', 'a' * 64, 1, 'b' * 64, raw, 2))
    migration = (ROOT / 'hub/internal/sqlstore/migrations/003.sql').read_text()
    db.executescript(migration)
    assert db.execute('SELECT COUNT(*) FROM usage_change_feed').fetchone()[0] == 1, 'legacy usage bootstrap missing'
    db.execute('INSERT INTO usage_change_feed(device_id,record_id,record_revision,payload_hash,payload_json,changed_at_ms) VALUES(?,?,?,?,?,?)',
               ('office-pc-01', 'a' * 64, 2, 'c' * 64, raw, 3))
    high = db.execute('SELECT MAX(change_seq) FROM usage_change_feed').fetchone()[0]
    page = db.execute('SELECT change_seq,record_revision FROM usage_change_feed WHERE change_seq>? AND change_seq<=? ORDER BY change_seq LIMIT ?', (0, high, 1)).fetchall()
    assert page == [(1, 1)] and high == 2
    page2 = db.execute('SELECT change_seq,record_revision FROM usage_change_feed WHERE change_seq>? AND change_seq<=? ORDER BY change_seq LIMIT ?', (page[-1][0], high, 10)).fetchall()
    assert page2 == [(2, 2)]
    db.close()

def local_cache_check() -> None:
    src = (ROOT / 'crates/local-store/src/schema.rs').read_text()
    migrations = re.findall(r'pub const MIGRATION_00[1-4]: &str = r#"(.*?)"#;', src, re.S)
    assert len(migrations) == 4
    db = sqlite3.connect(':memory:')
    for i, sql in enumerate(migrations, 1):
        db.executescript(sql)
        db.execute('INSERT INTO schema_migrations(version,applied_at_ms) VALUES(?,0)', (i,))
        db.execute(f'PRAGMA user_version={i}')
    record = {
        'record_id': 'd' * 64, 'owner_device_id': 'remote-pc-001', 'tool_id': 'codex',
        'scope': 'device_local', 'precision': 'estimated', 'event_at_ms': 1000,
        'usage': {'input_uncached_tokens': 10, 'cache_read_tokens': 2, 'cache_write_tokens': 3,
                  'output_tokens': 5, 'unclassified_tokens': 0, 'reasoning_tokens': None},
        'estimated_cost_microusd': 25, 'is_deleted': False,
    }
    raw = json.dumps(record, separators=(',', ':'))
    upsert = """INSERT INTO remote_usage_cache(owner_device_id,record_id,record_revision,payload_json,is_deleted,server_change_seq,payload_hash)
    VALUES(?,?,?,?,?,?,?) ON CONFLICT(owner_device_id,record_id) DO UPDATE SET
    record_revision=excluded.record_revision,payload_json=excluded.payload_json,is_deleted=excluded.is_deleted,
    server_change_seq=excluded.server_change_seq,payload_hash=excluded.payload_hash
    WHERE excluded.record_revision>remote_usage_cache.record_revision OR
    (excluded.record_revision=remote_usage_cache.record_revision AND remote_usage_cache.payload_hash='')"""
    db.execute(upsert, ('remote-pc-001', 'd' * 64, 1, raw, 0, 1, 'e' * 64))
    db.execute('UPDATE sync_state SET last_pull_seq=1 WHERE singleton=1')
    # Old revision cannot replace newer cache state.
    older = dict(record); older['usage'] = dict(record['usage']); older['usage']['output_tokens'] = 1
    db.execute(upsert, ('remote-pc-001', 'd' * 64, 1, json.dumps(older), 0, 2, 'f' * 64))
    current = db.execute('SELECT record_revision,payload_hash FROM remote_usage_cache').fetchone()
    assert current == (1, 'e' * 64)
    total_sql = """SELECT COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.input_uncached_tokens') AS INTEGER)+
    CAST(json_extract(payload_json,'$.usage.cache_read_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.cache_write_tokens') AS INTEGER)+
    CAST(json_extract(payload_json,'$.usage.output_tokens') AS INTEGER)+CAST(json_extract(payload_json,'$.usage.unclassified_tokens') AS INTEGER)),0),
    COUNT(*),SUM(CASE WHEN json_extract(payload_json,'$.estimated_cost_microusd') IS NOT NULL THEN 1 ELSE 0 END),
    SUM(CASE WHEN json_extract(payload_json,'$.precision')='estimated' THEN 1 ELSE 0 END)
    FROM remote_usage_cache WHERE is_deleted=0 AND json_extract(payload_json,'$.scope')='device_local'"""
    assert db.execute(total_sql).fetchone() == (20, 1, 1, 1)
    by_device = db.execute("SELECT owner_device_id,COUNT(*) FROM remote_usage_cache WHERE is_deleted=0 GROUP BY owner_device_id").fetchall()
    assert by_device == [('remote-pc-001', 1)]
    tomb = dict(record); tomb['usage'] = {k: 0 for k in ('input_uncached_tokens','cache_read_tokens','cache_write_tokens','output_tokens','unclassified_tokens')}; tomb['usage']['reasoning_tokens'] = None; tomb['estimated_cost_microusd'] = None; tomb['is_deleted'] = True
    db.execute(upsert, ('remote-pc-001', 'd' * 64, 2, json.dumps(tomb), 1, 3, 'a' * 64))
    assert db.execute(total_sql).fetchone()[0] == 0
    db.execute('UPDATE sync_state SET last_pull_seq=3 WHERE singleton=1')
    assert db.execute('SELECT last_pull_seq FROM sync_state').fetchone()[0] == 3
    db.close()

def source_contract_check() -> None:
    checks = {
        'Hub route': 'GET /api/v1/usage/changes' in (ROOT/'hub/internal/httpapi/server.go').read_text(),
        'append-only feed transaction': 'INSERT INTO usage_change_feed' in (ROOT/'hub/internal/sqlstore/sqlite.go').read_text(),
        'client pull': 'pull_usage_changes' in (ROOT/'crates/sync-client/src/client.rs').read_text(),
        'atomic local cache': 'apply_remote_changes' in (ROOT/'crates/local-store/src/repository/sync.rs').read_text(),
        'agent push+pull': 'pull_device_once' in (ROOT/'crates/tokenscope-agent/src/daemon.rs').read_text(),
        'per-device dashboard': 'remote_by_device' in (ROOT/'apps/desktop/src/App.svelte').read_text(),
    }
    for label, ok in checks.items():
        print(f'{"PASS" if ok else "FAIL"} {label}')
    assert all(checks.values())

if __name__ == '__main__':
    hub_migration_check(); print('PASS Hub migration/bootstrap/pagination')
    local_cache_check(); print('PASS local migration/revision/tombstone/aggregate/cursor')
    source_contract_check()
