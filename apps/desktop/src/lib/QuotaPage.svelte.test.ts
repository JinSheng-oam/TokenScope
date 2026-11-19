// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import QuotaPage from './QuotaPage.svelte';
import type { CodexQuota, DashboardSnapshot } from './types';

describe('QuotaPage.svelte', () => {
  it('distinguishes 0% exhausted quota from null unknown quota and triggers refresh', async () => {
    const now = Date.now();
    const onRefreshCodexQuota = vi.fn();

    const codexQuotas: CodexQuota[] = [
      {
        source_id: 'codex-1',
        pool_id: 'default',
        window_duration_minutes: 300,
        remaining_basis_points: 0,
        reset_at_ms: now + 1_800_000,
        observed_at_ms: now - 30_000,
      },
    ];

    const snapshot = {
      antigravity_quota: [
        {
          source_id: 'ag-1',
          pool_id: 'claude_gpt',
          window_id: 'five_hour',
          remaining_basis_points: null,
          reset_at_ms: null,
          observed_at_ms: now - 30_000,
        },
      ],
    } as unknown as DashboardSnapshot;

    const { getByText, getByRole } = render(QuotaPage, {
      props: {
        snapshot,
        codexQuotas,
        onRefreshCodexQuota,
      },
    });

    expect(getByText('0.0%')).toBeTruthy();
    expect(getByText('已耗尽')).toBeTruthy();
    expect(getByText('未知')).toBeTruthy();

    await fireEvent.click(getByRole('button', { name: '刷新额度' }));
    expect(onRefreshCodexQuota).toHaveBeenCalledTimes(1);
  });
});
