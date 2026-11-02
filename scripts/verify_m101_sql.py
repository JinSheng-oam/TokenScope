#!/usr/bin/env python3
"""Independent SQLite oracle for M1-01 DDL and transaction invariants.
This does not replace Go/modernc integration tests.
"""
from pathlib import Path
import hashlib
import sqlite3
import tempfile

ROOT = Path(__file__).resolve().parents[1]
DDL = (ROOT / "hub/internal/sqlstore/migrations/001.sql").read_text(encoding="utf-8")

with tempfile.TemporaryDirectory() as td:
    db = sqlite3.connect(Path(td) / "hub.db", isolation_level=None)
    db.execute("PRAGMA foreign_keys=ON")
    db.executescript(DDL)
    db.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES(1,1)")
    db.execute("PRAGMA user_version=1")

    code = "pair-code-one-time-example"
    code_hash = hashlib.sha256(code.encode()).hexdigest()
    token = "device-token-secret-example"
    token_hash = hashlib.sha256(token.encode()).hexdigest()
    db.execute("INSERT INTO pairing_codes(code_sha256,created_at_ms,expires_at_ms) VALUES(?,?,?)", (code_hash,100,400))

    db.execute("BEGIN IMMEDIATE")
    changed = db.execute("UPDATE pairing_codes SET consumed_at_ms=? WHERE code_sha256=? AND consumed_at_ms IS NULL AND expires_at_ms>?", (200,code_hash,200)).rowcount
    assert changed == 1
    db.execute("INSERT INTO devices(device_id,display_name,os_name,app_version,time_zone,created_at_ms) VALUES(?,?,?,?,?,?)", ("office-pc-01","Office PC","windows","0.1.0","Asia/Shanghai",200))
    db.execute("INSERT INTO device_credentials(device_id,token_sha256,created_at_ms) VALUES(?,?,?)", ("office-pc-01",token_hash,200))
    db.execute("COMMIT")

    # One-time code replay must not consume/register another device.
    db.execute("BEGIN IMMEDIATE")
    changed = db.execute("UPDATE pairing_codes SET consumed_at_ms=? WHERE code_sha256=? AND consumed_at_ms IS NULL AND expires_at_ms>?", (201,code_hash,201)).rowcount
    assert changed == 0
    db.execute("ROLLBACK")

    # Duplicate device registration rolls back the code consumption as one transaction.
    code2_hash = hashlib.sha256(b"second-code").hexdigest()
    db.execute("INSERT INTO pairing_codes(code_sha256,created_at_ms,expires_at_ms) VALUES(?,?,?)", (code2_hash,100,500))
    db.execute("BEGIN IMMEDIATE")
    assert db.execute("UPDATE pairing_codes SET consumed_at_ms=300 WHERE code_sha256=? AND consumed_at_ms IS NULL AND expires_at_ms>300", (code2_hash,)).rowcount == 1
    try:
        db.execute("INSERT INTO devices(device_id,display_name,os_name,app_version,time_zone,created_at_ms) VALUES(?,?,?,?,?,?)", ("office-pc-01","Duplicate","windows","0.1.0","UTC",300))
    except sqlite3.IntegrityError:
        db.execute("ROLLBACK")
    else:
        raise AssertionError("duplicate device id unexpectedly accepted")
    assert db.execute("SELECT consumed_at_ms FROM pairing_codes WHERE code_sha256=?", (code2_hash,)).fetchone()[0] is None

    # Rotate credential atomically: old revoked, new active.
    token2_hash = hashlib.sha256(b"rotated-device-token").hexdigest()
    db.execute("BEGIN IMMEDIATE")
    assert db.execute("UPDATE device_credentials SET revoked_at_ms=400 WHERE device_id=? AND token_sha256=? AND revoked_at_ms IS NULL", ("office-pc-01",token_hash)).rowcount == 1
    db.execute("INSERT INTO device_credentials(device_id,token_sha256,created_at_ms) VALUES(?,?,?)", ("office-pc-01",token2_hash,400))
    db.execute("COMMIT")
    assert db.execute("SELECT revoked_at_ms FROM device_credentials WHERE token_sha256=?", (token_hash,)).fetchone()[0] == 400
    assert db.execute("SELECT revoked_at_ms FROM device_credentials WHERE token_sha256=?", (token2_hash,)).fetchone()[0] is None

    # Revoke device and all active credentials.
    db.execute("BEGIN IMMEDIATE")
    db.execute("UPDATE devices SET revoked_at_ms=500 WHERE device_id=? AND revoked_at_ms IS NULL", ("office-pc-01",))
    db.execute("UPDATE device_credentials SET revoked_at_ms=500 WHERE device_id=? AND revoked_at_ms IS NULL", ("office-pc-01",))
    db.execute("COMMIT")
    assert db.execute("SELECT revoked_at_ms FROM devices WHERE device_id=?", ("office-pc-01",)).fetchone()[0] == 500
    assert db.execute("SELECT COUNT(*) FROM device_credentials WHERE device_id=? AND revoked_at_ms IS NULL", ("office-pc-01",)).fetchone()[0] == 0

    # Raw secrets are never persisted; only 64-char SHA-256 digests are present.
    assert db.execute("SELECT COUNT(*) FROM pairing_codes WHERE code_sha256=?", (code,)).fetchone()[0] == 0
    assert db.execute("SELECT COUNT(*) FROM device_credentials WHERE token_sha256=?", (token,)).fetchone()[0] == 0
    assert db.execute("PRAGMA foreign_key_check").fetchall() == []
    assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    db.close()

print("M1-01 SQLite DDL/transaction oracle: PASS (pair-once, rollback, rotate, revoke, secret hashes, FK, integrity)")
