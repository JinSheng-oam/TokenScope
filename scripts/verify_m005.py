#!/usr/bin/env python3
"""Independent fixture/schema checks; does NOT compile or execute Rust."""
import json
import re
import sqlite3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIX = ROOT / 'fixtures/codex'

def records(name):
    return [json.loads(line) for line in (FIX / name).read_text().splitlines()]

def test_modern():
    items = records('modern-sanitized.jsonl')
    usage = [x['payload']['usage'] for x in items if x['type'] == 'token_usage_record']
    assert len(usage) == 2
    assert sum(x['total_tokens'] for x in usage) == 350
    assert sum(x['reasoning_output_tokens'] for x in usage) == 12
    assert usage[0]['input_tokens'] - usage[0]['cached_input_tokens'] - usage[0]['cache_write_input_tokens'] == 60
    assert sum(x['total_tokens'] for x in usage) != 700  # avoid double counting snapshots
    assert [x['payload']['model'] for x in items if x['type']=='turn_context'] == ['gpt-example-a','gpt-example-b']
    assert items[2]['timestamp'][:10] == '2026-11-01'
    assert items[5]['timestamp'][:10] == '2026-11-02'
    return 'modern native response usage & model/time: PASS'

def test_legacy():
    items = records('legacy-sanitized.jsonl')
    snap = [x['payload']['info']['total_token_usage'] for x in items if x.get('payload',{}).get('type')=='token_count']
    previous = None
    deltas=[]
    for s in snap:
        if s == previous: continue
        if previous is None: prev={'input_tokens':0, 'output_tokens':0, 'total_tokens':0}
        else: prev=previous
        d={k:s[k]-prev.get(k,0) for k in ('input_tokens','output_tokens','total_tokens')}
        assert d['total_tokens']==d['input_tokens']+d['output_tokens']
        deltas.append(d)
        previous=s
    assert len(deltas)==2 and sum(x['total_tokens'] for x in deltas)==350
    return 'legacy snapshots & duplicate suppression: PASS'

def test_schema():
    code = (ROOT / 'crates/local-store/src/schema.rs').read_text()
    match = re.search(r'pub const MIGRATION_001: &str = r#"(.*?)"#;', code, re.S)
    assert match, 'missing SQL migration source'
    conn=sqlite3.connect(':memory:')
    conn.executescript(match.group(1))
    conn.execute('INSERT INTO local_identity(singleton,device_id,display_name,time_zone) VALUES(1,?,?,?)', ('dev1','Test','UTC'))
    cols=("record_id,owner_device_id,tool_id,scope,precision,record_revision,payload_hash,is_deleted,"
          "observed_at_ms,updated_at_ms")
    for recid in ('old_aggregate','new_response'):
        conn.execute(f'INSERT INTO usage_records({cols}) VALUES(?,?,?,?,?,?,?,?,?,?)',
                     (recid,'dev1','codex','device_local','exact',1,'hash',0,1,1))
        conn.execute('INSERT INTO record_sources(record_id,tool_id,source_key) VALUES(?,?,?)',
                     (recid,'codex','sess_example'))
        conn.execute('INSERT INTO sync_outbox(record_id,record_revision,enqueued_at_ms) VALUES(?,?,?)',(recid,1,1))
    conn.commit()
    conn.execute('BEGIN IMMEDIATE')
    conn.execute('UPDATE usage_records SET is_deleted=1,record_revision=2 WHERE record_id=?',('old_aggregate',))
    conn.execute('UPDATE sync_outbox SET record_revision=2 WHERE record_id=?',('old_aggregate',))
    conn.commit()
    assert conn.execute('SELECT count(*) FROM usage_records WHERE is_deleted=0').fetchone()[0] == 1
    assert conn.execute('DELETE FROM sync_outbox WHERE record_id=? AND record_revision=?',('old_aggregate',1)).rowcount == 0
    assert conn.execute('SELECT count(*) FROM pragma_foreign_key_check').fetchone()[0] == 0
    return 'SQLite migration/tombstone/old ACK independent check: PASS'

if __name__=='__main__':
    for fn in (test_modern,test_legacy,test_schema): print(fn())
    print('NOTE: These are independent Python checks, not Rust build/test results.')
