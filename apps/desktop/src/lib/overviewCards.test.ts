import { describe, it, expect } from 'vitest';
import {
  buildUnifiedQuotaItems,
  buildActivitySummary,
  buildDeviceComparison,
} from './overviewCards';
import type { CodexQuota, DashboardSnapshot, RemoteDevice } from './types';

describe('overviewCards', () => {
  it('merges codex and antigravity quotas, preserves null vs 0, and sorts urgent items first', () => {
    const now = 1_700_000_000_000;
    const codex: CodexQuota[] = [
      {
        source_id: 'codex-1',
        pool_id: 'default',
        window_duration_minutes: 300,
        remaining_basis_points: 1200, // 12% -> warning
        reset_at_ms: now + 3_600_000,
        observed_at_ms: now - 60_000,
      },
      {
        source_id: 'codex-1',
        pool_id: 'default',
        window_duration_minutes: 10080,
        remaining_basis_points: 8500, // 85% -> healthy
        reset_at_ms: now + 86_400_000,
        observed_at_ms: now - 60_000,
      },
    ];
    const ag: DashboardSnapshot['antigravity_quota'] = [
      {
        source_id: 'ag-1',
        pool_id: 'claude',
        window_id: 'five_hour',
        remaining_basis_points: 0, // 0% -> exhausted
        reset_at_ms: now + 1_800_000,
        observed_at_ms: now - 30_000,
      },
      {
        source_id: 'ag-1',
        pool_id: 'gemini_pro',
        window_id: 'weekly',
        remaining_basis_points: null, // unknown, NOT 0
        reset_at_ms: null,
        observed_at_ms: now - 30_000,
      },
    ];

    const items = buildUnifiedQuotaItems(codex, ag, now);
    expect(items.map((i) => i.status)).toEqual(['exhausted', 'warning', 'healthy', 'unknown']);
    expect(items[0].remainingBasisPoints).toBe(0);
    expect(items[3].remainingBasisPoints).toBeNull();
    expect(items[0].provider).toBe('antigravity');
    expect(items[1].provider).toBe('codex');
  });

  it('computes activity summary with peak day, average, and recent bars', () => {
    const daily = [
      { date: '2026-11-10', total_tokens: 1000, input_tokens: 700, output_tokens: 300 },
      { date: '2026-11-11', total_tokens: 0, input_tokens: 0, output_tokens: 0 },
      { date: '2026-11-12', total_tokens: 5000, input_tokens: 3500, output_tokens: 1500 },
      { date: '2026-11-13', total_tokens: 3000, input_tokens: 2000, output_tokens: 1000 },
    ];

    const summary = buildActivitySummary(daily, null, 3, 9000);
    expect(summary.activeDays).toBe(3);
    expect(summary.dailyAverageTokens).toBe(3000);
    expect(summary.peakDate).toBe('2026-11-12');
    expect(summary.peakTokens).toBe(5000);
    expect(summary.currentStreakDays).toBe(2);
    expect(summary.avgTokensPerSession).toBe(3000);
    expect(summary.recentBars.length).toBe(4);
    expect(summary.recentBars[2].heightPct).toBe(100);
  });

  it('builds sorted device comparison including local and remote devices', () => {
    const snapshot = {
      identity: { device_id: 'dev-local', display_name: 'My Laptop', time_zone: 'Asia/Shanghai' },
      local: { total_tokens: 4000, record_count: 10, estimated_cost_microusd: 1_000_000 },
      remote_by_device: [
        {
          owner_device_id: 'dev-remote-1',
          total_tokens: 6000,
          record_count: 15,
          estimated_cost_microusd: 2_000_000,
        },
      ],
    } as unknown as DashboardSnapshot;

    const remoteDevices = [
      { device_id: 'dev-remote-1', display_name: 'Office Workstation' },
    ] as RemoteDevice[];

    const rows = buildDeviceComparison(snapshot, remoteDevices);
    expect(rows.length).toBe(2);
    expect(rows[0].name).toBe('Office Workstation');
    expect(rows[0].isLocal).toBe(false);
    expect(rows[0].sharePct).toBeCloseTo(60);
    expect(rows[1].name).toBe('My Laptop');
    expect(rows[1].isLocal).toBe(true);
    expect(rows[1].sharePct).toBeCloseTo(40);
  });
});
