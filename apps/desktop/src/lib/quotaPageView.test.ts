import { expect, it } from 'vitest';
import { filterActiveCodexQuotas, groupUpcomingResets } from './quotaPageView';

it('keeps only the newest batch per Codex source and pool, regardless of window duration', () => {
  const row = (window_duration_minutes: number, observed_at_ms: number, source_id = 'local_codex') => ({
    source_id, pool_id: 'codex', window_duration_minutes, observed_at_ms,
    remaining_basis_points: 9800, reset_at_ms: null,
  });
  const oldMonthly = row(43_200, 1_000);
  const fiveHours = row(300, 2_000);
  const weekly = row(10_080, 2_000);
  const activeMonthly = row(43_200, 1_500, 'another_source');
  expect(filterActiveCodexQuotas([fiveHours, oldMonthly, weekly, activeMonthly])).toEqual([fiveHours, weekly, activeMonthly]);
  expect(filterActiveCodexQuotas([fiveHours, row(43_200, 3_000)])).toEqual([row(43_200, 3_000)]);
});

it('orders fresh future resets and groups them by local midnight', () => {
  const now = new Date(2026, 10, 12, 23, 50).getTime();
  const midnight = new Date(2026, 10, 13).getTime();
  const quota = (name: string, reset_at_ms: number | null, observed_at_ms = now) => ({
    name, reset_at_ms, observed_at_ms,
  });

  const groups = groupUpcomingResets([
    quota('after midnight', midnight + 1),
    quota('late today', midnight - 1),
    quota('unknown', null),
    quota('at midnight', midnight),
    quota('stale', now + 1, now - 16 * 60_000),
    quota('elapsed', now),
    quota('early today', now + 1),
  ], now);

  expect(groups.map(({ label, rows }) => [label, rows.map(({ name }) => name)])).toEqual([
    ['今天', ['early today', 'late today']],
    ['之后', ['at midnight', 'after midnight']],
  ]);
});
