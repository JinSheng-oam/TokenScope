#!/usr/bin/env python3
"""Execute the Rust Dashboard SQL against the real SQLite migration DDL."""
from pathlib import Path
import re
import sqlite3

ROOT = Path(__file__).resolve().parents[1]
schema_rs = (ROOT / "crates/local-store/src/schema.rs").read_text()
repo_rs = (ROOT / "crates/local-store/src/repository.rs").read_text()

m1 = re.search(r'MIGRATION_001: &str = r#"(.*?)"#;', schema_rs, re.S)
m2 = re.search(r'MIGRATION_002: &str = r#"(.*?)"#;', schema_rs, re.S)
assert m1 and m2, "migration SQL literals not found"

def rust_continued_string(pattern: str) -> str:
    match = re.search(pattern, repo_rs, re.S)
    assert match, f"Rust SQL literal not found: {pattern}"
    # Rust string continuation removes the backslash, newline and indentation.
    return re.sub(r"\\\n\s*", "", match.group(1))

totals_sql = rust_continued_string(r'let row = self\.conn\.query_row\(\s*"(.*?)",\s*params!')
breakdown_template = rust_continued_string(r'let sql = format!\(\s*"(.*?)"\s*\);')

conn = sqlite3.connect(":memory:")
conn.executescript(m1.group(1))
conn.executescript(m2.group(1))
conn.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES(1,1)")
conn.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES(2,2)")
conn.execute("PRAGMA user_version=2")
conn.execute("INSERT INTO local_identity(singleton,device_id,display_name,time_zone) VALUES(1,'device-a','Office PC','Asia/Shanghai')")

rows = [
    ("a" * 64, "device_local", 1500),
    ("b" * 64, "unknown", 1500),
    ("c" * 64, "account_global", 1500),
    ("d" * 64, "device_local", None),
]
for record_id, scope, event_at in rows:
    conn.execute(
        """INSERT INTO usage_records(
            record_id,owner_device_id,tool_id,scope,account_id,model_id,project_id,event_at_ms,
            input_uncached_tokens,cache_read_tokens,cache_write_tokens,output_tokens,unclassified_tokens,
            reasoning_tokens,estimated_cost_microusd,price_version,precision,record_revision,payload_hash,
            is_deleted,observed_at_ms,updated_at_ms
        ) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
        (record_id, "device-a", "codex", scope,
         "acct_" + "a" * 64 if scope == "account_global" else None,
         "gpt-5", "prj_" + "b" * 64, event_at,
         100, 10, 5, 50, 0, 20, 1250, "test-price-v1", "aggregate", 1,
         "f" * 64, 0, 2000, 2000),
    )

for scope in ("device_local", "unknown", "account_global"):
    result = conn.execute(totals_sql, ("device-a", scope, 1000, 3000)).fetchone()
    assert result[0] == 165 and result[8] == 1, (scope, result)

# All-time scope must include the untimed local row; a calendar bucket must not.
all_time = conn.execute(totals_sql, ("device-a", "device_local", None, None)).fetchone()
calendar = conn.execute(totals_sql, ("device-a", "device_local", 1000, 3000)).fetchone()
assert all_time[0] == 330 and all_time[8] == 2
assert calendar[0] == 165 and calendar[8] == 1

breakdown_sql = breakdown_template.replace("{column}", "model_id")
model = conn.execute(breakdown_sql, ("device-a", "device_local", 1000, 3000)).fetchone()
assert model[0] == "gpt-5" and model[2] == 165 and model[1] == 1

print("M0-11 SQLite migration + extracted Rust Dashboard SQL: PASS")
