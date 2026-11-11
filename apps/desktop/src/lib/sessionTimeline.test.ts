import { describe, expect, it } from 'vitest';
import { groupSessionTimeline, sessionCacheRate } from './sessionTimeline';
import type { SessionUsage } from './types';

function session(id: string, date: string | null, observed?: string): SessionUsage {
  return {
    session_id: id, tool_id: 'codex', total_tokens: 20,
    usage_record_count: 1, exact_records: 1, aggregate_records: 0, estimated_records: 0,
    unknown_precision_records: 0, untimed_records: 0, model_count: 0,
    known_response_count: null, unknown_response_count: null,
    last_event_at_ms: date ? Date.parse(date) : null,
    activity_observed_at_ms: observed ? Date.parse(observed) : null,
    display_name: null, project_ids: [], models: [],
  };
}

describe('session timeline', () => {
  it('groups by the dashboard time zone across midnight and keeps unknown events separate', () => {
    const rows = [
      session('1', '2026-11-11T16:05:00Z'),
      session('2', null, '2026-11-11T16:00:00Z'),
      session('3', '2026-11-11T15:59:00Z'),
      session('4', null),
    ];
    const groups = groupSessionTimeline(rows, true, 'Asia/Shanghai', Date.parse('2026-11-11T17:00:00Z'));
    expect(groups.map(({ label, sessions }) => [label, sessions.map((row) => row.session_id)])).toEqual([
      ['今天 · 11月12日', ['1', '2']],
      ['昨天 · 11月11日', ['3']],
      ['时间未知', ['4']],
    ]);
  });

  it('keeps non-time sorting in page order and distinguishes missing input from a zero cache hit rate', () => {
    const rows = [session('1', null), session('2', '2026-11-11T16:05:00Z')];
    expect(groupSessionTimeline(rows, false, 'UTC', Date.now())).toEqual([{ key: 'ranked', label: '', sessions: rows }]);
    expect(sessionCacheRate(rows[0])).toBe('—');
    expect(sessionCacheRate({ ...rows[1], input_uncached_tokens: 40, cache_read_tokens: 0 })).toBe('0.0%');
    expect(sessionCacheRate({ ...rows[1], input_uncached_tokens: 20, cache_read_tokens: 75, cache_write_tokens: 5 })).toBe('75.0%');
  });
});
