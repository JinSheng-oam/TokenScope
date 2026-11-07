import { getAgentStatus, getDashboardSnapshot, startAgent } from './api';
import { createRequestGate, dashboardRequestKey } from './dashboardRequests';
import type { AgentStatus, DashboardRange, DashboardSnapshot } from './types';

interface DashboardLoaderOptions {
  runtimeAvailable: () => boolean;
  selection: () => { range: DashboardRange; start: string; end: string };
  agentChanged: (status: AgentStatus) => void;
  loadingChanged: (loading: boolean) => void;
  refreshingChanged: (refreshing: boolean) => void;
  errorChanged: (message: string) => void;
  snapshotCleared: () => void;
  snapshotChanged: (snapshot: DashboardSnapshot) => void;
  snapshotLoaded: (snapshot: DashboardSnapshot) => void;
}

export function createDashboardLoader(options: DashboardLoaderOptions) {
  const dashboardRequests = createRequestGate();
  let agentStarted = false;
  let appliedSnapshotKey = '';
  let appliedAt = 0;
  let refreshTimer: number | undefined;
  let appliedRefreshIntervalMs = -1;

  function acceptSnapshot(next: DashboardSnapshot, key: string) {
    const { range, start, end } = options.selection();
    if (key !== dashboardRequestKey(range, start, end)) return;
    if (appliedSnapshotKey === key && appliedAt > next.generated_at_ms) return;
    appliedSnapshotKey = key;
    appliedAt = next.generated_at_ms;
    options.snapshotChanged(next);
  }

  function clearSnapshot() {
    appliedAt = 0;
    options.snapshotCleared();
  }

  function fetchSnapshot(selectedRange: DashboardRange, start: string, end: string) {
    return getDashboardSnapshot(
      selectedRange,
      0,
      selectedRange === 'custom' ? start : undefined,
      selectedRange === 'custom' ? end : undefined
    );
  }

  async function loadDashboard(showSpinner = true) {
    const request = dashboardRequests.begin();
    const { range: selectedRange, start, end } = options.selection();
    const key = dashboardRequestKey(selectedRange, start, end);
    if (!options.runtimeAvailable()) {
      options.errorChanged('请在 TokenScope 桌面端运行环境查看实时统计。');
      options.loadingChanged(false);
      return;
    }
    if (showSpinner) options.refreshingChanged(true);
    options.errorChanged('');
    try {
      const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
      let currentAgent: AgentStatus;
      if (!agentStarted) {
        currentAgent = await startAgent(timeZone);
        agentStarted = true;
      } else {
        currentAgent = await getAgentStatus();
        if (!currentAgent.running && currentAgent.binaryFound) {
          currentAgent = await startAgent(timeZone);
        }
      }
      if (!dashboardRequests.isCurrent(request)) return;
      options.agentChanged(currentAgent);
      if (!currentAgent.running) {
        options.errorChanged(currentAgent.message);
        clearSnapshot();
      } else if (!currentAgent.databaseReady) {
        options.errorChanged('Agent 已启动，正在初始化本地 SQLite WAL 数据库…');
        clearSnapshot();
      } else {
        const nextSnapshot = await fetchSnapshot(selectedRange, start, end);
        if (!dashboardRequests.isCurrent(request)) return;
        acceptSnapshot(nextSnapshot, key);
        options.snapshotLoaded(nextSnapshot);
      }
    } catch (error) {
      if (dashboardRequests.isCurrent(request)) options.errorChanged(error instanceof Error ? error.message : String(error));
    } finally {
      if (dashboardRequests.isCurrent(request)) { options.loadingChanged(false); options.refreshingChanged(false); }
    }
  }

  function applyRefreshInterval(intervalMs: number) {
    if (intervalMs === appliedRefreshIntervalMs) return;
    appliedRefreshIntervalMs = intervalMs;
    if (refreshTimer !== undefined) {
      window.clearInterval(refreshTimer);
      refreshTimer = undefined;
    }
    if (intervalMs > 0) {
      refreshTimer = window.setInterval(() => { if (document.visibilityState === 'visible') void loadDashboard(false); }, intervalMs);
    }
  }

  function dispose() {
    dashboardRequests.invalidate();
    if (refreshTimer !== undefined) window.clearInterval(refreshTimer);
    refreshTimer = undefined;
  }

  return { load: loadDashboard, setRefreshInterval: applyRefreshInterval, acceptSnapshot, dispose };
}
