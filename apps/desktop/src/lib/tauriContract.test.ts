import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

function collectSourceFiles(dir: string): string[] {
  const results: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const stat = statSync(full);
    if (stat.isDirectory()) {
      results.push(...collectSourceFiles(full));
    } else if (
      (full.endsWith('.ts') && !full.endsWith('.test.ts') && !full.endsWith('.d.ts')) ||
      full.endsWith('.svelte')
    ) {
      results.push(full);
    }
  }
  return results;
}

describe('Tauri invoke <-> generate_handler contract', () => {
  it('registers every frontend-invoked Tauri command in src-tauri/src/lib.rs', () => {
    const desktopRoot = resolve(__dirname, '../..');
    const libRsPath = resolve(desktopRoot, 'src-tauri/src/lib.rs');
    const libRs = readFileSync(libRsPath, 'utf8');

    const handlerMatch = libRs.match(/tauri::generate_handler!\[([\s\S]*?)\]/);
    expect(handlerMatch).not.toBeNull();

    const registeredCommands = new Set(
      handlerMatch![1]
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean)
        .map((item) => item.split('::').pop()!)
    );

    const srcRoot = resolve(desktopRoot, 'src');
    const files = collectSourceFiles(srcRoot);
    const invokePattern = /\binvoke\s*(?:<[^>]*>)?\s*\(\s*['"]([a-zA-Z0-9_]+)['"]/g;

    const invokedCommands = new Set<string>();
    const missing: Array<{ file: string; command: string }> = [];

    for (const file of files) {
      const content = readFileSync(file, 'utf8');
      for (const match of content.matchAll(invokePattern)) {
        const command = match[1];
        invokedCommands.add(command);
        if (!registeredCommands.has(command)) {
          missing.push({ file: file.replace(desktopRoot, ''), command });
        }
      }
    }

    expect(invokedCommands.size).toBeGreaterThan(20);
    expect(missing).toEqual([]);
  });
});
