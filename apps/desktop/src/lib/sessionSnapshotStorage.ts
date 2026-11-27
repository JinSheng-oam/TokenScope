import type { DashboardRange, SessionAnalysisPage } from './types';

const MAX_SIZE = 250_000;
const MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000;
const STORAGE_PREFIX = 'tokenscope.sessionSnapshot.v1.';
const ALL_RANGES: DashboardRange[] = ['today', '7d', '30d', 'all', 'custom'];

const storageKey = (range: DashboardRange) => `${STORAGE_PREFIX}${range}`;

interface StoredSessionSnapshot {
  key: string;
  savedAtMs: number;
  savedDate: string;
  result: SessionAnalysisPage;
}

function localDateKey(nowMs: number): string {
  try {
    return new Intl.DateTimeFormat('en-CA').format(nowMs);
  } catch {
    return new Date(nowMs).toISOString().slice(0, 10);
  }
}

export function readSessionSnapshot(range: DashboardRange, key: string): SessionAnalysisPage | null {
  try {
    if (typeof localStorage === 'undefined') return null;
    const raw = localStorage.getItem(storageKey(range));
    if (!raw || raw.length > MAX_SIZE) return null;
    const parsed = JSON.parse(raw) as StoredSessionSnapshot;
    if (!parsed || parsed.key !== key || !parsed.result?.page) return null;
    const age = Date.now() - parsed.savedAtMs;
    if (!Number.isFinite(age) || age < 0 || age > MAX_AGE_MS) return null;
    if (range !== 'all' && range !== 'custom' && parsed.savedDate !== localDateKey(Date.now())) {
      return null;
    }
    return parsed.result;
  } catch {
    return null;
  }
}

export function saveSessionSnapshot(range: DashboardRange, key: string, result: SessionAnalysisPage) {
  try {
    if (typeof localStorage === 'undefined') return;
    const now = Date.now();
    const payload: StoredSessionSnapshot = {
      key,
      savedAtMs: now,
      savedDate: localDateKey(now),
      result,
    };
    const serialized = JSON.stringify(payload);
    if (serialized.length <= MAX_SIZE) {
      localStorage.setItem(storageKey(range), serialized);
    }
  } catch {
    // Storage is optional and may be unavailable or full.
  }
}

export function clearSessionSnapshots() {
  try {
    if (typeof localStorage === 'undefined') return;
    for (const range of ALL_RANGES) {
      localStorage.removeItem(storageKey(range));
    }
  } catch {
    // Ignore storage cleanup errors.
  }
}
