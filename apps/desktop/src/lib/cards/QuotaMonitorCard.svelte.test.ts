// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import QuotaMonitorCard from './QuotaMonitorCard.svelte';
import type { CodexQuota, DashboardSnapshot } from '../types';

describe('QuotaMonitorCard.svelte', () => {
  it('renders merged Codex & Antigravity quotas with status pills and action buttons', async () => {
    const now = Date.now();
    const onRefreshCodexQuota = vi.fn();
    const onNavigateQuota = vi.fn();

    const activeCodexQuotas: CodexQuota[] = [
      {
        source_id: 'codex-1',
        pool_id: 'default',
        window_duration_minutes: 300,
        remaining_basis_points: 1500, // 15.0% -> warning
        reset_at_ms: now + 3_600_000,
        observed_at_ms: now - 60_000,
      },
    ];

    const activeAntigravityQuotas: DashboardSnapshot['antigravity_quota'] = [
      {
        source_id: 'ag-1',
        pool_id: 'claude_gpt',
        window_id: 'five_hour',
        remaining_basis_points: 8800, // 88.0% -> healthy
        reset_at_ms: now + 7_200_000,
        observed_at_ms: now - 60_000,
      },
    ];

    const { getByText, getByRole } = render(QuotaMonitorCard, {
      props: {
        activeCodexQuotas,
        activeAntigravityQuotas,
        onRefreshCodexQuota,
        onNavigateQuota,
      },
    });

    expect(getByText('1 即将用尽')).toBeTruthy();
    expect(getByText('15.0%')).toBeTruthy();
    expect(getByText('88.0%')).toBeTruthy();
    expect(getByText('Claude / GPT')).toBeTruthy();

    await fireEvent.click(getByRole('button', { name: '刷新本机 Codex 额度' }));
    expect(onRefreshCodexQuota).toHaveBeenCalledTimes(1);

    await fireEvent.click(getByRole('button', { name: '查看明细 →' }));
    expect(onNavigateQuota).toHaveBeenCalledTimes(1);
  });
});
