import { describe, expect, it } from 'vitest';
import {
  formatCountdown,
  formatQuotaPoolName,
  quotaWindowLabel,
  resolveScopedBreakdown,
  resolveScopedTotals,
} from './dashboardView';
import type { DashboardSnapshot } from './types';

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

  it('aggregates local + remote totals when deviceScope is all, and filters when scoped', () => {
    const mockSnapshot = {
      local: {
        total_records: 10,
        untimed_records: 0,
        input_tokens: 100,
        cache_read_tokens: 50,
        cache_write_tokens: 0,
        output_tokens: 50,
        reasoning_output_tokens: 10,
        total_tokens: 200,
        estimated_cost_usd: 0.1,
        priced_records: 10,
        exact_records: 8,
        aggregate_records: 2,
        estimated_records: 0,
        unknown_records: 0,
        unattributed_tokens: 0,
      },
      remote: {
        device_count: 1,
        total_records: 5,
        input_tokens: 300,
        cache_read_tokens: 100,
        cache_write_tokens: 0,
        output_tokens: 100,
        total_tokens: 500,
        exact_records: 5,
        aggregate_records: 0,
        estimated_records: 0,
        unknown_records: 0,
      },
      remote_by_device: [
        { owner_device_id: 'remote-dev-1', total_records: 5, total_tokens: 500 },
      ],
      by_tool: [
        {
          key: 'codex',
          label: 'Codex',
          total_records: 10,
          untimed_records: 0,
          input_tokens: 100,
          cache_read_tokens: 50,
          cache_write_tokens: 0,
          output_tokens: 50,
          reasoning_output_tokens: 10,
          total_tokens: 200,
          estimated_cost_usd: 0,
          priced_records: 0,
          exact_records: 8,
          aggregate_records: 2,
          estimated_records: 0,
          unknown_records: 0,
        },
      ],
      remote_by_tool: [
        { owner_device_id: 'remote-dev-1', key: 'codex', total_records: 5, total_tokens: 500 },
      ],
    } as unknown as DashboardSnapshot;

    const allTotals = resolveScopedTotals(mockSnapshot, 'all');
    expect(allTotals?.total_tokens).toBe(700);
    expect(allTotals?.exact_records).toBe(13);

    const localTotals = resolveScopedTotals(mockSnapshot, 'local');
    expect(localTotals?.total_tokens).toBe(200);

    const remoteTotals = resolveScopedTotals(mockSnapshot, 'remote-dev-1');
    expect(remoteTotals?.total_tokens).toBe(500);

    const allTools = resolveScopedBreakdown(mockSnapshot.by_tool, mockSnapshot.remote_by_tool, 'all');
    expect(allTools[0].total_tokens).toBe(700);
  });
});
