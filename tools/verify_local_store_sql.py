#!/usr/bin/env python3
"""External SQLite SQL contract smoke test. Not a replacement for cargo test."""
from pathlib import Path
import re
import sqlite3
import tempfile

ROOT = Path(__file__).resolve().parents[1]
schema_source = (ROOT/'crates/local-store/src/schema.rs').read_text()
source = '\n'.join((ROOT/'crates/local-store/src/repository'/p).read_text(encoding='utf-8') for p in ('records.rs', 'ingest/scans.rs', 'sync.rs'))
def migration(version):
    match = re.search(rf'pub const MIGRATION_00{version}: &str = r#"(.*?)"#;', schema_source, flags=re.S)
    assert match, f'Migration {version} source missing'
    return match.group(1)

def rust_strings(text):
    for m in re.finditer(r'"((?:[^"\\]|\\(?:\n|.))*)"', text, flags=re.S):
        v = m.group(1)
        v = re.sub(r'\\\n\s*', '', v)
        yield v

queries = list(rust_strings(source))
def sql(prefix):
    matches = [x for x in queries if x.startswith(prefix)]
    assert matches and len(set(matches)) == 1, (prefix, len(matches))
    return matches[0]

with tempfile.TemporaryDirectory(prefix='tokenscope-m03-') as td:
    path = Path(td)/'usage.db'
    db = sqlite3.connect(path)
    db.execute('PRAGMA foreign_keys=ON')
    assert db.execute('PRAGMA foreign_keys').fetchone()[0] == 1
    for version in (1, 2, 3):
        db.executescript(migration(version))
        db.execute('INSERT INTO schema_migrations(version,applied_at_ms) VALUES(?,?)', (version, 1234 + version))
        db.execute(f'PRAGMA user_version={version}')
    assert db.execute('PRAGMA user_version').fetchone()[0] == 3
    db.execute("INSERT INTO local_identity(singleton,device_id,display_name,time_zone) VALUES(1,'dev-test-001','Dev','Asia/Shanghai')")
    db.commit()
    ck = sql('INSERT INTO source_checkpoints(')
    put = sql('INSERT INTO usage_records(')
    out = sql('INSERT INTO sync_outbox(')
    load = sql('SELECT record_id,owner_device_id,tool_id,scope,account_id,model_id,project_id,session_id,event_at_ms,')
    ack = sql('DELETE FROM sync_outbox WHERE')
    def sample(tokens=100, rev=1):
        return ('f'*64,'dev-test-001','codex','device_local',None,'gpt-6',None,None,1000,
                tokens,0,0,20,0,5,125,'v1','exact',rev,'a'*64,0,10000,10000)
    db.execute('BEGIN IMMEDIATE')
    db.execute(put,sample())
    db.execute(out,('f'*64,1,10000))
    db.execute(ck,('codex','source1','file:abc',100,1,10,10,'complete'))
    db.commit()
    assert len(db.execute(load,('f'*64,)).fetchone()) == 21
    db.execute('BEGIN IMMEDIATE')
    db.execute(put,sample(30,2))
    db.execute(out,('f'*64,2,10001))
    assert db.execute(ack,('f'*64,1)).rowcount == 0
    db.commit()
    assert db.execute('SELECT input_uncached_tokens,record_revision FROM usage_records').fetchone() == (30,2)
    assert db.execute('SELECT record_revision FROM sync_outbox').fetchone() == (2,)
    # Force an outbox failure: all writes must roll back.
    db.execute("CREATE TRIGGER outbox_fail BEFORE INSERT ON sync_outbox BEGIN SELECT RAISE(ABORT,'injected'); END")
    try:
        with db:
            db.execute(put,('a'*64,)+sample()[1:])
            db.execute(out,('a'*64,1,10002))
            db.execute(ck,('codex','source2','file:2',100,1,12,12,'complete'))
        assert False, 'failure should raise'
    except sqlite3.DatabaseError as e:
        assert 'injected' in str(e)
    assert db.execute("SELECT count(*) FROM usage_records WHERE record_id=?",('a'*64,)).fetchone()[0] == 0
    assert db.execute("SELECT count(*) FROM source_checkpoints WHERE source_key='source2'").fetchone()[0] == 0
    assert db.execute('PRAGMA foreign_key_check').fetchall() == []
    # SQLite's online VACUUM INTO snapshot includes committed WAL state.
    backup = Path(td)/'backup.db'
    db.execute('VACUUM INTO ?', (str(backup),))
    snapshot=sqlite3.connect(backup)
    assert snapshot.execute('SELECT input_uncached_tokens,record_revision FROM usage_records').fetchone() == (30,2)
    assert snapshot.execute('SELECT record_revision FROM sync_outbox').fetchone() == (2,)
    snapshot.close()
    db.close()
    db = sqlite3.connect(path)
    assert db.execute('PRAGMA user_version').fetchone()[0] == 3
    assert db.execute('SELECT record_revision FROM sync_outbox').fetchone() == (2,)
    db.close()
print('PASS: schema, writer SQL, revision replace, CAS ack, rollback, foreign keys, online backup, file reopen')
