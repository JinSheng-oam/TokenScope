// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import QuotaPage from './QuotaPage.svelte';
import type { AntigravityQuota, CodexQuota } from './types';

describe('QuotaPage.svelte', () => {
  it('distinguishes 0% exhausted quota from null unknown quota and triggers refresh', async () => {
    const now = Date.now();
    const onRefreshQuota = vi.fn();

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

    const antigravityQuotas: AntigravityQuota[] = [
      {
        source_id: 'ag-1',
        pool_id: 'claude_gpt',
        window_id: 'five_hour',
        remaining_basis_points: null,
        reset_at_ms: null,
        observed_at_ms: now - 30_000,
      },
    ];

    const { getByText, getByRole } = render(QuotaPage, {
      props: {
        codexQuotas,
        antigravityQuotas,
        onRefreshQuota,
      },
    });

    expect(getByText('0.0%')).toBeTruthy();
    expect(getByText('已耗尽')).toBeTruthy();
    expect(getByText('未知')).toBeTruthy();

    await fireEvent.click(getByRole('button', { name: '刷新额度' }));
    expect(onRefreshQuota).toHaveBeenCalledTimes(1);
  });

  it('shows independent provider progress and errors', () => {
    const { getByRole, getByText } = render(QuotaPage, {
      props: {
        codexQuotas: [],
        antigravityQuotas: [],
        refreshing: true,
        codexRefreshing: true,
        antigravityRefreshing: true,
        codexQuotaError: 'Codex 查询失败',
        antigravityQuotaError: 'Antigravity 查询超时',
      },
    });

    expect(getByText('正在刷新')).toBeTruthy();
    expect(getByText('Codex 查询失败')).toBeTruthy();
    expect(getByText('Antigravity 查询超时')).toBeTruthy();
    expect(getByRole('button', { name: '正在刷新' })).toHaveProperty('disabled', true);
  });
});
