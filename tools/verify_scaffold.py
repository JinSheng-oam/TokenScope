#!/usr/bin/env python3
"""Dependency-free structural verification for the cumulative TokenScope workspace (M0-12 + M1-01).

Not a replacement for cargo/npm/tauri compilation.
"""
from __future__ import annotations
import json
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]

def check() -> None:
    workspace = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))
    members = workspace['workspace']['members']
    required = {
        'crates/core-domain', 'crates/source-claude', 'crates/source-codex',
        'crates/source-opencode', 'crates/source-antigravity',
        'crates/collector-engine', 'crates/local-store', 'crates/sync-client',
        'crates/local-ipc', 'crates/tokenscope-agent',
    }
    assert required <= set(members), f'missing Rust crates: {required - set(members)}'
    assert len(members) == len(set(members)), 'duplicate workspace member' 
    for member in members:
        cargo = tomllib.loads((ROOT / member / 'Cargo.toml').read_text(encoding='utf-8'))
        assert 'package' in cargo
        assert (ROOT / member / 'src').exists()
    pkg = json.loads((ROOT / 'apps/desktop/package.json').read_text(encoding='utf-8'))
    assert {'dev','check','test','build','tauri:dev','tauri:build'} <= set(pkg['scripts'])
    tauri = json.loads((ROOT / 'apps/desktop/src-tauri/tauri.conf.json').read_text(encoding='utf-8'))
    assert tauri['build']['frontendDist'] == '../dist'
    assert tauri['bundle']['active'] is True
    assert (ROOT / 'hub/go.mod').exists()
    assert (ROOT / 'hub/internal/sqlstore/migrations/001.sql').is_file()
    assert (ROOT / 'scripts/verify_m101_sql.py').is_file()
    for path in ['rust-ci.yml','desktop-ci.yml','hub-ci.yml']:
        assert (ROOT / '.github/workflows' / path).is_file()
    docs = ROOT / 'docs'
    if docs.is_dir():  # Local development docs are not in a GitHub checkout.
        for path in ['README.md', 'architecture.md', 'development.md']:
            assert (docs / path).is_file()
    assert (ROOT / 'README.md').is_file()
    print(f'PASS cumulative structural verification ({len(members)} Rust packages; bundled Svelte/Tauri shell; Go Hub M1-01; 3 CI workflows)')

if __name__ == '__main__':
    try:
        check()
    except (OSError, AssertionError, ValueError, KeyError) as exc:
        print(f'FAIL cumulative workspace structural verification: {exc}', file=sys.stderr)
        sys.exit(1)
