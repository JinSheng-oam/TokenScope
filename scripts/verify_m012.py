#!/usr/bin/env python3
"""Static contract checks for M0-12 packaging and lifecycle wiring."""
from pathlib import Path
import json
import re

ROOT = Path(__file__).resolve().parents[1]
checks = []
def check(name, condition):
    checks.append((name, bool(condition)))

cfg = json.loads((ROOT / 'apps/desktop/src-tauri/tauri.conf.json').read_text())
cap = json.loads((ROOT / 'apps/desktop/src-tauri/capabilities/default.json').read_text())
package = json.loads((ROOT / 'apps/desktop/package.json').read_text())
lib = '\n'.join((ROOT/'apps/desktop/src-tauri/src'/p).read_text(encoding='utf-8') for p in ('lib.rs', 'lifecycle.rs', 'agent.rs', 'state.rs'))
main = '\n'.join((ROOT/'crates/tokenscope-agent/src'/p).read_text(encoding='utf-8') for p in ('cli.rs', 'daemon.rs'))
settings = (ROOT / 'apps/desktop/src/lib/SettingsPage.svelte').read_text(encoding='utf-8')
api = (ROOT / 'apps/desktop/src/lib/api.ts').read_text()
workflow = (ROOT / '.github/workflows/desktop-ci.yml').read_text()
release = (ROOT / '.github/workflows/release.yml').read_text()

check('Tauri bundling enabled', cfg['bundle']['active'] is True)
check('All platform bundle targets configured', cfg['bundle']['targets'] == 'all')
check('Agent resource glob configured', 'resources/tokenscope-agent*' in cfg['bundle']['resources'])
check('Agent build metadata included', 'resources/agent-build.json' in cfg['bundle']['resources'])
check('Main WebView is lazy-created', cfg['app']['windows'][0].get('create') is False)
check('Tray feature enabled', 'tray-icon' in (ROOT / 'apps/desktop/src-tauri/Cargo.toml').read_text())
check('Autostart plugin dependency configured', 'tauri-plugin-autostart' in (ROOT / 'apps/desktop/src-tauri/Cargo.toml').read_text())
check('Autostart plugin initialized', 'tauri_plugin_autostart::init' in lib and '--background' in lib)
check('Tray menu contains open and quit', '"open"' in lib and '"quit"' in lib)
check('Window close destroys WebView but keeps tray app', 'prevent_close()' in lib and 'window.destroy()' in lib)
check('Background autostart does not create WebView', 'let is_bg = env::args().any(|arg| arg == "--background")' in lib and 'if !is_bg {' in lib)
check('Agent starts before optional WebView creation', lib.index('start_agent_inner(app.handle()') < lib.index('show_or_create_main_window(app.handle())'))
check('Agent process handle retained', 'Mutex<Option<Child>>' in lib)
check('Explicit Quit is distinguished from window close', '!lifecycle.quit_requested.load' in lib and re.search(r'keep_alive_after_window_close\s*\.swap\(false', lib))
check('Agent is started during app setup', 'start_agent_inner(app.handle()' in lib)
check('Lazy window creation uses config', 'WebviewWindowBuilder::from_config' in lib)
check('macOS Dock reopen recreates the WebView', re.search(r'RunEvent::Reopen\s*\{\s*has_visible_windows: false,\s*\.\.\s*\}', lib))
check('Local IANA timezone used for background startup', 'iana_time_zone::get_timezone()' in lib)
check('Agent cooperative shutdown passed', '--shutdown-file' in lib and '--shutdown-file' in main)
check('Shutdown marker consumed by daemon', 'shutdown::consume' in main)
check('Autostart frontend API wired', 'get_autostart_enabled' in api and 'set_autostart_enabled' in api)
check('Autostart setting visible', '登录系统后在后台启动采集器' in settings)
check('Debug staging script wired', 'stage-agent.mjs --debug' in package['scripts']['pretauri:dev'])
check('Release staging script wired', 'stage-agent.mjs --release' in package['scripts']['pretauri:build'])
check('Desktop CI builds installers', 'npx tauri build --bundles' in workflow)
check('Desktop CI uploads installers', 'actions/upload-artifact@v4' in workflow)
artifact_labels = set(re.findall(r'^\s*- os: ([-\w.]+)$', workflow, re.M))
release_artifacts = set(re.findall(r'^\s*"tokenscope-([-\w.]+)": \(', release, re.M))
check('Release collects the uploaded installer groups',
      'name: tokenscope-${{ matrix.os }}' in workflow and len(artifact_labels) == 3 and artifact_labels == release_artifacts)
check('Desktop CI lints the Tauri Rust shell', 'cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml' in workflow)
check('CI installs frontend dependencies from the lockfile', 'npm ci --no-audit --no-fund' in workflow)

failed = [name for name, ok in checks if not ok]
for name, ok in checks:
    print(f"{'PASS' if ok else 'FAIL'} {name}")
print(f"\n{len(checks)-len(failed)}/{len(checks)} checks passed")
if failed:
    raise SystemExit(1)
