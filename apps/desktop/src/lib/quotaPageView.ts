import { quotaIsStale } from './dashboardView';
import type { CodexQuota } from './types';

export function filterActiveCodexQuotas(rows: readonly CodexQuota[]): CodexQuota[] {
  const poolKey = (row: CodexQuota) => JSON.stringify([row.source_id, row.pool_id]);
  const latest = new Map<string, number>();
  for (const row of rows) {
    const key = poolKey(row);
    latest.set(key, Math.max(latest.get(key) ?? 0, row.observed_at_ms));
  }
  return rows.filter((row) => row.observed_at_ms === latest.get(poolKey(row)));
}

export function groupUpcomingResets<T extends { reset_at_ms: number | null; observed_at_ms: number }>(
  rows: readonly T[],
  now: number,
) {
  const tomorrow = new Date(now);
  tomorrow.setHours(24, 0, 0, 0);
  const midnight = tomorrow.getTime();
  const upcoming = rows
    .flatMap((row) => {
      const resetAt = row.reset_at_ms;
      return resetAt !== null && resetAt > now && !quotaIsStale(row.observed_at_ms, now)
        ? [{ ...row, reset_at_ms: resetAt }]
        : [];
    })
    .sort((a, b) => a.reset_at_ms - b.reset_at_ms);

  return [
    { label: '今天', rows: upcoming.filter((row) => row.reset_at_ms < midnight) },
    { label: '之后', rows: upcoming.filter((row) => row.reset_at_ms >= midnight) },
  ].filter((group) => group.rows.length > 0);
}
