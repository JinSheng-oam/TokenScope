#!/usr/bin/env python3
"""Independent fixture-based semantic oracle, NOT Rust execution.

Checks cumulative Codex counters, repetition, cache subset and fixture shape.
Can be run offline on any platform with Python 3.
"""
from pathlib import Path
import json
import tomllib

root = Path(__file__).resolve().parents[1]
fixture = root / 'fixtures/codex/rollout-synthetic.jsonl'
rows = [json.loads(line) for line in fixture.read_text().splitlines()]
assert rows[0]['type'] == 'session_meta'
totals = []
for row in rows:
    p = row.get('payload') or {}
    if row['type'] != 'event_msg' or p.get('type') != 'token_count':
        continue
    counts = p.get('info', {}).get('total_token_usage')
    if not counts:
        continue
    assert counts['cached_input_tokens'] <= counts['input_tokens']
    assert counts['reasoning_output_tokens'] <= counts['output_tokens']
    assert counts['input_tokens'] + counts['output_tokens'] <= counts['total_tokens']
    totals.append(counts)
assert len(totals) == 3
assert totals[1] == totals[2], 'rate-limit-only update should not count as usage'
last = totals[-1]
assert last['total_tokens'] == 180
assert last['input_tokens'] - last['cached_input_tokens'] == 110
assert last['output_tokens'] == 30
assert sum(x['total_tokens'] for x in totals) != last['total_tokens']

source = '\n'.join((root/'crates/source-codex/src'/p).read_text(encoding='utf-8') for p in ('lib.rs', 'model.rs', 'rollout.rs', 'scanner.rs'))
assert 'total_token_usage' in source and 'last_token_usage' in source
assert 'session-total-v1' in source
assert 'read_offset' in source and 'bounded_line' in source
agent = '\n'.join((root/'crates/tokenscope-agent/src'/p).read_text(encoding='utf-8') for p in ('cli.rs', 'collection.rs'))
assert '--scan-codex' in agent and 'collect_codex_precise_tolerant(' in agent
precise = '\n'.join((root/'crates/source-codex/src/precise'/p).read_text(encoding='utf-8') for p in ('collector.rs', 'model.rs', 'scanner.rs'))
assert 'token_usage_record' in precise and 'total_token_usage' in source
assert 'full_reconcile' in precise and 'tombstone' in precise
cargo = tomllib.loads((root/'crates/source-codex/Cargo.toml').read_text())
assert 'tokenscope-local-store' in cargo['dependencies']
print('PASS M0-04 legacy fixture & M0-05 agent entry checks (not Rust compilation)')
