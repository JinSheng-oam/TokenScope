import { describe, expect, it } from 'vitest';
import {
  formatCost,
  formatNumber,
  formatPercent,
  formatToolLabel,
  percentOf,
  relativeTime,
} from './format';

describe('dashboard formatters', () => {
  it('formats integer token counts without decimal noise', () => {
    expect(formatNumber(1234567)).toBe('1,234,567');
  });
  it('preserves missing cost as unknown instead of zero', () => {
    expect(formatCost(null)).toBe('暂无价格数据');
    expect(formatCost(0)).toContain('$0');
  });
  it('keeps unknown quota separate from zero percent', () => {
    expect(formatPercent(null)).toBe('未知');
    expect(formatPercent(0)).toBe('0.0%');
  });
  it('bounds breakdown bar percentages', () => {
    expect(percentOf(25, 100)).toBe(25);
    expect(percentOf(200, 100)).toBe(100);
    expect(percentOf(10, 0)).toBe(0);
  });
  it('formats collection recency', () => {
    expect(relativeTime(null)).toBe('从未采集');
    expect(relativeTime(9_940, 10_000)).toBe('刚刚');
    expect(relativeTime(0, 3_600_000)).toBe('1 小时前');
  });
  it('uses one product label for each supported tool', () => {
    expect(formatToolLabel('codex')).toBe('Codex');
    expect(formatToolLabel('deepseek-harness')).toBe('DeepSeek Harness');
    expect(formatToolLabel('future-tool')).toBe('future-tool');
  });
});
