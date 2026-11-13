import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
  DEFAULT_DASHBOARD_CARDS,
  STORAGE_KEY,
  loadDashboardLayout,
  saveDashboardLayout,
  resetDashboardLayout,
  reorderCards,
  updateCardWidth,
  setCardVisibility,
} from './dashboardLayout';

describe('dashboardLayout', () => {
  const store = new Map<string, string>();

  beforeEach(() => {
    store.clear();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => store.set(key, value),
      removeItem: (key: string) => store.delete(key),
      clear: () => store.clear(),
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('loads default cards when storage is empty', () => {
    const layout = loadDashboardLayout();
    expect(layout.length).toBe(DEFAULT_DASHBOARD_CARDS.length);
    expect(layout.find((c) => c.id === 'trend')?.width).toBe('2/3');
    expect(layout.find((c) => c.id === 'models')?.visible).toBe(true);
    expect(layout.find((c) => c.id === 'models')?.width).toBe('1/3');
    expect(layout.find((c) => c.id === 'sessions')?.visible).toBe(true);
    expect(layout.find((c) => c.id === 'sources')?.visible).toBe(true);
    expect(layout.find((c) => c.id === 'integrity')?.visible).toBe(false);
    expect(layout.find((c) => c.id === 'projects')?.visible).toBe(false);
    expect(layout.find((c) => c.id === 'quotas')?.visible).toBe(false);
  });

  it('loads saved layout from storage and preserves custom order and size', () => {
    const custom = [
      { id: 'models', visible: true, width: 'full' },
      { id: 'trend', visible: true, width: '1/2' },
      { id: 'tools', visible: false, width: '1/3' },
    ];
    store.set(STORAGE_KEY, JSON.stringify(custom));

    const loaded = loadDashboardLayout();
    expect(loaded[0].id).toBe('models');
    expect(loaded[0].width).toBe('full');
    expect(loaded[1].id).toBe('trend');
    expect(loaded[1].width).toBe('1/2');
    expect(loaded[2].id).toBe('tools');
    expect(loaded[2].visible).toBe(false);

    // Missing cards from catalog should be appended
    expect(loaded.find((c) => c.id === 'metrics')).toBeDefined();
    expect(loaded.find((c) => c.id === 'sessions')).toBeDefined();
    expect(loaded.find((c) => c.id === 'quotas')).toBeDefined();
  });

  it('falls back to default on corrupted storage data', () => {
    store.set(STORAGE_KEY, '{ invalid json');
    const loaded = loadDashboardLayout();
    expect(loaded.length).toBe(DEFAULT_DASHBOARD_CARDS.length);
    expect(loaded[0].id).toBe('metrics');
  });

  it('saves dashboard layout to storage', () => {
    const cards = [
      { id: 'models' as const, visible: true, width: 'full' as const },
    ];
    saveDashboardLayout(cards);
    expect(store.get(STORAGE_KEY)).toBe(JSON.stringify(cards));
  });

  it('resets layout to defaults and clears storage', () => {
    store.set(STORAGE_KEY, JSON.stringify([{ id: 'trend', visible: true, width: 'full' }]));
    const reset = resetDashboardLayout();
    expect(store.has(STORAGE_KEY)).toBe(false);
    expect(reset.length).toBe(DEFAULT_DASHBOARD_CARDS.length);
    expect(reset[0].id).toBe('metrics');
  });

  it('reorders cards correctly', () => {
    const initial = loadDashboardLayout();
    const firstId = initial[0].id;
    const secondId = initial[1].id;

    const reordered = reorderCards(initial, 0, 1);
    expect(reordered[0].id).toBe(secondId);
    expect(reordered[1].id).toBe(firstId);

    // Out of bound handles gracefully
    expect(reorderCards(initial, -1, 2)).toEqual(initial);
    expect(reorderCards(initial, 0, 99)).toEqual(initial);
    expect(reorderCards(initial, 2, 2)).toEqual(initial);
  });

  it('updates card width for resizable cards and ignores fixed cards', () => {
    const initial = loadDashboardLayout();

    // 'trend' is resizable
    const updatedTrend = updateCardWidth(initial, 'trend', 'full');
    expect(updatedTrend.find((c) => c.id === 'trend')?.width).toBe('full');

    // 'metrics' is non-resizable, width should not change
    const updatedMetrics = updateCardWidth(initial, 'metrics', '1/3');
    expect(updatedMetrics.find((c) => c.id === 'metrics')?.width).toBe('full');
  });

  it('updates card visibility', () => {
    const initial = loadDashboardLayout();
    const updated = setCardVisibility(initial, 'tools', false);
    expect(updated.find((c) => c.id === 'tools')?.visible).toBe(false);

    const reShown = setCardVisibility(updated, 'tools', true);
    expect(reShown.find((c) => c.id === 'tools')?.visible).toBe(true);
  });
});
