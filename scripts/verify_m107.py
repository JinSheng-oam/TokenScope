#!/usr/bin/env python3
"""M1-07 production-readiness static gates; these are not runtime E2E tests."""
from pathlib import Path
import json

ROOT = Path(__file__).resolve().parents[1]
checks = []
def check(name, ok):
    checks.append((name, bool(ok)))
    print(f"{'PASS' if ok else 'FAIL'} {name}")

compose = (ROOT / 'deploy/compose.yaml').read_text()
dockerfile = (ROOT / 'deploy/hub/Dockerfile').read_text()
caddy = (ROOT / 'deploy/caddy/Caddyfile').read_text()
agent = (ROOT / 'crates/tokenscope-agent/src/daemon.rs').read_text()
repo = '\n'.join((ROOT/'crates/local-store/src/repository'/p).read_text(encoding='utf-8') for p in ('analytics/remote.rs', 'analytics/dashboard.rs'))
ui = (ROOT / 'apps/desktop/src/lib/DevicesPage.svelte').read_text()
package = json.loads((ROOT / 'apps/desktop/package.json').read_text())

check('Hub listens on container interface, not host-published port', '0.0.0.0:8787' in compose and '8787:' not in compose)
check('Hub runs non-root and read-only', 'USER 10001:10001' in dockerfile and 'read_only: true' in compose and 'chown -R 10001:10001 /data' in compose)
check('Hub capabilities dropped and privilege escalation disabled', 'cap_drop: [ALL]' in compose and 'no-new-privileges:true' in compose)
check('Only Caddy publishes HTTP/HTTPS', '"80:80"' in compose and '"443:443"' in compose)
check('Hub data uses persistent named volume', 'hub-data:/data' in compose and 'hub-data:' in compose)
check('Caddy reverse proxies to Hub service', 'reverse_proxy hub:8787' in caddy)
check('Agent performs push and pull in same sync cycle', 'sync_device_once(store' in agent and 'pull_device_once(store' in agent)
check('Remote cache excludes tombstones and non-device-local records', 'is_deleted=0' in repo and "'device_local'" in repo)
check('Remote dimensions retain device identity', 'owner_device_id' in repo and 'remote_by_tool' in repo and 'remote_by_model' in repo and 'remote_by_project' in repo)
check('UI contains remote device selector', 'remoteSelectedDevice' in ui and '选择远端透视设备' in ui)
check('Desktop package has independent check/test/build gates', all(k in package['scripts'] for k in ['check','test','build']))
check('No plain HTTP allowed for non-loopback Hub URL', 'is_loopback_host' in (ROOT/'crates/sync-client/src/url.rs').read_text())

failed = [n for n, ok in checks if not ok]
print(f'M1-07 static gates: {len(checks)-len(failed)}/{len(checks)} passed')
if failed:
    raise SystemExit(1)
