import type { DashboardRange, DashboardSnapshot } from './types';

const MAX_SIZE = 400_000;
const MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000;
const storageKey = (range: DashboardRange) => `tokenscope.dashboardSnapshot.v1.${range}`;

export function readDashboardSnapshot(range: DashboardRange, key: string): DashboardSnapshot | null {
  try {
    const stored = localStorage.getItem(storageKey(range));
    if (!stored || stored.length > MAX_SIZE) return null;
    const { requestKey, snapshot } = JSON.parse(stored) as {
      requestKey: string;
      snapshot: DashboardSnapshot;
    };
    if (requestKey !== key || snapshot.range !== range || !snapshot.identity) return null;
    const age = Date.now() - snapshot.generated_at_ms;
    if (!Number.isFinite(age) || age < 0 || age > MAX_AGE_MS) return null;
    if (range !== 'all' && range !== 'custom') {
      const localDate = new Intl.DateTimeFormat('en-CA', { timeZone: snapshot.time_zone });
      if (localDate.format(snapshot.generated_at_ms) !== localDate.format(Date.now())) return null;
    }
    return snapshot;
  } catch {
    return null;
  }
}

export function saveDashboardSnapshot(range: DashboardRange, key: string, snapshot: DashboardSnapshot) {
  if (!snapshot.identity || snapshot.range !== range) return;
  try {
    const stored = JSON.stringify({ requestKey: key, snapshot });
    if (stored.length <= MAX_SIZE) localStorage.setItem(storageKey(range), stored);
  } catch {
    // Storage is optional and may be unavailable or full.
  }
}
