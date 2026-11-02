#!/usr/bin/env python3
"""Static security/contract checks for M0-10. This is NOT a Rust build test."""
from pathlib import Path
import json

ROOT = Path(__file__).resolve().parents[1]
rpc = (ROOT / "crates/tokenscope-agent/src/antigravity_rpc.rs").read_text()
main = (ROOT / "crates/tokenscope-agent/src/main.rs").read_text()
manifest = (ROOT / "crates/tokenscope-agent/Cargo.toml").read_text()
store = (ROOT / "crates/local-store/src/repository.rs").read_text()

checks = {
    "loopback URL is hard-coded": '"{scheme}://127.0.0.1:{}/{SERVICE}/{method}"' in rpc,
    "proxy disabled": ".no_proxy()" in rpc,
    "redirects disabled": "redirect(Policy::none())" in rpc,
    "local self-signed TLS only with loopback URL": ".danger_accept_invalid_certs(true)" in rpc and "127.0.0.1" in rpc,
    "CSRF header is sent": '"x-codeium-csrf-token"' in rpc,
    "CSRF is not logged": "println!(\"{}\", endpoint.csrf" not in rpc and "eprintln!(\"{}\", endpoint.csrf" not in rpc,
    "OAuth/browser credential code absent": all(x not in rpc.lower() for x in ["refresh_token", "access_token", "authorization_url", 'header("cookie"']),
    "response size bounded": "MAX_RESPONSE_BYTES" in rpc and "response.take(" in rpc,
    "quota is persisted through separate quota table API": "upsert_antigravity_quotas" in rpc and "antigravity_quota_snapshots" in store,
    "quota refresh CLI wired": "--refresh-antigravity-quota" in main and "refresh_and_store" in main,
    "legacy windows are not guessed": 'window_id: "other"' in rpc,
    "local account namespace contains no email": 'LOCAL_SOURCE_ID: &str = "local_antigravity"' in rpc and "source_id_from_email" not in rpc,
    "reqwest restricted features": 'features = ["blocking", "json", "rustls-tls"]' in manifest and "default-features = false" in manifest,
    "loopback request test present": "local_rpc_posts_to_loopback_with_csrf_and_parses_json" in rpc,
    "redirect regression test present": "local_rpc_does_not_follow_redirects" in rpc,
    "Windows quoted path regression present": "shell_tokenizer_preserves_quoted_windows_paths" in rpc,
    "duplicate quota pool regression present": "grouped_duplicate_pool_keeps_lowest_remaining_quota" in rpc,
}

for name, passed in checks.items():
    print(("PASS" if passed else "FAIL") + "  " + name)

fixture = json.loads((ROOT / "fixtures/antigravity/quota-example.json").read_text())
assert isinstance(fixture, list) and fixture, "quota fixture must remain a non-empty array"
assert all("remaining_basis_points" in row for row in fixture), "quota fixture schema drift"
summary = json.loads((ROOT / "fixtures/antigravity/rpc/grouped-summary-synthetic.json").read_text())
legacy = json.loads((ROOT / "fixtures/antigravity/rpc/legacy-user-status-synthetic.json").read_text())
assert len(summary["groups"]) == 3 and len(legacy["userStatus"]["cascadeModelConfigData"]["clientModelConfigs"]) == 3
assert summary["groups"][0]["buckets"][0]["remainingFraction"] == 0.0
assert summary["groups"][1]["buckets"][0]["disabled"] is True
print("PASS  grouped and legacy RPC synthetic fixtures parse")

if not all(checks.values()):
    raise SystemExit(1)
print("M0-10 static contract checks passed (Rust compilation is not tested here).")
