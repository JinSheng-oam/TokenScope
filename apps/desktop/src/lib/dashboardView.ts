import { formatNumber } from './format';
import { buildHeatmapCalendar, formatWeekday } from './heatmap';
import type {
  DashboardRange,
  DashboardSnapshot,
  RemoteUsageBreakdown,
  UsageBreakdown,
  UsageTotals,
} from './types';

export type TrendViewMode = 'curve' | 'heatmap';
export type ModelSortBy = 'tokens' | 'cost';
export type DeviceScope = 'all' | 'local' | string;

export type DashboardProjectionOptions = {
  snapshot: DashboardSnapshot | null;
  heatmapDaily: DashboardSnapshot['daily'] | null;
  todayIso: string;
  hoveredCurveIndex: number | null;
  remoteSelectedDevice: string;
  deviceScope?: DeviceScope;
  modelSortBy: ModelSortBy;
  range: DashboardRange;
  customStart: string;
  customEnd: string;
};

export type ChartPoint = DashboardSnapshot['daily'][number] & {
  cache_tokens: number;
  weekdayLabel: string;
  x: number;
  y: number;
};

export function resolveScopedTotals(
  snapshot: DashboardSnapshot | null,
  deviceScope: DeviceScope = 'all',
): UsageTotals | undefined {
  if (!snapshot) return undefined;
  const local = snapshot.local;
  const remote = snapshot.remote;

  if (deviceScope === 'local') {
    return local;
  }

  if (deviceScope === 'all') {
    const hasLocalCost = local.estimated_cost_microusd !== null;
    const hasRemoteCost = remote.estimated_cost_microusd !== null;
    return {
      total_tokens: local.total_tokens + remote.total_tokens,
      input_tokens: local.input_tokens + remote.input_tokens,
      cache_read_tokens: local.cache_read_tokens + remote.cache_read_tokens,
      cache_write_tokens: local.cache_write_tokens + remote.cache_write_tokens,
      output_tokens: local.output_tokens + remote.output_tokens,
      unclassified_tokens: local.unclassified_tokens + remote.unclassified_tokens,
      reasoning_tokens: local.reasoning_tokens,
      reasoning_known_records: local.reasoning_known_records,
      record_count: local.record_count + remote.record_count,
      estimated_cost_microusd:
        hasLocalCost || hasRemoteCost
          ? (local.estimated_cost_microusd ?? 0) + (remote.estimated_cost_microusd ?? 0)
          : null,
      priced_records: local.priced_records + remote.priced_records,
      unpriced_records: local.unpriced_records + remote.unpriced_records,
      exact_records: local.exact_records + remote.exact_records,
      aggregate_records: local.aggregate_records + remote.aggregate_records,
      estimated_records: local.estimated_records + remote.estimated_records,
      unknown_precision_records: local.unknown_precision_records + remote.unknown_precision_records,
    };
  }

  // Specific remote device
  const devEntry = snapshot.remote_by_device.find((d) => d.owner_device_id === deviceScope);
  if (!devEntry) {
    return {
      total_tokens: 0,
      input_tokens: 0,
      cache_read_tokens: 0,
      cache_write_tokens: 0,
      output_tokens: 0,
      unclassified_tokens: 0,
      reasoning_tokens: 0,
      reasoning_known_records: 0,
      record_count: 0,
      estimated_cost_microusd: null,
      priced_records: 0,
      unpriced_records: 0,
      exact_records: 0,
      aggregate_records: 0,
      estimated_records: 0,
      unknown_precision_records: 0,
    };
  }

  // When there is a single remote device, snapshot.remote holds its exact component breakdown
  if (snapshot.remote_by_device.length === 1) {
    return {
      total_tokens: remote.total_tokens,
      input_tokens: remote.input_tokens,
      cache_read_tokens: remote.cache_read_tokens,
      cache_write_tokens: remote.cache_write_tokens,
      output_tokens: remote.output_tokens,
      unclassified_tokens: remote.unclassified_tokens,
      reasoning_tokens: 0,
      reasoning_known_records: 0,
      record_count: remote.record_count,
      estimated_cost_microusd: remote.estimated_cost_microusd,
      priced_records: remote.priced_records,
      unpriced_records: remote.unpriced_records,
      exact_records: remote.exact_records,
      aggregate_records: remote.aggregate_records,
      estimated_records: remote.estimated_records,
      unknown_precision_records: remote.unknown_precision_records,
    };
  }

  const ratio = remote.total_tokens > 0 ? devEntry.total_tokens / remote.total_tokens : 0;
  return {
    total_tokens: devEntry.total_tokens,
    input_tokens: Math.round(remote.input_tokens * ratio),
    cache_read_tokens: Math.round(remote.cache_read_tokens * ratio),
    cache_write_tokens: Math.round(remote.cache_write_tokens * ratio),
    output_tokens: Math.round(remote.output_tokens * ratio),
    unclassified_tokens: Math.round(remote.unclassified_tokens * ratio),
    reasoning_tokens: 0,
    reasoning_known_records: 0,
    record_count: devEntry.record_count,
    estimated_cost_microusd: devEntry.estimated_cost_microusd,
    priced_records: devEntry.estimated_cost_microusd !== null ? devEntry.record_count : 0,
    unpriced_records: devEntry.estimated_cost_microusd === null ? devEntry.record_count : 0,
    exact_records: devEntry.record_count,
    aggregate_records: 0,
    estimated_records: 0,
    unknown_precision_records: 0,
  };
}

export function resolveScopedBreakdown(
  localRows: UsageBreakdown[],
  remoteRows: RemoteUsageBreakdown[],
  deviceScope: DeviceScope = 'all',
): UsageBreakdown[] {
  if (deviceScope === 'local') {
    return localRows;
  }

  const map = new Map<string, UsageBreakdown>();

  if (deviceScope === 'all') {
    for (const row of localRows) {
      map.set(row.key, { ...row });
    }
    for (const r of remoteRows) {
      const existing = map.get(r.key);
      if (existing) {
        existing.total_tokens += r.total_tokens;
        existing.record_count += r.record_count;
        if (r.estimated_cost_microusd !== null) {
          existing.estimated_cost_microusd =
            (existing.estimated_cost_microusd ?? 0) + r.estimated_cost_microusd;
        }
        existing.exact_records += r.record_count;
      } else {
        map.set(r.key, {
          key: r.key,
          label: r.label,
          total_tokens: r.total_tokens,
          record_count: r.record_count,
          estimated_cost_microusd: r.estimated_cost_microusd,
          exact_records: r.record_count,
          aggregate_records: 0,
          estimated_records: 0,
          unknown_precision_records: 0,
          untimed_records: 0,
        });
      }
    }
  } else {
    for (const r of remoteRows) {
      if (r.owner_device_id !== deviceScope) continue;
      const existing = map.get(r.key);
      if (existing) {
        existing.total_tokens += r.total_tokens;
        existing.record_count += r.record_count;
        if (r.estimated_cost_microusd !== null) {
          existing.estimated_cost_microusd =
            (existing.estimated_cost_microusd ?? 0) + r.estimated_cost_microusd;
        }
      } else {
        map.set(r.key, {
          key: r.key,
          label: r.label,
          total_tokens: r.total_tokens,
          record_count: r.record_count,
          estimated_cost_microusd: r.estimated_cost_microusd,
          exact_records: r.record_count,
          aggregate_records: 0,
          estimated_records: 0,
          unknown_precision_records: 0,
          untimed_records: 0,
        });
      }
    }
  }

  return Array.from(map.values()).sort((a, b) => b.total_tokens - a.total_tokens);
}

export function buildDashboardProjection(options: DashboardProjectionOptions) {
  const { snapshot } = options;
  const scope: DeviceScope = options.deviceScope ?? 'all';
  const local = resolveScopedTotals(snapshot, scope);
  const scopedTools = resolveScopedBreakdown(
    snapshot?.by_tool ?? [],
    snapshot?.remote_by_tool ?? [],
    scope,
  );
  const scopedModels = resolveScopedBreakdown(
    snapshot?.by_model ?? [],
    snapshot?.remote_by_model ?? [],
    scope,
  );
  const scopedProjects = resolveScopedBreakdown(
    snapshot?.by_project ?? [],
    snapshot?.remote_by_project ?? [],
    scope,
  );
  const chartValues = snapshot?.daily ?? [];
  const chartPoints = buildChartPoints(chartValues);
  const tokenShares = getTokenShares(local);
  const remoteFilter = (row: { owner_device_id: string }) =>
    options.remoteSelectedDevice === 'all' || row.owner_device_id === options.remoteSelectedDevice;
  const sessionRangeLabel = buildRangeLabel(options.range, options.customStart, options.customEnd);

  return {
    local,
    scopedTools,
    scopedProjects,
    chartValues,
    chartMax: Math.max(1, ...chartValues.map((item) => item.total_tokens)),
    chartPoints,
    chartPath: buildChartPath(chartPoints),
    hoveredCurvePoint:
      options.hoveredCurveIndex !== null && chartPoints[options.hoveredCurveIndex]
        ? chartPoints[options.hoveredCurveIndex]
        : null,
    heatmapCalendar: buildHeatmapCalendar(
      options.heatmapDaily ?? chartValues,
      options.todayIso || chartValues[chartValues.length - 1]?.date || '',
    ),
    topToolTotal: Math.max(1, ...scopedTools.map((item) => item.total_tokens)),
    topModelTotal: Math.max(1, ...scopedModels.map((item) => item.total_tokens)),
    antigravityUsage: scopedTools.find((item) => item.key === 'antigravity'),
    remoteToolRows: (snapshot?.remote_by_tool ?? []).filter(remoteFilter),
    remoteModelRows: (snapshot?.remote_by_model ?? []).filter(remoteFilter),
    sortedModels: sortModelsBy(scopedModels, options.modelSortBy),
    tokenShares,
    exactRatio: local?.record_count
      ? ((local.exact_records / local.record_count) * 100).toFixed(1)
      : '—',
    rangeLabel: sessionRangeLabel,
  };
}

export function buildChartPoints(daily: DashboardSnapshot['daily']): ChartPoint[] {
  const chartMax = Math.max(1, ...daily.map((item) => item.total_tokens));
  return daily.map((item, index) => ({
    ...item,
    cache_tokens: Math.max(0, item.total_tokens - item.input_tokens - item.output_tokens),
    weekdayLabel: formatWeekday(item.date),
    x: daily.length <= 1 ? 420 : 64 + (index / (daily.length - 1)) * 708,
    y: 156 - (item.total_tokens / chartMax) * 124,
  }));
}

export function buildChartPath(points: ChartPoint[]): string {
  return points.map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x} ${point.y}`).join(' ');
}

export function formatCompactTokens(value: number): string {
  if (value >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1)}B`;
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(Math.round(value));
}

export function buildRangeLabel(
  range: DashboardRange,
  customStart: string,
  customEnd: string
): string {
  return range === 'all'
    ? '全部'
    : range === 'today'
      ? '今天'
      : range === '7d'
        ? '7 天'
        : range === '30d'
          ? '30 天'
          : `${customStart} 至 ${customEnd}`;
}

export function sortModelsBy(
  models: UsageBreakdown[],
  sortBy: ModelSortBy
): UsageBreakdown[] {
  return [...models].sort((a, b) => {
    if (sortBy === 'cost') {
      return (b.estimated_cost_microusd ?? 0) - (a.estimated_cost_microusd ?? 0);
    }
    return b.total_tokens - a.total_tokens;
  });
}

export function getTokenShares(local: UsageTotals | undefined): {
  cacheTokens: number;
  cacheHitRate: string;
  inputShareRate: string;
  outputShareRate: string;
  cacheShareRate: string;
  reasoningShareRate: string;
} {
  const cacheTokens = (local?.cache_read_tokens ?? 0) + (local?.cache_write_tokens ?? 0);
  const cacheInputTokens =
    (local?.input_tokens ?? 0) +
    (local?.cache_read_tokens ?? 0) +
    (local?.cache_write_tokens ?? 0);
  const total = local?.total_tokens ?? 0;
  return {
    cacheTokens,
    cacheHitRate: cacheInputTokens
      ? (((local?.cache_read_tokens ?? 0) / cacheInputTokens) * 100).toFixed(1)
      : '0.0',
    inputShareRate: percentShare(local?.input_tokens ?? 0, total),
    outputShareRate: percentShare(local?.output_tokens ?? 0, total),
    cacheShareRate: percentShare(cacheTokens, total),
    reasoningShareRate: percentShare(local?.reasoning_tokens ?? 0, local?.output_tokens ?? 0),
  };
}

function percentShare(value: number, total: number): string {
  return total ? ((value / total) * 100).toFixed(1) : '0.0';
}

export function quotaWindowLabel(durationMinutes: number): string {
  if (durationMinutes === 300) return '5 小时周期';
  if (durationMinutes === 10_080) return '每周周期';
  if (durationMinutes === 1_440) return '每日周期';
  return `${formatNumber(durationMinutes)} 分钟周期`;
}

export function antigravityWindowLabel(windowId: string): string {
  if (windowId === 'five_hour') return '5 小时周期';
  if (windowId === 'weekly') return '每周周期';
  if (windowId === 'daily') return '每日周期';
  return '其他周期';
}

export function quotaIsStale(observedAtMs: number, now = Date.now()): boolean {
  return now - observedAtMs > 15 * 60_000;
}

export function formatQuotaPoolName(poolId: string): string {
  const normalized = poolId.toLowerCase().trim();
  if (normalized === 'claude_gpt') return 'Claude / GPT';
  if (normalized === 'gemini_4_argon') return 'Gemini 4 Argon';
  if (normalized === 'gemini') return 'Gemini';
  if (normalized === 'codex') return 'Codex';
  return poolId
    .split(/[_-]/)
    .filter(Boolean)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(' ');
}

export function formatCountdown(resetAtMs: number | null, now = Date.now()): string {
  if (resetAtMs === null) return '未提供重置时间';
  const diff = resetAtMs - now;
  if (diff <= 0) return '即将重置';
  const totalHours = Math.floor(diff / 3_600_000);
  const mins = Math.floor((diff % 3_600_000) / 60_000);
  const secs = Math.floor((diff % 60_000) / 1000);

  if (totalHours >= 24) {
    const days = Math.floor(totalHours / 24);
    const remHours = totalHours % 24;
    return remHours > 0 ? `${days}天 ${remHours}小时 后重置` : `${days}天 后重置`;
  }
  return `${totalHours.toString().padStart(2, '0')}:${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')} 后重置`;
}
