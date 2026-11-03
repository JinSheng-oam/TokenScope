import { describe, expect, it } from 'vitest';
import { nextChartIndex } from './chartInteraction';

describe('chart keyboard navigation', () => {
  it('moves within chart bounds and supports home and end', () => {
    expect(nextChartIndex(null, 'next', 4)).toBe(1);
    expect(nextChartIndex(0, 'prev', 4)).toBe(0);
    expect(nextChartIndex(2, 'next', 4)).toBe(3);
    expect(nextChartIndex(3, 'next', 4)).toBe(3);
    expect(nextChartIndex(1, 'home', 4)).toBe(0);
    expect(nextChartIndex(1, 'end', 4)).toBe(3);
  });

  it('returns no point for an empty chart', () => {
    expect(nextChartIndex(null, 'next', 0)).toBeNull();
  });
});
