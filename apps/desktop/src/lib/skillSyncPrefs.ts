import type { Provider } from './agentCustomizationEntries';
import type { SkillSyncMode } from './skillLibraryTypes';

export type ProviderSyncPrefs = Record<Provider, SkillSyncMode>;

const STORAGE_KEY = 'tokenscope:provider-sync-prefs';

const PROVIDERS: Provider[] = ['claude', 'codex', 'antigravity', 'pi', 'deepseek'];

export const DEFAULT_PROVIDER_SYNC_PREFS: ProviderSyncPrefs = {
  claude: 'link',
  codex: 'link',
  antigravity: 'link',
  pi: 'link',
  deepseek: 'link',
};

export function loadProviderSyncPrefs(): ProviderSyncPrefs {
  const result: ProviderSyncPrefs = { ...DEFAULT_PROVIDER_SYNC_PREFS };
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY);
    if (!raw) return result;
    const parsed = JSON.parse(raw) as Partial<Record<Provider, string>>;
    for (const p of PROVIDERS) {
      if (parsed[p] === 'copy' || parsed[p] === 'link') {
        result[p] = parsed[p] as SkillSyncMode;
      }
    }
    return result;
  } catch {
    return result;
  }
}

export function saveProviderSyncPrefs(prefs: ProviderSyncPrefs): ProviderSyncPrefs {
  const normalized: ProviderSyncPrefs = { ...DEFAULT_PROVIDER_SYNC_PREFS };
  for (const p of PROVIDERS) {
    normalized[p] = prefs[p] === 'copy' ? 'copy' : 'link';
  }
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, JSON.stringify(normalized));
  } catch {
    // ignore
  }
  return normalized;
}

export function getProviderConfiguredSyncMode(
  prefs: ProviderSyncPrefs,
  provider: Provider
): SkillSyncMode {
  return prefs[provider] === 'copy' ? 'copy' : 'link';
}

export function setProviderConfiguredSyncMode(
  prefs: ProviderSyncPrefs,
  provider: Provider,
  mode: SkillSyncMode
): ProviderSyncPrefs {
  return saveProviderSyncPrefs({ ...prefs, [provider]: mode });
}

export function syncModeLabel(mode: SkillSyncMode): string {
  return mode === 'copy' ? '文件复制' : '静态链接';
}
