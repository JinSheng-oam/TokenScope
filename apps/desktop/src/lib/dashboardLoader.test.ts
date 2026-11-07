import { expect, it, vi } from 'vitest';
import { getDashboardSnapshot, startAgent, getAgentStatus } from './api';
import { createDashboardLoader } from './dashboardLoader';
import { dashboardRequestKey } from './dashboardRequests';
import type { AgentStatus, DashboardRange, DashboardSnapshot } from './types';

vi.mock('./api', () => ({
  startAgent: vi.fn(),
  getAgentStatus: vi.fn(),
  getDashboardSnapshot: vi.fn(),
}));

it('keeps the latest selection and rejects older snapshots from requests and events', async () => {
  const ready = { running: true, databaseReady: true } as AgentStatus;
  vi.mocked(startAgent).mockResolvedValue(ready);
  vi.mocked(getAgentStatus).mockResolvedValue(ready);
  let range: DashboardRange = 'all';
  let finishOld!: (snapshot: DashboardSnapshot) => void;
  const oldRequest = new Promise<DashboardSnapshot>((resolve) => { finishOld = resolve; });
  const current = { range: '7d', generated_at_ms: 200 } as DashboardSnapshot;
  vi.mocked(getDashboardSnapshot).mockReturnValueOnce(oldRequest).mockResolvedValueOnce(current);
  const snapshotChanged = vi.fn();
  const snapshotLoaded = vi.fn();
  const loader = createDashboardLoader({
    runtimeAvailable: () => true,
    selection: () => ({ range, start: '', end: '' }),
    agentChanged: vi.fn(),
    loadingChanged: vi.fn(),
    refreshingChanged: vi.fn(),
    errorChanged: vi.fn(),
    snapshotCleared: vi.fn(),
    snapshotChanged,
    snapshotLoaded,
  });

  const first = loader.load();
  await vi.waitFor(() => expect(getDashboardSnapshot).toHaveBeenCalledTimes(1));
  range = '7d';
  await loader.load();
  finishOld({ range: 'all', generated_at_ms: 300 } as DashboardSnapshot);
  await first;
  loader.acceptSnapshot({ ...current, generated_at_ms: 100 }, dashboardRequestKey('7d'));
  expect(snapshotChanged.mock.calls).toEqual([[current]]);
  expect(snapshotLoaded.mock.calls).toEqual([[current]]);
  loader.dispose();
});
