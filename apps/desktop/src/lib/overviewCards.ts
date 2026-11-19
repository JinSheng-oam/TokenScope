import {
  antigravityWindowLabel,
  formatQuotaPoolName,
  quotaIsStale,
  quotaWindowLabel,
} from './dashboardView';
import type { HeatmapCalendar } from './heatmap';
import type { CodexQuota, DashboardSnapshot, RemoteDevice } from './types';

export type QuotaHealthStatus = 'exhausted' | 'warning' | 'healthy' | 'unknown';

export interface UnifiedQuotaItem {
  key: string;
  provider: 'codex' | 'antigravity';
  providerLabel: string;
  name: string;
  period: string;
  remainingBasisPoints: number | null;
  status: QuotaHealthStatus;
  stale: boolean;
  resetAtMs: number | null;
  observedAtMs: number;
}

export interface ActivitySummary {
  activeDays: number;
  dailyAverageTokens: number;
  peakDate: string | null;
  peakTokens: number;
  currentStreakDays: number;
  sessionCount: number;
  avgTokensPerSession: number;
  recentBars: { date: string; totalTokens: number; heightPct: number }[];
}

export interface DeviceComparisonRow {
  deviceId: string;
  name: string;
  isLocal: boolean;
  totalTokens: number;
  recordCount: number;
  estimatedCostMicrousd: number | null;
  sharePct: number;
}

function resolveQuotaStatus(remainingBasisPoints: number | null): QuotaHealthStatus {
  if (remainingBasisPoints === null) return 'unknown';
  if (remainingBasisPoints === 0) return 'exhausted';
  if (remainingBasisPoints < 2000) return 'warning';
  return 'healthy';
}

const STATUS_PRIORITY: Record<QuotaHealthStatus, number> = {
  exhausted: 0,
  warning: 1,
  healthy: 2,
  unknown: 3,
};

export function buildUnifiedQuotaItems(
  codexQuotas: readonly CodexQuota[],
  antigravityQuotas: readonly DashboardSnapshot['antigravity_quota'][number][],
  nowMs: number,
): UnifiedQuotaItem[] {
  const codexItems: UnifiedQuotaItem[] = codexQuotas.map((q) => ({
    key: JSON.stringify(['codex', q.source_id, q.pool_id, q.window_duration_minutes]),
    provider: 'codex',
    providerLabel: 'Codex',
    name: q.pool_id && q.pool_id !== 'default' ? formatQuotaPoolName(q.pool_id) : 'Codex',
    period: quotaWindowLabel(q.window_duration_minutes).replace(/周期$/, ''),
    remainingBasisPoints: q.remaining_basis_points,
    status: resolveQuotaStatus(q.remaining_basis_points),
    stale: quotaIsStale(q.observed_at_ms, nowMs),
    resetAtMs: q.reset_at_ms,
    observedAtMs: q.observed_at_ms,
  }));

  const agItems: UnifiedQuotaItem[] = antigravityQuotas.map((q) => ({
    key: JSON.stringify(['antigravity', q.pool_id, q.window_id]),
    provider: 'antigravity',
    providerLabel: 'Antigravity',
    name: formatQuotaPoolName(q.pool_id),
    period: antigravityWindowLabel(q.window_id).replace(/周期$/, ''),
    remainingBasisPoints: q.remaining_basis_points,
    status: resolveQuotaStatus(q.remaining_basis_points),
    stale: quotaIsStale(q.observed_at_ms, nowMs),
    resetAtMs: q.reset_at_ms,
    observedAtMs: q.observed_at_ms,
  }));

  return [...codexItems, ...agItems].sort((a, b) => {
    const statusDiff = STATUS_PRIORITY[a.status] - STATUS_PRIORITY[b.status];
    if (statusDiff !== 0) return statusDiff;

    const remA = a.remainingBasisPoints ?? Number.POSITIVE_INFINITY;
    const remB = b.remainingBasisPoints ?? Number.POSITIVE_INFINITY;
    if (remA !== remB) return remA - remB;

    const resetA = a.resetAtMs ?? Number.POSITIVE_INFINITY;
    const resetB = b.resetAtMs ?? Number.POSITIVE_INFINITY;
    if (resetA !== resetB) return resetA - resetB;

    return a.name.localeCompare(b.name);
  });
}

export function buildActivitySummary(
  daily: readonly DashboardSnapshot['daily'][number][],
  heatmapCalendar: HeatmapCalendar | null | undefined,
  sessionCount: number,
  totalTokens: number,
): ActivitySummary {
  const activeEntries = daily.filter((d) => d.total_tokens > 0);
  const activeDays = activeEntries.length;
  const sumTokens = activeEntries.reduce((acc, d) => acc + d.total_tokens, 0);
  const dailyAverageTokens = activeDays > 0 ? Math.round(sumTokens / activeDays) : 0;

  let peakDate: string | null = null;
  let peakTokens = 0;
  for (const d of activeEntries) {
    if (d.total_tokens > peakTokens) {
      peakTokens = d.total_tokens;
      peakDate = d.date;
    }
  }

  // Compute current active streak from heatmap cells (or fall back to daily)
  let currentStreakDays = 0;
  if (heatmapCalendar && heatmapCalendar.weeks.length > 0) {
    const flatCells = heatmapCalendar.weeks
      .flat()
      .filter((c) => !c.isFuture)
      .sort((a, b) => a.date.localeCompare(b.date));
    // Allow today to be 0 if yesterday was active
    let idx = flatCells.length - 1;
    if (idx >= 0 && flatCells[idx].total_tokens === 0) {
      idx -= 1;
    }
    while (idx >= 0 && flatCells[idx].total_tokens > 0) {
      currentStreakDays += 1;
      idx -= 1;
    }
  } else {
    const sorted = [...daily].sort((a, b) => a.date.localeCompare(b.date));
    let idx = sorted.length - 1;
    if (idx >= 0 && sorted[idx].total_tokens === 0) idx -= 1;
    while (idx >= 0 && sorted[idx].total_tokens > 0) {
      currentStreakDays += 1;
      idx -= 1;
    }
  }

  const recentSlice = [...daily]
    .sort((a, b) => a.date.localeCompare(b.date))
    .slice(-14);
  const recentMax = Math.max(1, ...recentSlice.map((d) => d.total_tokens));
  const recentBars = recentSlice.map((d) => ({
    date: d.date,
    totalTokens: d.total_tokens,
    heightPct: d.total_tokens > 0 ? Math.max(8, Math.round((d.total_tokens / recentMax) * 100)) : 4,
  }));

  const avgTokensPerSession = sessionCount > 0 ? Math.round(totalTokens / sessionCount) : 0;

  return {
    activeDays,
    dailyAverageTokens,
    peakDate,
    peakTokens,
    currentStreakDays,
    sessionCount,
    avgTokensPerSession,
    recentBars,
  };
}

export function buildDeviceComparison(
  snapshot: DashboardSnapshot,
  remoteDevices: readonly RemoteDevice[],
): DeviceComparisonRow[] {
  const nameMap = new Map<string, string>();
  for (const d of remoteDevices) {
    nameMap.set(d.device_id, d.display_name);
  }

  const localId = snapshot.identity?.device_id ?? 'local';
  const localName = snapshot.identity?.display_name || '本机设备';

  const rawRows: Omit<DeviceComparisonRow, 'sharePct'>[] = [
    {
      deviceId: localId,
      name: localName,
      isLocal: true,
      totalTokens: snapshot.local.total_tokens,
      recordCount: snapshot.local.record_count,
      estimatedCostMicrousd: snapshot.local.estimated_cost_microusd,
    },
  ];

  for (const remote of snapshot.remote_by_device) {
    if (remote.owner_device_id === localId) continue;
    rawRows.push({
      deviceId: remote.owner_device_id,
      name: nameMap.get(remote.owner_device_id) ?? `远端设备 (${remote.owner_device_id.slice(0, 6)})`,
      isLocal: false,
      totalTokens: remote.total_tokens,
      recordCount: remote.record_count,
      estimatedCostMicrousd: remote.estimated_cost_microusd,
    });
  }

  const grandTotal = rawRows.reduce((sum, r) => sum + r.totalTokens, 0);

  return rawRows
    .map((r) => ({
      ...r,
      sharePct: grandTotal > 0 ? (r.totalTokens / grandTotal) * 100 : 0,
    }))
    .sort((a, b) => b.totalTokens - a.totalTokens);
}
