#!/usr/bin/env python3
"""Independent SQLite oracle for M1-06 remote usage breakdown semantics."""
import json
import sqlite3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = (ROOT / 'crates/local-store/src/schema.rs').read_text()
REPO = (ROOT / 'crates/local-store/src/repository.rs').read_text()
UI = (ROOT / 'apps/desktop/src/App.svelte').read_text()
DOCKER = (ROOT / 'deploy/compose.yaml').read_text()

assert 'remote_by_tool' in REPO and 'remote_by_model' in REPO and 'remote_by_project' in REPO
assert '"tool_id" => "$.tool_id"' in REPO
assert '"model_id" => "$.model_id"' in REPO
assert '"project_id" => "$.project_id"' in REPO
assert 'remote_by_tool' in SCHEMA or 'remote_usage_cache' in SCHEMA
assert 'REMOTE CACHE' in UI and 'remoteSelectedDevice' in UI
assert 'read_only: true' in DOCKER and 'no-new-privileges:true' in DOCKER
assert 'hub-data-init:' in DOCKER and 'chown -R 10001:10001 /data' in DOCKER
assert '172.30.0.0/24' in DOCKER

con = sqlite3.connect(':memory:')
con.execute('''CREATE TABLE remote_usage_cache (
 owner_device_id TEXT, record_id TEXT, payload_json TEXT, is_deleted INTEGER,
 PRIMARY KEY(owner_device_id, record_id))''')

def record(owner, rid, tool, model, project, scope='device_local', tokens=0, deleted=0, cost=None):
    payload = {
      'tool_id': tool, 'model_id': model, 'project_id': project, 'scope': scope,
      'usage': {'input_uncached_tokens': tokens, 'cache_read_tokens': 0,
                'cache_write_tokens': 0, 'output_tokens': 2, 'unclassified_tokens': 0},
      'estimated_cost_microusd': cost,
    }
    con.execute('INSERT OR REPLACE INTO remote_usage_cache VALUES(?,?,?,?)',
                (owner, rid, json.dumps(payload), deleted))

record('pc-a-0001', 'a1', 'codex', 'gpt-6', 'prj_alpha123456', tokens=10, cost=100)
record('pc-a-0001', 'a2', 'claude-code', 'claude-sonnet', None, tokens=20)
record('pc-b-0002', 'b1', 'codex', 'gpt-6', 'prj_beta123456', tokens=30, cost=300)
record('pc-b-0002', 'b2', 'codex', 'gpt-6', 'prj_deleted', tokens=999, deleted=1)
record('pc-b-0002', 'b3', 'codex', 'gpt-6', None, scope='account_global', tokens=500)

sql = '''SELECT owner_device_id, json_extract(payload_json, ?), COUNT(*),
 COALESCE(SUM(CAST(json_extract(payload_json,'$.usage.input_uncached_tokens') AS INTEGER)+
 CAST(json_extract(payload_json,'$.usage.cache_read_tokens') AS INTEGER)+
 CAST(json_extract(payload_json,'$.usage.cache_write_tokens') AS INTEGER)+
 CAST(json_extract(payload_json,'$.usage.output_tokens') AS INTEGER)+
 CAST(json_extract(payload_json,'$.usage.unclassified_tokens') AS INTEGER)),0),
 SUM(json_extract(payload_json,'$.estimated_cost_microusd'))
 FROM remote_usage_cache WHERE is_deleted=0 AND json_extract(payload_json,'$.scope')='device_local'
 GROUP BY owner_device_id, json_extract(payload_json, ?) ORDER BY 4 DESC'''
for dimension, path in [('tool', '$.tool_id'), ('model', '$.model_id'), ('project', '$.project_id')]:
    rows = con.execute(sql, (path, path)).fetchall()
    assert rows, dimension
    assert all(r[0] in ('pc-a-0001', 'pc-b-0002') for r in rows)
    assert not any(r[1] == 'prj_deleted' for r in rows)
    assert not any(r[1] == 'account_global' for r in rows)

# Per-device tool totals: pc-a codex = 12; pc-b codex = 32. Deleted and account-global rows excluded.
tool_rows = con.execute(sql, ('$.tool_id', '$.tool_id')).fetchall()
by_owner = {(r[0], r[1]): r[3] for r in tool_rows}
assert by_owner[('pc-a-0001', 'codex')] == 12
assert by_owner[('pc-a-0001', 'claude-code')] == 22
assert by_owner[('pc-b-0002', 'codex')] == 32

# Revision overwrite semantics: current cache stores one row per owner+record.
record('pc-b-0002', 'b1', 'codex', 'gpt-6', 'prj_beta123456', tokens=5, cost=50)
assert con.execute("SELECT COUNT(*) FROM remote_usage_cache WHERE owner_device_id='pc-b-0002' AND record_id='b1'").fetchone()[0] == 1
print('M1-06 SQLite remote breakdown oracle: PASS')
