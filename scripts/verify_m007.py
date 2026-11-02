#!/usr/bin/env python3
"""M0-07 independent SQL integration oracle; does NOT compile or execute Rust."""
from pathlib import Path
import re
import sqlite3
import tempfile

ROOT = Path(__file__).resolve().parents[1]
schema_source = (ROOT/'crates/local-store/src/schema.rs').read_text()
source = (ROOT/'crates/local-store/src/repository.rs').read_text()
def migration(version):
    m = re.search(rf'pub const MIGRATION_00{version}: &str = r#"(.*?)"#;', schema_source, re.S)
    assert m, version
    return m.group(1)

def rust_strings(text):
    for match in re.finditer(r'"((?:[^"\\]|\\(?:\n|.))*)"',text,re.S):
        yield re.sub(r'\\\n\s*','',match.group(1))

queries=list(rust_strings(source))
def sql(prefix):
    got=[v for v in queries if v.startswith(prefix)]
    assert got and len(set(got)) == 1, (prefix,len(got))
    return got[0]

put = sql('INSERT INTO usage_records(')
source_put = sql('INSERT OR IGNORE INTO record_sources(')
ck = sql('INSERT INTO source_checkpoints(')
out = sql('INSERT INTO sync_outbox(')
missing = sql('SELECT rs.rowid, rs.record_id FROM record_sources rs')
ack = sql('DELETE FROM sync_outbox WHERE')
seen = sql('INSERT OR IGNORE INTO temp.tokenscope_seen(')
assert 'NOT EXISTS' in missing and 'u.is_deleted=0' in missing
with tempfile.TemporaryDirectory(prefix='tokenscope-m007-') as td:
    db=sqlite3.connect(str(Path(td)/'data.db'))
    db.execute('PRAGMA foreign_keys=ON')
    for version in (1, 2, 3):
        db.executescript(migration(version))
        db.execute('INSERT INTO schema_migrations(version,applied_at_ms) VALUES(?,?)', (version, 1000 + version))
        db.execute(f'PRAGMA user_version={version}')
    db.execute("INSERT INTO local_identity(singleton,device_id,display_name,time_zone) VALUES(1,'dev1','desk','UTC')")
    db.execute('CREATE TEMP TABLE tokenscope_seen(record_id TEXT PRIMARY KEY) WITHOUT ROWID')
    def record(i, rev=1, deleted=0):
        rec_id=f'{i:064x}'
        return (rec_id,'dev1','claude','device_local',None,'claude-test',None,None,1000,
                0 if deleted else 1,0,0,0 if deleted else 2,0,None,None,None,'exact',rev,'a'*64,deleted,1234,1234)
    db.commit()
    db.execute('BEGIN IMMEDIATE')
    for i in range(1,3206):
        db.execute(put,record(i))
        db.execute(source_put,(f'{i:064x}','claude','src'))
        db.execute(out,(f'{i:064x}',1,1234))
        assert db.execute(seen,(f'{i:064x}',)).rowcount==1
    db.execute(ck,('claude','src','first',3205,1,10,10,'complete'))
    db.execute('DELETE FROM temp.tokenscope_seen')
    db.commit()
    assert db.execute('SELECT count(*) FROM usage_records').fetchone()[0]==3205
    db.execute('BEGIN IMMEDIATE')
    for i in range(1,3101):
        assert db.execute(seen,(f'{i:064x}',)).rowcount==1
    stale=[]; cursor=-(2**63)
    while True:
        row=db.execute(missing,('claude','src',cursor)).fetchone()
        if row is None:break
        cursor=row[0]
        record_id=row[1]
        stale.append(record_id)
        db.execute(put,record(int(record_id,16),2,1))
        db.execute(out,(record_id,2,1235))
    assert len(stale)==105
    db.execute(ck,('claude','src','second',3100,1,11,11,'complete'))
    db.execute('DELETE FROM temp.tokenscope_seen')
    db.commit()
    assert db.execute('SELECT COUNT(*) FROM usage_records WHERE is_deleted=1').fetchone()[0]==105
    assert db.execute(ack,(stale[0],1)).rowcount==0
    assert db.execute('SELECT record_revision FROM sync_outbox WHERE record_id=?',(stale[0],)).fetchone()[0]==2
    db.commit()
    # An application error or interruption must preserve original checkpoint,
    # record revisions, outbox, and source markers.
    db.execute('BEGIN IMMEDIATE')
    db.execute(put,record(1,2,1))
    db.execute(out,(f'{1:064x}',2,1236))
    db.execute(ck,('claude','src','bad',42,1,12,12,'complete'))
    db.rollback()
    assert db.execute("SELECT source_fingerprint FROM source_checkpoints WHERE source_key='src'").fetchone()[0]=='second'
    assert db.execute('SELECT record_revision FROM usage_records WHERE record_id=?',(f'{1:064x}',)).fetchone()[0]==1
    assert db.execute('PRAGMA foreign_key_check').fetchall()==[]
    db.close()
print('PASS M0-07: 3205 row complete scan, 105 tombstones, versioned ACK, transaction rollback, FK integrity')
print('NOTE: SQL integration oracle only. Rust code compilation and integration tests are NOT RUN.')
