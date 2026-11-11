import type { SessionUsage } from './types';

function dateFormatter(timeZone: string) {
  const options = { year: 'numeric', month: '2-digit', day: '2-digit' } as const;
  try {
    return new Intl.DateTimeFormat('sv-SE', { ...options, timeZone });
  } catch {
    return new Intl.DateTimeFormat('sv-SE', { ...options, timeZone: 'UTC' });
  }
}

export function sessionEventTime(session: SessionUsage): number | null {
  return session.last_event_at_ms ?? session.activity_observed_at_ms;
}

export function groupSessionTimeline(sessions: SessionUsage[], byDay: boolean, timeZone: string, now: number) {
  if (!byDay) return [{ key: 'ranked', label: '', sessions }];

  const dates = dateFormatter(timeZone);
  const today = dates.format(now);
  // Subtract one calendar day from the local date, rather than 24 hours from now.
  const yesterday = new Date(Date.parse(today) - 86_400_000).toISOString().slice(0, 10);
  const days = new Map<string, SessionUsage[]>();
  for (const session of sessions) {
    const time = sessionEventTime(session);
    const key = time === null ? '' : dates.format(time);
    const day = days.get(key);
    if (day) day.push(session);
    else days.set(key, [session]);
  }

  return Array.from(days, ([key, sessions]) => {
    if (!key) return { key, label: '时间未知', sessions };
    const [year, month, day] = key.split('-').map(Number);
    const date = (year === Number(today.slice(0, 4)) ? '' : year + '年') + month + '月' + day + '日';
    const prefix = key === today ? '今天 · ' : key === yesterday ? '昨天 · ' : '';
    return { key, label: prefix + date, sessions };
  });
}

export function sessionCacheRate(session: SessionUsage): string {
  const read = session.cache_read_tokens ?? 0;
  const input = (session.input_uncached_tokens ?? 0) + read + (session.cache_write_tokens ?? 0);
  return input > 0 ? ((read / input) * 100).toFixed(1) + '%' : '—';
}

export function sessionProjectLabels(session: SessionUsage, getLabel: (id: string) => string): string[] {
  return [...new Set(session.project_ids.map(getLabel).filter(
    (label) => label.trim() && !label.startsWith('prj_') && label !== '项目名称未提供',
  ))];
}
