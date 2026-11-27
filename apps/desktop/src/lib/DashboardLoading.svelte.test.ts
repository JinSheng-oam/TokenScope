// @vitest-environment happy-dom
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import { tick } from 'svelte';
import DashboardLoading from './DashboardLoading.svelte';
import {
  resetDashboardLayout,
  saveDashboardCardGap,
  saveDashboardCardRows,
  saveDashboardLayout,
} from './dashboardLayout';

describe('DashboardLoading.svelte', () => {
  const store = new Map<string, string>();

  beforeEach(() => {
    store.clear();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => store.set(key, value),
      removeItem: (key: string) => store.delete(key),
      clear: () => store.clear(),
    });
    resetDashboardLayout();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders default visible overview cards in configured order and widths', () => {
    const { container } = render(DashboardLoading, { props: { activeTab: 'overview' } });

    const wrappers = Array.from(container.querySelectorAll<HTMLElement>('.overview-card-wrapper'));
    expect(wrappers.map((el) => el.dataset.cardId)).toEqual([
      'metrics',
      'trend',
      'models',
      'sessions',
      'sources',
    ]);
    expect(wrappers.map((el) => el.dataset.cardWidth)).toEqual([
      'full',
      '2/3',
      '1/3',
      '2/3',
      '1/3',
    ]);
    expect(container.querySelector('[data-card-id="integrity"]')).toBeNull();
  });

  it('reacts immediately when user changes card order, visibility, width, gap, or rows', async () => {
    const { container } = render(DashboardLoading, { props: { activeTab: 'overview' } });

    saveDashboardCardGap(22);
    saveDashboardCardRows(3);
    saveDashboardLayout([
      { id: 'quotas', visible: true, width: '1/2', rows: 5 },
      { id: 'cost', visible: true, width: '1/2', rows: 3 },
      { id: 'metrics', visible: true, width: 'full' },
      { id: 'trend', visible: false, width: '2/3' },
    ]);
    await tick();

    const grid = container.querySelector<HTMLElement>('.loading-overview-grid');
    expect(grid?.style.getPropertyValue('--overview-card-gap')).toBe('22px');

    const wrappers = Array.from(container.querySelectorAll<HTMLElement>('.overview-card-wrapper'));
    expect(wrappers.map((el) => el.dataset.cardId)).toEqual(['quotas', 'cost', 'metrics']);
    expect(wrappers[0].classList.contains('col-span-6')).toBe(true);
    expect(wrappers[1].classList.contains('col-span-6')).toBe(true);
    expect(wrappers[2].classList.contains('col-span-12')).toBe(true);
    expect(wrappers[0].querySelectorAll('.sk-session-row').length).toBe(5);
    expect(wrappers[1].querySelectorAll('.sk-rank-item').length).toBe(3);
  });

  it('switches skeleton layout structure when activeTab is not overview', () => {
    const { container } = render(DashboardLoading, { props: { activeTab: 'quota' } });
    expect(container.querySelector('.loading-quota-grid')).not.toBeNull();
    expect(container.querySelector('.loading-overview-grid')).toBeNull();
  });
});
