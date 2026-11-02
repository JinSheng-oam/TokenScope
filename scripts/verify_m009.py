#!/usr/bin/env python3
"""Independent executable SQLite SQL oracle. NOT a Rust/cargo test."""
from pathlib import Path
import sqlite3, re, tempfile
root = Path(__file__).resolve().parents[1]
schema = (root/'crates/local-store/src/schema.rs').read_text()
repo = (root/'crates/local-store/src/repository.rs').read_text()
def migration(n):
    m=re.search(r'pub const MIGRATION_00%s: &str = r#"(.*?)"#;' % n,schema,re.S)
    assert m, n
    return m.group(1)
def rust_strings(src):
    # Rust multiline SQL literals with escaped line continuation
    for m in re.finditer(r'"((?:[^"\\]|\\(?:\n|.))*)"', src, flags=re.S):
        yield re.sub(r'\\\n\s*', '', m.group(1))
queries=list(rust_strings(repo))
def query(prefix):
    match=[q for q in queries if q.startswith(prefix)]
    assert match, f'missing SQL: {prefix}'
    assert len(set(match))==1, prefix
    return match[0]
q_activity=query('INSERT INTO antigravity_activity_sources(')
q_model=query('INSERT INTO antigravity_model_activity(')
q_quota=query('INSERT INTO antigravity_quota_snapshots(')
q_summary=query('SELECT COALESCE(SUM(known_turns),0),')
q_model_summary=query('SELECT model_id,SUM(response_count)')
with tempfile.TemporaryDirectory() as d:
    p=Path(d)/'app.db'
    con=sqlite3.connect(p,isolation_level=None)
    con.execute('PRAGMA foreign_keys=ON')
    con.executescript(migration('1'))
    con.execute('INSERT INTO schema_migrations(version,applied_at_ms) VALUES(1,123)')
    con.execute('PRAGMA user_version=1')
    con.execute('INSERT INTO local_identity(singleton,device_id,display_name,time_zone) VALUES(1,\'device\',\'D\',\'UTC\')')
    # The new app migrates the exact M0-08 v1 database in-place.
    con.execute('BEGIN IMMEDIATE')
    for statement in migration('2').split(';'):
        if statement.strip():
            con.execute(statement)
    con.execute('INSERT INTO schema_migrations(version,applied_at_ms) VALUES(2,456)')
    con.execute('PRAGMA user_version=2')
    con.execute('COMMIT')
    assert con.execute('PRAGMA user_version').fetchone()[0] == 2
    assert con.execute('SELECT COUNT(*) FROM schema_migrations').fetchone()[0] == 2
    # Activity has no token rows and no outbox entries.
    con.execute('BEGIN IMMEDIATE')
    con.execute(q_activity, ('opaque-source',3,1,100))
    con.execute(q_model, ('opaque-source','gemini-pro',2))
    con.execute(q_model, ('opaque-source','claude-sonnet',1))
    con.execute('COMMIT')
    assert con.execute(q_summary).fetchone() == (3,1,1)
    assert dict(con.execute(q_model_summary)) == {'claude-sonnet':1,'gemini-pro':2}
    assert con.execute('SELECT COUNT(*) FROM sync_outbox').fetchone()[0] == 0
    con.execute('BEGIN IMMEDIATE')
    con.execute(q_activity,('opaque-source',1,0,200))
    con.execute('DELETE FROM antigravity_model_activity WHERE source_key=?',('opaque-source',))
    con.execute(q_model,('opaque-source','gemini-pro',1))
    con.execute('COMMIT')
    assert con.execute(q_summary).fetchone() == (1,0,1)
    assert dict(con.execute(q_model_summary)) == {'gemini-pro':1}
    # Inject a failure in model insert; activity count must roll back together.
    con.execute('CREATE TRIGGER break_activity BEFORE INSERT ON antigravity_model_activity BEGIN SELECT RAISE(ABORT,\'injected\'); END')
    try:
        con.execute('BEGIN IMMEDIATE')
        con.execute(q_activity,('opaque-source',99,0,300))
        con.execute('DELETE FROM antigravity_model_activity WHERE source_key=?',('opaque-source',))
        con.execute(q_model,('opaque-source','gemini-pro',99))
        con.execute('COMMIT')
        raise AssertionError('trigger was expected')
    except sqlite3.DatabaseError as exc:
        assert 'injected' in str(exc)
        con.execute('ROLLBACK')
    assert con.execute(q_summary).fetchone()==(1,0,1)
    assert dict(con.execute(q_model_summary))=={'gemini-pro':1}
    # Snapshot NULL unknown is distinct from exact 0, and older is ignored.
    con.execute(q_quota,('local-account-1','gemini_pro','five_hour',None,None,100,110))
    assert con.execute('SELECT remaining_basis_points FROM antigravity_quota_snapshots').fetchone()[0] is None
    con.execute(q_quota,('local-account-1','gemini_pro','five_hour',0,None,200,210))
    con.execute(q_quota,('local-account-1','gemini_pro','five_hour',9900,None,190,220))
    assert con.execute('SELECT remaining_basis_points,observed_at_ms FROM antigravity_quota_snapshots').fetchone()==(0,200)
    assert con.execute('SELECT COUNT(*) FROM sync_outbox').fetchone()[0]==0
    assert con.execute('PRAGMA foreign_key_check').fetchall()==[]
    con.close()
    reopened=sqlite3.connect(p)
    assert reopened.execute(q_summary).fetchone()==(1,0,1)
    assert reopened.execute('SELECT remaining_basis_points FROM antigravity_quota_snapshots').fetchone()[0]==0
    reopened.close()
print('PASS M0-09 independent SQLite oracle: v1->v2 migration, activity, rollback, quota CAS, reopen (NOT Rust test)')
