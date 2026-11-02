#!/usr/bin/env python3
"""Standalone Claude fixture oracle; DOES NOT compile or execute Rust."""
from pathlib import Path
import json

ROOT = Path(__file__).resolve().parents[1]
fixture = ROOT / 'fixtures/claude/projects/sample-project/session.jsonl'
rows = [json.loads(line) for line in fixture.read_text().splitlines()]
messages = {}
for row in rows:
    if row.get('type') != 'assistant':
        continue
    msg = row['message']
    u = msg['usage']
    assert u['cache_creation_input_tokens'] == sum(u['cache_creation'].values())
    tok = (u['input_tokens'], u['cache_read_input_tokens'], u['cache_creation_input_tokens'], u['output_tokens'])
    score = (sum(tok), tok[-1])
    if msg['id'] not in messages or score >= messages[msg['id']][0]:
        messages[msg['id']] = (score, tok, row['timestamp'])
assert len(messages) == 2, messages
assert sum(sum(r[1]) for r in messages.values()) == 143
assert messages['msg-1'][1] == (3, 80, 20, 18)
assert messages['msg-2'][1] == (5, 8, 0, 9)
assert messages['msg-1'][2][:10] == '2026-11-01'
assert messages['msg-2'][2][:10] == '2026-11-02'
source = (ROOT/'crates/source-claude/src/lib.rs').read_text()
for marker in ('fn bounded_line', 'pub fn discover_claude_files', 'pub fn scan_claude_file',
               'pub fn collect_claude', 'stable_record_id', 'last_complete_scan_ms',
               'store.reconcile_complete_source(', 'result.tombstoned', 'Precision::Estimated'):
    assert marker in source, marker
assert 'message.id' in source
agent = (ROOT/'crates/tokenscope-agent/src/main.rs').read_text()
for flag in ('--scan-codex', '--scan-claude', '--scan-all', '--claude-home', '--codex-home'):
    assert flag in agent, flag
print('PASS: Claude reference fixture, dedup/cache cross-date oracle and source wiring')
print('NOTE: Python reference checks only; Rust cargo build/tests still NOT RUN')
