import { afterEach, describe, expect, it, vi } from 'vitest';
import { createSessionController, type SessionControllerState } from './sessionController';
import type { DashboardRange, SessionAnalysisPage } from './types';

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
  filters: { search: string; toolId: string; modelId: string; projectId: string },
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
      { search: 'current', toolId: '', modelId: '', projectId: '' },
      undefined,
      undefined,
    );
    expect(getState(sessions).analysis?.page.total).toBe(7);
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
