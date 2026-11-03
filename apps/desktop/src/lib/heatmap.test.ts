import { describe, expect, it } from 'vitest';
import { buildHeatmapCalendar } from './heatmap';

function day(date: string, total_tokens: number) {
  return { date, total_tokens, input_tokens: 10, output_tokens: 20 };
}

describe('buildHeatmapCalendar', () => {
  it('shows every retained week as a complete seven-row calendar', () => {
    const calendar = buildHeatmapCalendar(
      [day('2026-03-30', 100), day('2026-11-04', 300)],
      '2026-11-04'
    );

    expect(calendar.weeks).toHaveLength(32);
    expect(calendar.weeks.every((week) => week.length === 7)).toBe(true);
    expect(calendar.rangeStartDate).toBe('2026-03-30');
    expect(calendar.rangeEndDate).toBe('2026-11-04');
    expect(calendar.activeDays).toBe(2);
    expect(calendar.peakDate).toBe('2026-11-04');
    expect(calendar.peakTokens).toBe(300);
  });

  it('avoids cramped month labels at both edges', () => {
    const calendar = buildHeatmapCalendar([day('2026-03-30', 100)], '2026-11-04');

    expect(calendar.monthHeaders[0]?.label).toBe('4月');
    expect(calendar.monthHeaders.at(-1)).toMatchObject({ label: '11月', align: 'end' });
  });

  it('uses a compact empty-state calendar when no dated usage exists', () => {
    const calendar = buildHeatmapCalendar([], '2026-11-04');

    expect(calendar.weeks).toHaveLength(12);
    expect(calendar.activeDays).toBe(0);
  });
});
