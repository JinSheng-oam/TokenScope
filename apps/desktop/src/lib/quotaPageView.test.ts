import { expect, it } from 'vitest';
import { groupUpcomingResets } from './quotaPageView';

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
