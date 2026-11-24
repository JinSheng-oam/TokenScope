// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import { tick } from 'svelte';
import DashboardTopbar from './DashboardTopbar.svelte';
import type { DashboardSnapshot } from './types';

describe('DashboardTopbar.svelte', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  const baseProps = {
    activeTabLabel: '使用总览',
    showControls: true,
    deviceScope: 'all' as const,
    deviceScopeLabel: '全部设备',
    selectableRemoteDevices: [],
    snapshot: {
      generated_at_ms: Date.now() - 120_000,
      local: { total_tokens: 1000 },
      remote: { total_tokens: 0 },
      remote_by_device: [],
    } as unknown as DashboardSnapshot,
    range: 'all' as const,
    customStart: '',
    customEnd: '',
    todayIso: '2026-11-24',
    refreshing: false,
    selectDeviceScope: vi.fn(),
    handleSelectRange: vi.fn(),
    applyCustomRange: vi.fn(),
    loadDashboard: vi.fn().mockResolvedValue(undefined),
  };

  it('shows transient done feedback after manual refresh and resets cleanly', async () => {
    vi.useFakeTimers();
    const loadDashboard = vi.fn().mockResolvedValue(undefined);

    const { getByRole, getByText } = render(DashboardTopbar, {
      props: {
        ...baseProps,
        loadDashboard,
      },
    });

    const btn = getByRole('button', { name: '刷新统计' });
    expect(btn.getAttribute('title')).toContain('上次更新：');
    expect(getByText('刷新')).toBeTruthy();

    await fireEvent.click(btn);
    expect(loadDashboard).toHaveBeenCalledTimes(1);

    await tick();
    expect(getByText('已更新')).toBeTruthy();
    expect(btn.classList.contains('is-done')).toBe(true);

    vi.advanceTimersByTime(1600);
    await tick();
    expect(getByText('刷新')).toBeTruthy();
    expect(btn.classList.contains('is-done')).toBe(false);
  });

  it('displays busy state and disables click while refreshing', async () => {
    const loadDashboard = vi.fn().mockResolvedValue(undefined);
    const { getByRole, getByText } = render(DashboardTopbar, {
      props: {
        ...baseProps,
        refreshing: true,
        loadDashboard,
      },
    });

    const btn = getByRole('button', { name: '正在刷新数据' }) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    expect(btn.getAttribute('aria-busy')).toBe('true');
    expect(getByText('刷新中')).toBeTruthy();

    await fireEvent.click(btn);
    expect(loadDashboard).not.toHaveBeenCalled();
  });
});
