import { describe, expect, it } from 'vitest';
import { formatCountdown, formatQuotaPoolName, quotaWindowLabel } from './dashboardView';

describe('dashboardView quota utilities', () => {
  it('formats pool names into readable display strings', () => {
    expect(formatQuotaPoolName('claude_gpt')).toBe('Claude / GPT');
    expect(formatQuotaPoolName('gemini_4_argon')).toBe('Gemini 4 Argon');
    expect(formatQuotaPoolName('gemini')).toBe('Gemini');
    expect(formatQuotaPoolName('codex')).toBe('Codex');
  });

  it('formats reset countdown with days when >= 24h', () => {
    const baseNow = 1_000_000_000;
    // 166 hours, 1 minute, 55 seconds = (166 * 3600 + 1 * 60 + 55) * 1000 ms
    const diffMs = (166 * 3600 + 1 * 60 + 55) * 1000;
    expect(formatCountdown(baseNow + diffMs, baseNow)).toBe('6天 22小时 后重置');
  });

  it('formats reset countdown with standard timer when < 24h', () => {
    const baseNow = 1_000_000_000;
    // 3 hours, 1 minute, 55 seconds
    const diffMs = (3 * 3600 + 1 * 60 + 55) * 1000;
    expect(formatCountdown(baseNow + diffMs, baseNow)).toBe('03:01:55 后重置');
  });

  it('handles expired and null countdowns gracefully', () => {
    expect(formatCountdown(null)).toBe('未提供重置时间');
    expect(formatCountdown(100, 200)).toBe('即将重置');
  });

  it('labels standard quota window durations', () => {
    expect(quotaWindowLabel(300)).toBe('5 小时周期');
    expect(quotaWindowLabel(10_080)).toBe('每周周期');
  });
});
