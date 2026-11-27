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

  it('supports preset week ranges and custom start/end date ranges', () => {
    const data = [day('2025-01-10', 10_000_000), day('2026-10-01', 20_000_000), day('2026-11-04', 30_000_000)];
    const twelveWeeks = buildHeatmapCalendar(data, '2026-11-04', { range: '12w' });
    expect(twelveWeeks.weeks).toHaveLength(12);
    expect(twelveWeeks.activeDays).toBe(2);

    const custom = buildHeatmapCalendar(data, '2026-11-04', {
      range: 'custom',
      customStart: '2026-09-28',
      customEnd: '2026-10-18',
    });
    expect(custom.rangeStartDate).toBe('2026-09-28');
    expect(custom.rangeEndDate).toBe('2026-10-18');
    expect(custom.activeDays).toBe(1);
    expect(custom.peakDate).toBe('2026-10-01');
  });

  it('prevents a single outlier spike from flattening normal active days in auto mode', () => {
    const data = [
      day('2026-10-25', 10_000_000),
      day('2026-10-26', 40_000_000),
      day('2026-10-27', 80_000_000),
      day('2026-10-28', 120_000_000),
      day('2026-10-29', 150_000_000),
      day('2026-10-30', 1_400_000_000),
    ];
    const calendar = buildHeatmapCalendar(data, '2026-10-30', { thresholdMode: 'auto' });
    const cells = calendar.weeks.flat();
    expect(cells.find((c) => c.date === '2026-10-29')?.level).toBe(4);
    expect(cells.find((c) => c.date === '2026-10-28')?.level).toBe(4);
    expect(cells.find((c) => c.date === '2026-10-27')?.level).toBe(3);
  });

  it('applies custom M-unit thresholds when thresholdMode is custom', () => {
    const data = [
      day('2026-11-01', 1_000_000),
      day('2026-11-02', 6_000_000),
      day('2026-11-03', 25_000_000),
      day('2026-11-04', 70_000_000),
    ];
    const calendar = buildHeatmapCalendar(data, '2026-11-04', {
      thresholdMode: 'custom',
      customThresholdsM: [5, 20, 60],
    });
    const cells = calendar.weeks.flat();
    expect(cells.find((c) => c.date === '2026-11-01')?.level).toBe(1);
    expect(cells.find((c) => c.date === '2026-11-02')?.level).toBe(2);
    expect(cells.find((c) => c.date === '2026-11-03')?.level).toBe(3);
    expect(cells.find((c) => c.date === '2026-11-04')?.level).toBe(4);
  });
});
