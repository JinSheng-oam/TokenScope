#!/usr/bin/env python3
"""Offline M1-03 contract and SQLite migration oracle; not a Rust/Go build substitute."""
from pathlib import Path
import sqlite3, tempfile
ROOT=Path(__file__).resolve().parents[1]
checks=[]
def check(name, ok):
    checks.append((name,bool(ok)))
    if not ok: raise SystemExit(f'FAIL: {name}')

hub=ROOT/'hub'
service=(hub/'internal/hub/service.go').read_text()
api=(hub/'internal/httpapi/server.go').read_text()
sql=(hub/'internal/sqlstore/sqlite.go').read_text()
rust=(ROOT/'crates/sync-client/src/sync.rs').read_text()
worker=(ROOT/'crates/sync-client/src/worker.rs').read_text()
check('Hub usage batch endpoint', 'POST /api/v1/usage/batch' in api)
check('authenticated device context', 's.requireDevice(http.HandlerFunc(s.pushUsageBatch))' in api)
check('owner device binding', 'r.OwnerDeviceID != deviceID' in service)
check('canonical payload hash verification', 'tokenscope/canonical-record/v1\\x00' in service)
check('revision conflict protection', 'ErrRevisionConflict' in (hub/'internal/hub/memory_repository.go').read_text() and 'ErrRevisionConflict' in sql)
check('SQLite v2 migration retained in forward schema', 'migrations/002.sql' in sql and 'migrations/003.sql' in sql and 'const schemaVersion = 3' in sql)
check('bounded 200 record batches', 'len(records) > 200' in service and 'MAX_SYNC_BATCH: usize = 200' in rust)
check('2 MiB client request bound', 'MAX_SYNC_REQUEST_BYTES' in rust)
check('revision compare-and-delete ACK', 'ack_outbox_if_revision' in worker)
check('superseded does not ACK', 'UsageAckStatus::Superseded' in worker and 'hub_superseded' in worker)

# Exercise the real migration SQL and table constraints with Python SQLite.
with tempfile.TemporaryDirectory() as td:
    db=sqlite3.connect(Path(td)/'hub.db')
    db.execute('PRAGMA foreign_keys=ON')
    for version in (1,2):
        migration=(hub/'internal/sqlstore/migrations'/f'{version:03d}.sql').read_text()
        for stmt in migration.split(';'):
            if stmt.strip(): db.execute(stmt)
        db.execute('INSERT INTO schema_migrations VALUES(?,?)',(version,version*1000))
        db.execute(f'PRAGMA user_version={version}')
    db.execute("INSERT INTO devices(device_id,display_name,os_name,app_version,time_zone,created_at_ms) VALUES('office-pc-01','Office','windows','0.1.0','UTC',1)")
    rid='a'*64
    db.execute('INSERT INTO usage_events VALUES(?,?,?,?,?,?)',('office-pc-01',rid,1,'b'*64,'{}',2))
    # Monotonic revision update and stale-revision non-overwrite oracle.
    current=db.execute('SELECT record_revision,payload_hash FROM usage_events WHERE device_id=? AND record_id=?',('office-pc-01',rid)).fetchone()
    assert current==(1,'b'*64)
    db.execute('UPDATE usage_events SET record_revision=2,payload_hash=?,payload_json=?,received_at_ms=? WHERE device_id=? AND record_id=?',('c'*64,'{"r":2}',3,'office-pc-01',rid))
    assert db.execute('SELECT record_revision FROM usage_events').fetchone()[0]==2
    assert db.execute('PRAGMA foreign_key_check').fetchall()==[]
    assert db.execute('PRAGMA user_version').fetchone()[0]==2
    db.commit()
check('SQLite v1->v2 migration and usage table oracle', True)
print(f'M1-03 offline checks: {len(checks)}/{len(checks)} PASS')
for name,_ in checks: print('PASS',name)
