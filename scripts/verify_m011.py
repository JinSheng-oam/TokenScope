#!/usr/bin/env python3
"""Static contract checks for the M0-11 desktop dashboard bridge."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
checks = []

def check(name: str, condition: bool):
    checks.append((name, bool(condition)))
    print(f"{'PASS' if condition else 'FAIL'}  {name}")

agent = (ROOT / "crates/tokenscope-agent/src/main.rs").read_text()
dashboard = (ROOT / "crates/tokenscope-agent/src/dashboard.rs").read_text()
store = (ROOT / "crates/local-store/src/repository.rs").read_text()
tauri = (ROOT / "apps/desktop/src-tauri/src/lib.rs").read_text()
app = (ROOT / "apps/desktop/src/App.svelte").read_text()
api = (ROOT / "apps/desktop/src/lib/api.ts").read_text()
ipc = (ROOT / "crates/local-ipc/src/lib.rs").read_text()
css = (ROOT / "apps/desktop/src/app.css").read_text()

check("SQLite dashboard SQL has an executable regression oracle", (ROOT / "scripts/verify_m011_sql.py").is_file())
check("Agent exposes a JSON dashboard query", "--dashboard-json" in agent and "dashboard::read_snapshot" in agent)
check("stdio IPC has a versioned shared envelope", "PROTOCOL_VERSION: u32 = 1" in ipc and "DashboardResponse" in ipc)
check("dashboard query uses read-only SQLite", "LocalStore::open_readonly(path)" in dashboard)
check("read-only SQLite enforces query_only", "PRAGMA query_only=ON" in store)
check("dashboard scopes queries by the indexed owner device", "owner_device_id=?1 AND scope=?2" in store)
check("dashboard aggregates local usage by explicit scope", 'usage_totals(owner_device_id, "device_local"' in store)
check("unknown scope is shown separately", 'usage_totals(owner_device_id, "unknown"' in store)
check("account-global scope is shown separately", 'usage_totals(owner_device_id, "account_global"' in store)
check("deleted usage is excluded", "is_deleted=0" in store)
check("calendar boundaries use IANA timezone rules", "chrono_tz::Tz" in dashboard and "from_local_datetime" in dashboard)
check("DST spring/autumn regression tests exist", "dst_day_boundaries_are_not_forced_to_24_hours" in dashboard)
check("dashboard range is allowlisted in Agent", '["today", "7d", "30d", "all", "custom"]' in dashboard)
check("Tauri calls Agent as a child process, not SQLite", "--dashboard-json" in tauri and "Command::new" in tauri and "LocalStore" not in tauri)
check("Tauri drains Agent stdout/stderr concurrently", "stdout_reader" in tauri and "stderr_reader" in tauri)
check("Tauri caps Dashboard response size", "1_048_576" in tauri and "65_536" in tauri)
check("Tauri range is allowlisted", '["today", "7d", "30d", "all", "custom"]' in tauri)
check("Tauri never invokes a shell", "sh -c" not in tauri and "cmd.exe" not in tauri)
check("absolute database path is not exposed to WebView", "database_path:" not in tauri and "databasePath" not in app)
check("frontend uses Tauri invoke", "invoke<DashboardResponse>" in api)
hub_storage_calls = re.findall(r"window\.localStorage\.(?:getItem|setItem)\((.*?)\)", app)
check("frontend does not directly read files or open network connections; localStorage is Hub URL only", "fetch(" not in app and "import sqlite" not in app.lower() and len(hub_storage_calls) == 2 and all("tokenscope.hubUrl" in call for call in hub_storage_calls) and "device_token" not in app)
check("UI does not merge unknown/account-global totals into local total", "snapshot.unassigned.total_tokens" in app and "snapshot.account_global.total_tokens" in app)
check("Antigravity quota is labeled separately from token usage", "不是 Token 数量" in app)
check("project identifiers are displayed as short pseudonyms", "脱敏项目标识" in app)
check("no remote Google Fonts dependency", "fonts.googleapis.com" not in css)
check("Tauri auto-start keeps daemon independent from window lifetime", "--daemon" in tauri and ".spawn()" in tauri)

failed = [name for name, ok in checks if not ok]
if failed:
    raise SystemExit(f"M0-11 checks failed: {', '.join(failed)}")
print(f"M0-11 static contract checks passed ({len(checks)} checks).")
