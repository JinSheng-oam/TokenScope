import { afterEach, describe, expect, it, vi } from 'vitest';
import { createSessionController, type SessionControllerState } from './sessionController';
import type { DashboardRange, SessionAnalysisPage, SessionUsage } from './types';

function analysis(total: number, offset = 0): SessionAnalysisPage {
  return {
    page: { sessions: [], total, offset, limit: 50 },
    filter_options: { tools: [], models: [], projects: [] },
  };
}

function controller(loadPage: MockSessionLoader) {
  return createSessionController({
    isRuntimeAvailable: () => true,
    isActive: () => true,
    range: () => '7d',
    customStart: () => '',
    customEnd: () => '',
    refreshDashboard: async () => {},
    loadPage,
  });
}

type MockSessionLoader = (
  range: DashboardRange,
  offset: number,
  filters: { search: string; toolId: string; modelId: string; projectId: string; pageSize?: number },
  startDate?: string,
  endDate?: string,
) => Promise<SessionAnalysisPage>;

function getState(controller: ReturnType<typeof createSessionController>) {
  let result = undefined as SessionControllerState | undefined;
  controller.state.subscribe((value) => { result = value; })();
  return result!;
}

describe('session controller', () => {
  afterEach(() => vi.useRealTimers());

  it('deduplicates identical reads and reloads when dashboard data changes', async () => {
    let resolveFirst!: (value: SessionAnalysisPage) => void;
    const loadPage = vi.fn<MockSessionLoader>()
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValue(analysis(8));
    const sessions = controller(loadPage);

    const first = sessions.load(false, 4);
    const duplicate = sessions.load(false, 4);
    expect(loadPage).toHaveBeenCalledTimes(1);
    resolveFirst(analysis(7));
    await Promise.all([first, duplicate]);

    await sessions.load(false, 4);
    expect(loadPage).toHaveBeenCalledTimes(1);
    await sessions.load(false, 5);
    expect(loadPage).toHaveBeenCalledTimes(2);
    expect(getState(sessions).analysis?.page.total).toBe(8);
  });

  it('does not let a stale response replace a newer filter result', async () => {
    let resolveFirst!: (value: SessionAnalysisPage) => void;
    const loadPage = vi.fn<MockSessionLoader>()
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce(analysis(7));
    const sessions = controller(loadPage);

    const first = sessions.load();
    sessions.state.update((value) => ({ ...value, search: 'current' }));
    await sessions.load();
    resolveFirst(analysis(99));
    await first;

    expect(loadPage).toHaveBeenLastCalledWith(
      '7d',
      0,
      { search: 'current', toolId: '', modelId: '', projectId: '', pageSize: 100 },
      undefined,
      undefined,
    );
    expect(getState(sessions).analysis?.page.total).toBe(7);
  });

  it('reads two pages in one request and reuses both until the data revision changes', async () => {
    const loadPage = vi.fn<MockSessionLoader>().mockImplementation(async (_range, offset, filters) => {
      const result = analysis(123, offset);
      result.page.limit = filters.pageSize!;
      result.page.sessions = Array.from({ length: Math.min(result.page.limit, 123 - offset) }, (_, index) => ({
        session_id: String(offset + index),
      }) as SessionUsage);
      return result;
    });
    const sessions = controller(loadPage);

    await sessions.load(false, 1);
    expect(getState(sessions).analysis?.page).toMatchObject({ offset: 0, limit: 50, total: 123 });
    expect(getState(sessions).analysis?.page.sessions).toHaveLength(50);

    const next = sessions.setPage(50);
    expect(getState(sessions)).toMatchObject({ offset: 50, loading: false });
    expect(getState(sessions).analysis?.page.sessions[0].session_id).toBe('50');
    await next;
    await sessions.setPage(0);
    expect(loadPage).toHaveBeenCalledTimes(1);

    await sessions.load(false, 2);
    expect(loadPage).toHaveBeenCalledTimes(2);
    await sessions.setPage(100);
    expect(getState(sessions).analysis?.page.sessions).toHaveLength(23);
    expect(loadPage).toHaveBeenCalledTimes(3);
  });

  it('corrects an out-of-range offset using the visible page size rather than the batch size', async () => {
    const loadPage = vi.fn<MockSessionLoader>().mockImplementation(async (_range, offset) => analysis(73, offset));
    const sessions = controller(loadPage);
    await sessions.setPage(100);
    expect(loadPage.mock.calls.map((call) => call[1])).toEqual([100, 50]);
    expect(getState(sessions)).toMatchObject({ offset: 50, loading: false });
    expect(getState(sessions).analysis?.page).toMatchObject({ offset: 50, limit: 50 });
  });

  it('debounces filter changes and clears all filters', async () => {
    vi.useFakeTimers();
    const loadPage = vi.fn<MockSessionLoader>().mockResolvedValue(analysis(3));
    const sessions = controller(loadPage);
    sessions.state.update((value) => ({
      ...value,
      search: 'old',
      toolFilter: 'codex',
      modelFilter: 'gpt',
      projectFilter: 'project',
      offset: 50,
    }));

    sessions.schedule();
    sessions.clearFilters();
    await vi.advanceTimersByTimeAsync(220);

    expect(getState(sessions)).toMatchObject({
      search: '',
      toolFilter: '',
      modelFilter: '',
      projectFilter: '',
      offset: 0,
    });
    expect(loadPage).toHaveBeenCalledTimes(1);
  });
});
