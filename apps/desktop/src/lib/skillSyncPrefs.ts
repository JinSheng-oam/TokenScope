import type { SkillSyncMode } from './skillLibraryTypes';

export type SkillSyncOverride = 'inherit' | SkillSyncMode;

export type SkillSyncPrefs = {
  defaultMode: SkillSyncMode;
  overrides: Record<string, SkillSyncMode>;
};

const STORAGE_KEY = 'tokenscope:skill-sync-prefs';

export const DEFAULT_SYNC_PREFS: SkillSyncPrefs = {
  defaultMode: 'link',
  overrides: {},
};

export function loadSkillSyncPrefs(): SkillSyncPrefs {
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY);
    if (!raw) return { defaultMode: 'link', overrides: {} };
    const parsed = JSON.parse(raw) as Partial<SkillSyncPrefs>;
    const defaultMode: SkillSyncMode = parsed.defaultMode === 'copy' ? 'copy' : 'link';
    const overrides: Record<string, SkillSyncMode> = {};
    if (parsed.overrides && typeof parsed.overrides === 'object') {
      for (const [k, v] of Object.entries(parsed.overrides)) {
        if (v === 'link' || v === 'copy') overrides[k] = v;
      }
    }
    return { defaultMode, overrides };
  } catch {
    return { defaultMode: 'link', overrides: {} };
  }
}

export function saveSkillSyncPrefs(prefs: SkillSyncPrefs): SkillSyncPrefs {
  const normalized: SkillSyncPrefs = {
    defaultMode: prefs.defaultMode === 'copy' ? 'copy' : 'link',
    overrides: { ...prefs.overrides },
  };
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, JSON.stringify(normalized));
  } catch {
    // ignore
  }
  return normalized;
}

export function getSkillSyncOverride(prefs: SkillSyncPrefs, skillKey: string): SkillSyncOverride {
  return prefs.overrides[skillKey] ?? 'inherit';
}

export function resolveSkillSyncMode(prefs: SkillSyncPrefs, skillKey: string): SkillSyncMode {
  return prefs.overrides[skillKey] ?? prefs.defaultMode;
}

export function setSkillSyncOverride(
  prefs: SkillSyncPrefs,
  skillKey: string,
  override: SkillSyncOverride
): SkillSyncPrefs {
  const nextOverrides = { ...prefs.overrides };
  if (override === 'inherit') {
    delete nextOverrides[skillKey];
  } else {
    nextOverrides[skillKey] = override;
  }
  return saveSkillSyncPrefs({ ...prefs, overrides: nextOverrides });
}

export function syncModeLabel(mode: SkillSyncMode): string {
  return mode === 'copy' ? '文件复制' : '静态链接';
}
