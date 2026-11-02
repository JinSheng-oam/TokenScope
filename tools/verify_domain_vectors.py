#!/usr/bin/env python3
"""Independent Python reference for Rust CanonicalRecord schema v1 wire vectors.
Does not compile Rust. It checks a fixed cross-language hash and ID specification.
"""
from pathlib import Path
import hashlib
import json

ROOT = Path(__file__).resolve().parents[1]
VECTOR = ROOT / "fixtures/domain/canonical-record-v1.json"
EXPECTED_FIELDS = [
    'record_id','owner_device_id','tool_id','scope','precision','account_id',
    'model_id','project_id','session_id','event_at_ms','usage',
    'estimated_cost_microusd','price_version','is_deleted'
]
EXPECTED_BUCKETS = [
    'input_uncached_tokens','cache_read_tokens','cache_write_tokens',
    'output_tokens','unclassified_tokens','reasoning_tokens'
]

def identity(domain: str, *parts: bytes, prefix='') -> str:
    raw = (domain + '\0').encode() + b''.join(len(p).to_bytes(4,'big') + p for p in parts)
    return prefix + hashlib.sha256(raw).hexdigest()

def main():
    fixture=json.loads(VECTOR.read_text(encoding='utf-8'))
    rec=fixture['record']
    assert list(rec)==EXPECTED_FIELDS
    assert list(rec['usage'])==EXPECTED_BUCKETS
    t=rec['tool_id'].encode()
    assert rec['record_id']==identity('tokenscope/record-id/v1',t,b'source-a',b'response-42')
    assert rec['account_id']==identity('tokenscope/account-id/v1',t,b'user-5',prefix='acct_')
    assert rec['project_id']==identity('tokenscope/project-id/v1',t,b'/repo/secret',prefix='prj_')
    assert rec['session_id']==identity('tokenscope/session-id/v1',t,b'session-5',prefix='sess_')
    encoded=json.dumps(rec,ensure_ascii=False,separators=(',',':')).encode('utf-8')
    h=hashlib.sha256(b'tokenscope/canonical-record/v1\0'+encoded).hexdigest()
    assert h==fixture['payload_hash']
    # These are pinned v1 contract vectors, not just self-consistency checks.
    assert rec['record_id']=='cb38507ac3893862a5f850a8ff8d8fdc43ee3ca0be04eb543c11de52f881121e'
    assert h=='fa0328d10329d3ab4757f41159b91a6dd162e7ebde8f9b2526e8d99af4299897'
    assert fixture['record_revision']==1
    assert sum(rec['usage'][x] for x in EXPECTED_BUCKETS[:-1])==180
    assert b'/repo/secret' not in encoded and b'user-5' not in encoded
    print('PASS M0-02 independent canonical wire vector', h)

if __name__=='__main__': main()
