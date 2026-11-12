#!/usr/bin/env python3
"""Static M1-02 pairing/client security contract checks (not Rust compilation)."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
checks = []

def check(name: str, condition: bool):
    checks.append((name, bool(condition)))
    print(f"{'PASS' if condition else 'FAIL'}  {name}")

cargo = (ROOT / "crates/sync-client/Cargo.toml").read_text()
client = (ROOT / "crates/sync-client/src/client.rs").read_text()
url = (ROOT / "crates/sync-client/src/url.rs").read_text()
creds = (ROOT / "crates/sync-client/src/credentials.rs").read_text()
pairing = (ROOT / "crates/sync-client/src/pairing.rs").read_text()
tauri = (ROOT / "apps/desktop/src-tauri/src/hub.rs").read_text()
app = (ROOT / 'apps/desktop/src/App.svelte').read_text(encoding='utf-8')
controller = (ROOT / 'apps/desktop/src/lib/hubController.ts').read_text(encoding='utf-8')
wire = (ROOT / 'crates/core-domain/src/record.rs').read_text(encoding='utf-8')
api = (ROOT / "apps/desktop/src/lib/api.ts").read_text()
rust_ci = (ROOT / ".github/workflows/rust-ci.yml").read_text()
desktop_ci = (ROOT / ".github/workflows/desktop-ci.yml").read_text()

check("reqwest uses blocking + rustls without default features", 'default-features = false' in cargo and '"blocking"' in cargo and '"rustls-tls"' in cargo)
check("Hub HTTP disables ambient proxies", ".no_proxy()" in client)
check("Hub HTTP never follows redirects", "redirect(Policy::none())" in client)
check("Hub HTTP has connect/total timeouts and response cap", "from_secs(5)" in client and "from_secs(15)" in client and "MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024" in client and ".take(MAX_RESPONSE_BYTES + 1)" in client)
check("Hub URL allows HTTPS or loopback HTTP only", '"https" => {}' in url and '"http" if is_loopback_host(host)' in url)
check("Hub URL rejects userinfo, path, query and fragment", "parsed.username()" in url and "parsed.password()" in url and "parsed.query()" in url and "parsed.fragment()" in url and 'matches!(parsed.path(), "" | "/")' in url)
check("pairing POST is not retried on ambiguous outcome", "PairOutcomeUnknown" in client and "deliberately not retried" in client and "request.send()" in client)
check("native keyring backend configured for all three desktop OSes", all(x in cargo for x in ["windows-native", "apple-native", "linux-native-sync-persistent"]))
check("Linux keyring uses Rust crypto backend", '"crypto-rust"' in cargo)
check("credential key is scoped by Hub origin and device ID hash", "tokenscope/hub-credential/v1" in creds and "hub.origin()" in creds and "device_id.as_bytes()" in creds and "Sha256" in creds)
check("credential store uses native Entry API", "keyring::Entry::new" in creds and "set_password" in creds and "get_password" in creds and "delete_credential" in creds)
check("pairing code and device token zeroized on drop", "impl Drop for PairRequest" in pairing and "impl Drop for DeviceToken" in pairing and "zeroize" in cargo)
check("secret values are redacted in Debug", '"[REDACTED]"' in pairing and '"[SCOPED]"' in creds)
check("Tauri pairs using existing Agent device identity", "sync_context(&app, &hub_url)" in tauri and "PairRequest" in tauri and "device_id: device_id.clone()" in tauri)
check("keyring preflight occurs before one-time pairing", re.search(r'store\.preflight\(\).*client\.pair\(&request\).*store\.save\(&key, &response\.device_token\)', tauri, re.S))
check("blocking network and keyring commands run in spawn_blocking", tauri.count("tauri::async_runtime::spawn_blocking(move ||") >= 5)
check("frontend receives no device bearer token", "deviceToken" not in app + api + controller and "Bearer " not in app + api)
check("frontend stores only non-secret Hub origin", "const HUB_ORIGIN_KEY = 'tokenscope.hubUrl'" in controller and re.findall(r'runtime\.storage\.setItem\(([^\n]+)\);', controller) == ['HUB_ORIGIN_KEY, hubUrl', 'HUB_ORIGIN_KEY, result.hubOrigin'])
check("frontend uses Tauri invoke and not direct network", "invoke<" in api and "fetch(" not in app and "fetch(" not in api)
check("Tauri exposes pair/list/rename/rotate/disconnect commands", all(name in tauri for name in ["pair_hub_device", "list_hub_devices", "rename_hub_device", "rotate_hub_device_token", "disconnect_hub_device"]))
check("local disconnect explicitly does not revoke Hub device", re.search(r'fn disconnect_hub_device\(.*?KeyringCredentialStore\s*\.delete\(&key\).*?Ok\(deleted\)', tauri, re.S) and 'revoke' not in tauri.split('fn disconnect_hub_device(', 1)[-1])
record_fields = wire.split('pub struct CanonicalRecord {', 1)[-1].split('\n}', 1)[0]
check('synchronized record carries usage metadata without prompts or source code', 'pub usage:' in record_fields and not re.search(r'pub (prompt|response|source_code|api_key)\w*:', record_fields))
check("Linux CI installs DBus build dependency", "libdbus-1-dev" in rust_ci and "libdbus-1-dev" in desktop_ci)
check("Rust test cases added for URL, HTTP, keyring and DTOs", len(re.findall(r"#\[test\]", client + url + creds + pairing)) >= 20)

failed = [name for name, passed in checks if not passed]
print(f"M1-02 static contract checks: {len(checks) - len(failed)}/{len(checks)} passed")
if failed:
    raise SystemExit(1)
