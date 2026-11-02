import { createHash } from 'node:crypto';
import { chmod, copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(scriptDir, '..');
const mode = process.argv[2] === '--release' ? 'release' : 'debug';
const isWindows = process.platform === 'win32';
const binaryName = isWindows ? 'tokenscope-agent.exe' : 'tokenscope-agent';
const source = path.join(root, 'target', mode, binaryName);
const resourcesDir = path.join(root, 'apps', 'desktop', 'src-tauri', 'resources');
const destination = path.join(resourcesDir, binaryName);

await mkdir(resourcesDir, { recursive: true });
await copyFile(source, destination);
if (!isWindows) await chmod(destination, 0o755);
const bytes = await readFile(destination);
const workspaceManifest = await readFile(path.join(root, 'Cargo.toml'), 'utf8');
const agentVersion = workspaceManifest.match(/\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1] ?? 'unknown';
const metadata = {
  appVersion: JSON.parse(await readFile(path.join(root, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json'), 'utf8')).version,
  agentVersion,
  buildProfile: mode,
  targetPlatform: process.platform,
  targetArch: process.arch,
  binary: binaryName,
  sha256: createHash('sha256').update(bytes).digest('hex'),
  stagedAt: new Date().toISOString(),
};
await writeFile(path.join(resourcesDir, 'agent-build.json'), `${JSON.stringify(metadata, null, 2)}\n`, 'utf8');
console.log(`Staged ${source} -> ${destination} (${bytes.length} bytes)`);
